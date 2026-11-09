//! File-backed OTA sink: `/api/ota` on the sim writes the image to
//! `tmp/host_board_firmware.bin` and verifies the SHA-256 from the query —
//! the exact request the `just ota` recipe fires at real hardware, so the
//! upload path itself is testable on the Mac.

use std::io::Write;

use led_core::board::{OtaBackend, OtaWriter};

use sha2::{Digest, Sha256};

const IMAGE_PATH: &str = "tmp/host_board_firmware.bin";

#[derive(Debug)]
pub enum FileOtaError {
    Io(String),
    HashMismatch { expected: String, got: String },
}

#[derive(Clone, Default)]
pub struct FileOtaBackend;

impl OtaBackend for FileOtaBackend {
    type Writer = FileOtaWriter;
    type Error = FileOtaError;

    fn begin(&self, expected_sha256: String) -> Result<Self::Writer, Self::Error> {
        if let Some(dir) = std::path::Path::new(IMAGE_PATH).parent() {
            let _ = std::fs::create_dir_all(dir);
        }

        let file = std::fs::File::create(IMAGE_PATH).map_err(|e| FileOtaError::Io(e.to_string()))?;

        println!("(sim) OTA started -> {IMAGE_PATH}");

        Ok(FileOtaWriter {
            file,
            hasher: Sha256::new(),
            expected_sha256,
            written: 0,
        })
    }
}

pub struct FileOtaWriter {
    file: std::fs::File,
    hasher: Sha256,
    expected_sha256: String,
    written: usize,
}

impl OtaWriter for FileOtaWriter {
    type Error = FileOtaError;

    fn write_chunk(&mut self, chunk: &[u8]) -> Result<(), Self::Error> {
        self.hasher.update(chunk);
        self.file.write_all(chunk).map_err(|e| FileOtaError::Io(e.to_string()))?;
        self.written += chunk.len();
        Ok(())
    }

    fn finalise(&mut self) -> Result<(), Self::Error> {
        self.file.flush().map_err(|e| FileOtaError::Io(e.to_string()))?;

        let got = format!("{:x}", self.hasher.finalize_reset());

        if got != self.expected_sha256 {
            return Err(FileOtaError::HashMismatch {
                expected: self.expected_sha256.clone(),
                got,
            });
        }

        println!("(sim) OTA complete: {} bytes, sha256 verified", self.written);

        Ok(())
    }
}
