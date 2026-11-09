use crate::board::{Board, OtaBackend, OtaWriter};
use alloc::{boxed::Box, string::ToString, vec::Vec};
use embedded_tools::{error, info, Debug2Format};
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
        // Watchdog interferes with the reboot process so we must disable it...
        self.board.stop_watchdog();

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

        if sum_str.len() != 64 || !sum_str.bytes().all(|b| b.is_ascii_hexdigit()) {
            return format_response!(request, response_writer, "Bad SHA256 length!\r\n");
        }
        let expected_sha256 = sum_str.to_string();

        info!("expected_sha256 = {}", expected_sha256.as_str());
        info!("OTA upload starting (compressed = {})", compressed);

        let mut infl = compressed.then(|| Box::new(InflateState::new(DataFormat::Raw)));

        let mut ota = match self.ota.begin(expected_sha256) {
            Ok(ota) => ota,
            Err(err) => {
                return format_response!(request, response_writer, "Could not start OTA: {err:?}\r\n");
            }
        };

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

            loop {
                if ci >= clen {
                    let mut got = 0usize;
                    while got < cbuf.len() {
                        match reader.read(&mut cbuf[got..]).await? {
                            0 => break,
                            n => got += n,
                        }
                    }
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
            }
        } else {
            // Plain path: one 4 KB read per flash chunk.
            loop {
                let mut read_size = 0;

                // Make sure the buffer is full
                loop {
                    let chunk_read_bytes = reader.read(&mut buffer[read_size..]).await?;
                    read_size += chunk_read_bytes;
                    if chunk_read_bytes == 0 {
                        break;
                    }
                }

                if read_size == 0 {
                    break;
                }

                if let Err(err) = ota.write_chunk(&buffer[0..read_size]) {
                    return format_response!(request, response_writer, "Could not write flash: {err:?}\r\n");
                }

                total_size += read_size;
            }
        }

        info!("OTA wrote {} bytes — finalising (hash verify)...", total_size);

        if let Err(err) = ota.finalise() {
            error!("OTA verification FAILED after {} bytes: {}", total_size, Debug2Format(&err));
            return format_response!(request, response_writer, "Verification Error: {err:?}\r\n");
        }

        info!("OTA verified — rebooting into new image (Total Size: {})", total_size);

        return format_response!(request, response_writer, "Total Size: {total_size}\r\nWill reboot...");
    }
}
