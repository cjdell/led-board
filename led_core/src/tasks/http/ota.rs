use crate::board::{Board, OtaBackend, OtaWriter};
use alloc::{boxed::Box, string::ToString, vec::Vec};
use embassy_time::{Duration, Timer};
use embedded_tools::{Debug2Format, error, info};
use miniz_oxide::inflate::stream::{InflateState, inflate as mz_inflate};
use miniz_oxide::{DataFormat, MZFlush};
use picoserve::{
    ResponseSent,
    io::Read,
    request::Request,
    response::{IntoResponse, ResponseWriter},
    routing::RequestHandlerService,
};

pub struct OtaUpdateHandler<B: Board> {
    ota: B::Ota,
    board: B,
}

impl<B: Board> OtaUpdateHandler<B> {
    pub fn new(ota: B::Ota, board: B) -> Self {
        Self { ota, board }
    }
}

impl<B: Board> RequestHandlerService<()> for OtaUpdateHandler<B>
where
    <B::Ota as OtaBackend>::Writer: OtaWriter,
{
    async fn call_request_handler_service<R: Read, W: ResponseWriter<Error = R::Error>>(
        &self,
        (): &(),
        (): (),
        mut request: Request<'_, R>,
        response_writer: W,
    ) -> Result<ResponseSent, W::Error> {
        let mut reader = request.body_connection.body().reader();

        let query = match request.parts.query() {
            Some(q) => q,
            None => {
                return format_response!(request, response_writer, "Missing SHA256 sum!\r\n");
            }
        };

        // Query is `<sha256>` or `<sha256>&<flags>`; flag `z=raw` means the
        // body is a raw DEFLATE stream of the image (built by `just ota`),
        // which cuts wire time ~2.5×. No flag = raw bytes (old behaviour).
        let q: &str = query.0;
        let (sum_str, flags) = match q.split_once('&') {
            Some((sum, flags)) => (sum, flags),
            None => (q, ""),
        };
        let compressed = flags.contains("z=raw");

        if compressed {
            // The DEFLATE state is ~44 KB and its constructor builds that on
            // the stack before boxing it, which resets the chip before a log
            // can escape (no panic record — it looks like a watchdog reset).
            // Raw uploads work reliably with a paced client (`just ota`), so
            // do not pretend to support the compressed flag.
            return format_response!(
                request,
                response_writer,
                "z=raw is disabled on this build — POST the raw image (no z=raw)\r\n"
            );
        }

        if sum_str.len() != 64 || !sum_str.bytes().all(|b| b.is_ascii_hexdigit()) {
            return format_response!(request, response_writer, "Bad SHA256 length!\r\n");
        }
        let expected_sha256 = sum_str.to_string();

        info!("expected_sha256 = {}", expected_sha256.as_str());
        info!("OTA upload starting (compressed = {})", compressed);

        // `new_boxed` allocates the ~44 KB state on the heap. `Box::new(
        // InflateState::new(..))` builds it on the stack first, which blows
        // the task stack and silently resets the chip before any log escapes.
        let mut infl = compressed.then(|| InflateState::new_boxed(DataFormat::Raw));

        let mut ota = match self.ota.begin(expected_sha256) {
            Ok(ota) => ota,
            Err(err) => {
                return format_response!(request, response_writer, "Could not start OTA: {err:?}\r\n");
            }
        };

        // `begin` sets `OTA_ACTIVE`, so the 5 s feeder has already stopped
        // re-arming the watchdog by the time we disable it here. Doing this
        // before `begin` left a race where the feeder re-armed a 15 s timer
        // and reset the board mid-upload (B stayed empty).
        self.board.stop_watchdog();

        let mut buffer = Vec::new();
        buffer.resize(4096, 0u8);
        let mut total_size = 0;

        if let Some(state) = infl.as_mut() {
            // Inflate path: pull compressed bytes, pump the decoder, and hand
            // 4096-aligned pieces to the flash (the tail is one short chunk).
            let mut cbuf = [0u8; 1024];
            let mut pending: Vec<u8> = Vec::with_capacity(4096 + 1024);
            let mut ci = 0usize;
            let mut clen = 0usize;
            let mut eof = false;
            // Same Content-Length bound as the plain path: never call `read`
            // once the declared body has been consumed (the reader would block
            // on a keep-alive connection forever).
            let body_length = reader.content_length();
            let mut consumed = 0usize;

            loop {
                if ci >= clen {
                    let mut got = 0usize;
                    while got < cbuf.len() && consumed + got < body_length {
                        match reader.read(&mut cbuf[got..]).await? {
                            0 => break,
                            n => got += n,
                        }
                    }
                    consumed += got;
                    clen = got;
                    ci = 0;
                    if got == 0 {
                        eof = true;
                    }
                }

                let result = mz_inflate(state, &cbuf[ci..clen], &mut buffer[..], MZFlush::None);
                if result.status.is_err() {
                    return format_response!(request, response_writer, "Inflate failed: {:?}\r\n", result.status);
                }
                ci += result.bytes_consumed;
                pending.extend_from_slice(&buffer[..result.bytes_written]);

                while pending.len() >= 4096 {
                    if let Err(err) = ota.write_chunk(&pending[..4096]) {
                        return format_response!(request, response_writer, "Could not write flash: {err:?}\r\n");
                    }
                    total_size += 4096;
                    if total_size % 65536 == 0 {
                        info!("OTA progress: {} bytes", total_size);
                    }
                    pending.drain(..4096);
                    // Let the cyw43/network task service the TCP window
                    // between flash operations. `Flash::blocking_erase` runs
                    // in RAM with interrupts disabled for ~50 ms per sector;
                    // back-to-back writes starve WiFi for seconds and the
                    // upload stalls/drops its connection.
                    Timer::after(Duration::from_millis(5)).await;
                }

                if result.bytes_consumed == 0 && result.bytes_written == 0 {
                    if !eof && ci >= clen {
                        continue; // pull more input
                    }
                    if eof {
                        break; // drained
                    }
                    return format_response!(request, response_writer, "Deflate stream stalled\r\n");
                }
            }

            if !pending.is_empty() {
                if let Err(err) = ota.write_chunk(&pending) {
                    return format_response!(request, response_writer, "Could not write flash: {err:?}\r\n");
                }
                total_size += pending.len();
                Timer::after(Duration::from_millis(5)).await;
            }
        } else {
            // Plain path: one 4 KB read per flash chunk.
            //
            // Stop at `content_length` instead of waiting for the reader to
            // return 0. picoserve's body reader blocks on the socket once the
            // declared body is consumed if the client keeps the connection
            // open, which wedged the handler forever on every real upload.
            let content_length = reader.content_length();
            info!("OTA upload: {} bytes", content_length);
            loop {
                let mut read_size = 0;

                // Fill the buffer, but never read past the declared body.
                while read_size < buffer.len() && total_size + read_size < content_length {
                    let chunk_read_bytes = reader.read(&mut buffer[read_size..]).await?;
                    if chunk_read_bytes == 0 {
                        break;
                    }
                    read_size += chunk_read_bytes;
                }

                if read_size == 0 {
                    break;
                }

                if let Err(err) = ota.write_chunk(&buffer[0..read_size]) {
                    return format_response!(request, response_writer, "Could not write flash: {err:?}\r\n");
                }

                total_size += read_size;
                if total_size % 65536 < read_size {
                    info!("OTA progress: {} bytes", total_size);
                }
                // Give the cyw43/network stack time to drain between flash
                // sectors (each erase blocks with interrupts off for ~30 ms).
                Timer::after(Duration::from_millis(20)).await;
            }
        }

        info!("OTA wrote {} bytes — finalising (hash verify)...", total_size);

        if let Err(err) = ota.finalise() {
            error!(
                "OTA verification FAILED after {} bytes: {}",
                total_size,
                Debug2Format(&err)
            );
            return format_response!(request, response_writer, "Verification Error: {err:?}\r\n");
        }

        info!("OTA verified — rebooting into new image (Total Size: {})", total_size);

        return format_response!(request, response_writer, "Total Size: {total_size}\r\nWill reboot...");
    }
}
