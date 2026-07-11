use crate::{flash::FlashStorage, types::*};
use alloc::{string::ToString, vec::Vec};
use defmt::info;
use embassy_time::{Duration, Timer};
use picoserve::{
    ResponseSent,
    io::Read,
    request::Request,
    response::{IntoResponse, ResponseWriter},
    routing::RequestHandlerService,
};
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

        let expected_sha256 = match query.try_into_string::<64>() {
            Ok(s) => s.to_string(),
            Err(err) => {
                return format_response!(request, response_writer, "Bad SHA256 length! {}\r\n", err);
            }
        };

        info!("expected_sha256 = {}", expected_sha256.as_str());

        let mut ota = match Rp235xOta::new(self.flash_storage.clone(), expected_sha256) {
            Ok(ota) => ota,
            Err(err) => {
                return format_response!(request, response_writer, "Could not start OTA: {err:?}\r\n");
            }
        };

        let mut buffer = Vec::new();
        buffer.resize(4096, 0u8);
        let mut total_size = 0;

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

            Timer::after(Duration::from_millis(10)).await;

            // info!("Wrote {}", read_size);
        }

        info!("Done writing");

        if let Err(err) = ota.finalise() {
            return format_response!(request, response_writer, "Verification Error: {err:?}\r\n");
        }

        return format_response!(request, response_writer, "Total Size: {total_size}\r\nWill reboot...");
    }
}
