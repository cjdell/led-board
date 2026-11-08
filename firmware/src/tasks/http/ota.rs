use crate::types::*;
use alloc::{boxed::Box, string::ToString, vec::Vec};
use defmt::info;
use miniz_oxide::inflate::stream::{InflateState, inflate as mz_inflate};
use miniz_oxide::{DataFormat, MZFlush};
use picoserve::{
    ResponseSent,
    io::Read,
    request::Request,
    response::{IntoResponse, ResponseWriter},
    routing::RequestHandlerService,
};
use rp235x_embedded_tools::flash::FlashStorage;
use rp235x_ota::Rp235xOta;

pub struct OtaUpdateHandler {
    flash_storage: FlashStorage,
    watchdog: SharedWatchdog,
}

impl OtaUpdateHandler {
    pub fn new(flash_storage: FlashStorage, watchdog: SharedWatchdog) -> Self {
        Self {
            flash_storage,
            watchdog,
        }
    }
}

impl RequestHandlerService<()> for OtaUpdateHandler {
    async fn call_request_handler_service<R: Read, W: ResponseWriter<Error = R::Error>>(
        &self,
        (): &(),
        (): (),
        mut request: Request<'_, R>,
        response_writer: W,
    ) -> Result<ResponseSent, W::Error> {
        // Watchdog interferes with the reboot process so we must disable it...
        self.watchdog.try_write().unwrap().stop();

        // for _ in 0..5000000 {
        //     cortex_m::asm::nop();
        // }

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

        let mut infl = compressed.then(|| Box::new(InflateState::new(DataFormat::Raw)));

        let mut ota = match Rp235xOta::new(self.flash_storage.clone(), expected_sha256) {
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

        info!("Done writing");

        if let Err(err) = ota.finalise() {
            return format_response!(request, response_writer, "Verification Error: {err:?}\r\n");
        }

        return format_response!(request, response_writer, "Total Size: {total_size}\r\nWill reboot...");
    }
}
