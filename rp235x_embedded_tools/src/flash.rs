use alloc::sync::Arc;
use block_device::BlockDevice;
use defmt::info;
use embassy_rp::{
    flash::{Async, Flash},
    peripherals::FLASH,
};
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, rwlock::RwLock};

const FLASH_BASE: u32 = 0x1c000000;
const FLASH_SIZE: usize = 4 * 1024 * 1024;
const BLOCK_SIZE: usize = 4 * 1024;

#[derive(Clone)]
pub struct FlashStorage {
    flash: Arc<RwLock<CriticalSectionRawMutex, &'static mut Flash<'static, FLASH, Async, FLASH_SIZE>>>,
}

impl FlashStorage {
    pub fn new(
        flash: Arc<RwLock<CriticalSectionRawMutex, &'static mut Flash<'static, FLASH, Async, FLASH_SIZE>>>,
    ) -> Self {
        Self { flash }
    }
}

impl BlockDevice for FlashStorage {
    const BLOCK_SIZE: u32 = BLOCK_SIZE as u32;

    type Error = FlashStorageError;

    fn read(&self, buf: &mut [u8], address: usize, number_of_blocks: usize) -> Result<(), Self::Error> {
        if address % Self::BLOCK_SIZE as usize != 0 {
            return Err(FlashStorageError::AddressNotAligned);
        }

        if buf.len() < number_of_blocks * Self::BLOCK_SIZE as usize {
            return Err(FlashStorageError::BufferSizeIncorrect);
        }

        let flash_data = unsafe {
            core::slice::from_raw_parts(
                (FLASH_BASE + address as u32) as *const u8,
                number_of_blocks * Self::BLOCK_SIZE as usize,
            )
        };

        buf.copy_from_slice(flash_data);

        Ok(())
    }

    fn write(&self, buf: &[u8], address: usize, number_of_blocks: usize) -> Result<(), Self::Error> {
        if address % Self::BLOCK_SIZE as usize != 0 {
            return Err(FlashStorageError::AddressNotAligned);
        }

        if buf.len() > number_of_blocks * Self::BLOCK_SIZE as usize {
            return Err(FlashStorageError::BufferSizeIncorrect);
        }

        let mut flash = self.flash.try_write().map_err(|_| FlashStorageError::FlashBusy)?;

        for block_index in 0..number_of_blocks {
            let position = block_index * Self::BLOCK_SIZE as usize;
            let block_end = ((block_index + 1) + Self::BLOCK_SIZE as usize).min(buf.len());
            let absolute_position = address + position;

            let block = &buf[position..block_end];

            flash
                .blocking_erase(absolute_position as u32, absolute_position as u32 + Self::BLOCK_SIZE)
                .map_err(|_| FlashStorageError::WriteFailed)?;

            flash
                .blocking_write(absolute_position as u32, block)
                .map_err(|_| FlashStorageError::WriteFailed)?;
        }

        Ok(())
    }
}

#[derive(Debug)]
pub enum FlashStorageError {
    AddressNotAligned,
    BufferSizeIncorrect,
    FlashBusy,
    WriteFailed,
}

const LITTLEFS_FLASH_OFFSET: u32 = 0x300000;

pub struct LittleFsFlashStorage {
    flash: Arc<RwLock<CriticalSectionRawMutex, &'static mut Flash<'static, FLASH, Async, FLASH_SIZE>>>,
}

impl LittleFsFlashStorage {
    pub fn new(
        flash: Arc<RwLock<CriticalSectionRawMutex, &'static mut Flash<'static, FLASH, Async, FLASH_SIZE>>>,
    ) -> Self {
        Self { flash }
    }
}

impl littlefs_rust::Storage for LittleFsFlashStorage {
    /// Read `buf.len()` bytes starting at `offset` within `block`.
    fn read(&mut self, block: u32, offset: u32, buf: &mut [u8]) -> Result<(), littlefs_rust::Error> {
        // Calculate the absolute address in flash
        let block_address = LITTLEFS_FLASH_OFFSET + (block * BLOCK_SIZE as u32);
        let absolute_address = block_address + offset;

        // Ensure we don't read beyond the block boundary
        if offset as usize + buf.len() > BLOCK_SIZE {
            return Err(littlefs_rust::Error::Io);
        }

        // Ensure we don't read beyond flash size
        let flash_end = FLASH_SIZE as u32;
        if absolute_address as usize + buf.len() > flash_end as usize {
            return Err(littlefs_rust::Error::Io);
        }

        // Read directly from flash memory (memory-mapped)
        let flash_data =
            unsafe { core::slice::from_raw_parts((FLASH_BASE + absolute_address) as *const u8, buf.len()) };

        buf.copy_from_slice(flash_data);

        Ok(())
    }

    /// Write `data` starting at `offset` within `block`.
    ///
    /// The block must have been erased before writing.
    fn write(&mut self, block: u32, offset: u32, data: &[u8]) -> Result<(), littlefs_rust::Error> {
        // Calculate the absolute address in flash
        let block_address = LITTLEFS_FLASH_OFFSET + (block * BLOCK_SIZE as u32);
        let absolute_address = block_address + offset;

        // Ensure we don't write beyond the block boundary
        if offset as usize + data.len() > BLOCK_SIZE {
            return Err(littlefs_rust::Error::Io);
        }

        // Ensure we don't write beyond flash size
        let flash_end = FLASH_SIZE as u32;
        if absolute_address as usize + data.len() > flash_end as usize {
            return Err(littlefs_rust::Error::Io);
        }

        // Get exclusive access to the flash
        let mut flash = self.flash.try_write().map_err(|_| littlefs_rust::Error::Io)?;

        // Write the data
        flash
            .blocking_write(absolute_address, data)
            .map_err(|_| littlefs_rust::Error::Io)?;

        Ok(())
    }

    /// Erase `block`, resetting all bytes to the erased state (typically `0xFF`).
    fn erase(&mut self, block: u32) -> Result<(), littlefs_rust::Error> {
        // Calculate the absolute address in flash
        let block_address = LITTLEFS_FLASH_OFFSET + (block * BLOCK_SIZE as u32);

        // Ensure the block is within flash bounds
        let flash_end = FLASH_SIZE as u32;
        if block_address + BLOCK_SIZE as u32 > flash_end {
            return Err(littlefs_rust::Error::Io);
        }

        // Get exclusive access to the flash
        let mut flash = self.flash.try_write().map_err(|_| littlefs_rust::Error::Io)?;

        // Erase the entire block
        flash
            .blocking_erase(block_address, block_address + BLOCK_SIZE as u32)
            .map_err(|_| littlefs_rust::Error::Io)?;

        Ok(())
    }

    fn sync(&mut self) -> Result<(), littlefs_rust::Error> {
        info!("LittleFsFlashStorage.sync()");

        Ok(())
    }
}
