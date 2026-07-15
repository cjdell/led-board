#![no_std]
#![no_main]

extern crate alloc;

use alloc::vec::Vec;
use alloc::{format, string::String};
use block_device::BlockDevice;
use core::fmt::Debug;
use defmt::info;
use embassy_rp::block::Partition;
use embassy_rp::rom_data;
use sha2::{Digest, Sha256};

const REBOOT2_FLAG_REBOOT_TYPE_FLASH_UPDATE: u32 = 0x4;

const RP2350_ARM_S: u32 = 0xe48bff59;

const FLASH_BASE_RAW: u32 = 0x1c000000; // XIP_NOCACHE_NOALLOC_NOTRANSLATE_BASE
const FLASH_BASE: u32 = 0x10000000;
const FLASH_SIZE: usize = 4 * 1024 * 1024;
const BLOCK_SIZE: usize = 4 * 1024;

const SYS_INFO_BOOT_INFO: u32 = 0x0040;
const BOOT_TBYB_AND_UPDATE_FLAG_BUY_PENDING: u8 = 0x1;
const BOOT_TYPE_FLASH_UPDATE: u8 = 0x4;

pub fn mark_firmware_good() {
    info!("Rp235xOta: mark_firmware_good()");

    let mut workarea = [0u8; 4096];

    let mut sys_info_buffer = [0u32; 5];

    let get_sys_info_result = unsafe { rom_data::get_sys_info(sys_info_buffer.as_mut_ptr(), 5, SYS_INFO_BOOT_INFO) };
    info!("Rp235xOta: get_sys_info: {}", get_sys_info_result);
    info!("Rp235xOta: sys_info_buffer: {}", sys_info_buffer);

    let boot_type = ((sys_info_buffer[1] & 0x0000FF00) >> 8) as u8;

    if boot_type & BOOT_TYPE_FLASH_UPDATE > 0 {
        info!("Rp235xOta: Marking this firmware as good...");

        let buy_result = unsafe { rom_data::explicit_buy(workarea.as_mut_ptr(), workarea.len() as u32) };
        info!("Rp235xOta: buy_result: {}", buy_result);
    } else {
        info!("Rp235xOta: Not a flash update, ignoring...");
    }
}

// 1.038942 [INFO ] Rp235xOta: get_sys_info: 5 (rp235x_ota rp235x_ota/src/lib.rs:33)
// 1.038965 [INFO ] Rp235xOta: sys_info_buffer: [64, 255, 0, 0, 0] (rp235x_ota rp235x_ota/src/lib.rs:34)
// 1.039007 [INFO ] Rp235xOta: buy_result: 0 (rp235x_ota rp235x_ota/src/lib.rs:37)

// 1.077197 [INFO ] Rp235xOta: get_sys_info: 5 (rp235x_ota rp235x_ota/src/lib.rs:33)
// 1.077219 [INFO ] Rp235xOta: sys_info_buffer: [64, 67175679, 0, 268959744, 0] (rp235x_ota rp235x_ota/src/lib.rs:34)   0x040104FF 0x0 0x10080000 0x0
// 1.077280 [INFO ] Rp235xOta: buy_result: 0 (rp235x_ota rp235x_ota/src/lib.rs:37)

// 0.077014 [INFO ] Rp235xOta: get_sys_info: 5 (rp235x_ota rp235x_ota/src/lib.rs:33)
// 0.077038 [INFO ] Rp235xOta: sys_info_buffer: [64, 1279, 0, 268439552, 0] (rp235x_ota rp235x_ota/src/lib.rs:34)   0x000004FF 0x0 0x10001000 0x0
// 0.077078 [INFO ] Rp235xOta: Marking this firmware as good... (rp235x_ota rp235x_ota/src/lib.rs:28)
// 0.077099 [INFO ] Rp235xOta: buy_result: 0 (rp235x_ota rp235x_ota/src/lib.rs:37)

// 0.077287 [INFO ] Rp235xOta: get_sys_info: 5 (rp235x_ota rp235x_ota/src/lib.rs:33)
// 0.077310 [INFO ] Rp235xOta: sys_info_buffer: [64, 67175679, 0, 268959744, 0] (rp235x_ota rp235x_ota/src/lib.rs:34)
// 0.077351 [INFO ] Rp235xOta: Marking this firmware as good... (rp235x_ota rp235x_ota/src/lib.rs:28)
// 0.077371 [INFO ] Rp235xOta: buy_result: 0 (rp235x_ota rp235x_ota/src/lib.rs:37)

// 1.288594 [INFO ] Rp235xOta: get_sys_info: 5 (rp235x_ota rp235x_ota/src/lib.rs:33)
// 1.289059 [INFO ] Rp235xOta: sys_info_buffer: [64, 1279, 0, 268439552, 0] (rp235x_ota rp235x_ota/src/lib.rs:34)
// 1.289554 [INFO ] Rp235xOta: Marking this firmware as good... (rp235x_ota rp235x_ota/src/lib.rs:28)
// 1.289793 [INFO ] Rp235xOta: buy_result: 0 (rp235x_ota rp235x_ota/src/lib.rs:37)

// 0.000000 [INFO ] Rp235xOta: get_sys_info: 5 (rp235x_ota rp235x_ota/src/lib.rs:33)
// 0.000000 [INFO ] Rp235xOta: sys_info_buffer: [64, 254, 852821, 0, 0] (rp235x_ota rp235x_ota/src/lib.rs:34)       // 0x000000FE 0x000D0355 0x0 0x0
// 0.000000 [INFO ] Rp235xOta: Marking this firmware as good... (rp235x_ota rp235x_ota/src/lib.rs:28)
// 0.000000 [INFO ] Rp235xOta: buy_result: 0 (rp235x_ota rp235x_ota/src/lib.rs:37)

// 1.077039 [INFO ] Rp235xOta: sys_info_buffer: 5 (rp235x_ota rp235x_ota/src/lib.rs:34)
// 1.077062 [INFO ] Rp235xOta: sys_info_buffer: [64, 67175678, 852821, 268959744, 0] (utils src/config/mod.rs:85)   // 0x040104FE
// 1.077111 [INFO ] Rp235xOta: Marking this firmware as good... (rp235x_ota rp235x_ota/src/lib.rs:28)
// 1.077132 [INFO ] Rp235xOta: get_sys_info: 0 (rp235x_ota rp235x_ota/src/lib.rs:33)

#[derive(Debug)]
pub enum OtaError {
    Unknown,
    BadHashEncoding,
    BadHashLength,
    TargetPartitionNotFound,
    WriteOverflow,
    WriteError(String),
    ReadError(String),
    HashMismatch, // ✅ New error: hash verification failed
}

pub struct Rp235xOta<F: BlockDevice> {
    flash: F,
    expected_sha256: [u8; 32], // ✅ Store expected SHA256 hash
    // watchdog: Arc<RwLock<CriticalSectionRawMutex, Watchdog>>,
    start_addr: u32,
    end_addr: u32,
    position: u32,
}

impl<F: BlockDevice> Rp235xOta<F>
where
    <F as BlockDevice>::Error: Debug,
{
    pub fn new(
        flash: F,
        expected_sha256: String,
        // watchdog: Arc<RwLock<CriticalSectionRawMutex, Watchdog>>,
    ) -> Result<Rp235xOta<F>, OtaError> {
        let expected_sha256 = match hex::decode(expected_sha256) {
            Ok(vec) => vec,
            Err(_) => return Err(OtaError::BadHashEncoding),
        };

        let expected_sha256 = match expected_sha256.as_array::<32>() {
            Some(arr) => *arr,
            None => return Err(OtaError::BadHashLength),
        };

        let mut workarea = [0u8; 4096];
        let mut partition_out = [0u32; 2];

        let part_ptr: *mut u32 = &mut partition_out as *mut u32;

        let result = unsafe {
            rom_data::get_uf2_target_partition(workarea.as_mut_ptr(), workarea.len(), RP2350_ARM_S, part_ptr)
        };

        if result == 0xFF {
            return Err(OtaError::TargetPartitionNotFound);
        }

        let partition_index = result;
        let permissions_and_location = partition_out[0];
        let permissions_and_flags = partition_out[1];

        // Extract first and last sector
        let partition = Partition::from_raw(permissions_and_location, permissions_and_flags);

        let (start_addr, end_addr) = partition.get_first_last_bytes();

        info!("Rp235xOta: Target partition index: {}", partition_index);
        info!("Rp235xOta: Start: {:#x}, End: {:#x}", start_addr, end_addr);

        Ok(Rp235xOta {
            flash,
            expected_sha256, // ✅ Store expected hash
            // watchdog,
            start_addr,
            end_addr: end_addr + 1, // +1 to make end_addr exclusive
            position: 0,
        })
    }

    pub fn write_chunk(&mut self, chunk: &[u8]) -> Result<(), OtaError> {
        let offset = self.start_addr + self.position;

        if offset + chunk.len() as u32 > self.end_addr {
            return Err(OtaError::WriteOverflow);
        }

        // info!(
        //     "Rp235xOta: Writing chunk: {:#x}, Size: {} bytes, Progress: {} bytes",
        //     offset,
        //     chunk.len(),
        //     self.position
        // );

        self.flash
            .write(chunk, offset as usize, 1)
            .map_err(|err| OtaError::WriteError(format!("{:?}", err)))?;

        self.position += chunk.len() as u32;

        Ok(())
    }

    pub fn finalise(&self) -> Result<(), OtaError> {
        let total_size = self.position as usize;
        let flash_start = self.start_addr as usize;

        info!("Rp235xOta: Finalising OTA, verifying firmware integrity...");

        // Allocate buffer for reading back firmware
        let mut block_buffer = Vec::new();
        block_buffer.resize(BLOCK_SIZE, 0u8);

        let mut hasher = Sha256::new();

        let mut position = 0usize;

        while position < total_size {
            let block_end = (total_size - position).min(BLOCK_SIZE);

            // info!("Reading... {}", block_end);

            // Read entire firmware back from flash
            match self.flash.read(&mut block_buffer, flash_start + position, 1) {
                Ok(_) => {
                    // Compute SHA256 of the read data
                    hasher.update(&block_buffer[0..block_end]);
                }
                Err(err) => {
                    info!(
                        "Rp235xOta: Failed to read back firmware: {:?}",
                        defmt::Debug2Format(&err)
                    );

                    return Err(OtaError::ReadError(format!("{:?}", err)));
                }
            }

            // info!("Read {}", block_end);

            position += block_end;
        }

        let actual_sha256 = hasher.finalize().as_slice().to_vec();

        // Compare with expected hash
        let actual_sha256_bytes: [u8; 32] = actual_sha256
            .as_slice()
            .try_into()
            .expect("SHA256 output should be 64 bytes");

        if actual_sha256_bytes == self.expected_sha256 {
            info!("Rp235xOta: SHA256 verification SUCCESS!");
            info!("Rp235xOta: Rebooting into new firmware...");

            // self.watchdog.try_write().unwrap().stop();

            rom_data::reboot(
                REBOOT2_FLAG_REBOOT_TYPE_FLASH_UPDATE,
                1_000,
                FLASH_BASE + self.start_addr,
                0,
            );

            // self.watchdog.try_write().unwrap().trigger_reset();
        } else {
            info!("Rp235xOta: SHA256 verification FAILED!");
            info!("Expected: {:?}", hex::encode(&self.expected_sha256).as_str());
            info!("Actual:   {:?}", hex::encode(&actual_sha256_bytes).as_str());

            return Err(OtaError::HashMismatch);
        }

        Ok(())
    }
}
