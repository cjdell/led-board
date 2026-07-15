use alloc::{
    string::{String, ToString as _},
    sync::Arc,
    vec::Vec,
};
use core::str::{Utf8Error, from_utf8};
use defmt::info;
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, mutex::Mutex};
use littlefs_rust::{Config, Filesystem, OpenFlags, SeekFrom, Storage};

pub const BLOCK_SIZE: u32 = 4 * 1024;
pub const FILESYSTEM_SIZE: u32 = 1024 * 1024;

pub trait LocalFsTrait: Clone {
    fn list_files(&self) -> impl Future<Output = Result<Vec<String>, FsError>>;
    fn get_file_size(&self, file_name: &str) -> impl Future<Output = Result<u32, FsError>>;
    fn read_binary_chunk(&self, file_name: &str, pos: u32, size: u32)
    -> impl Future<Output = Result<Vec<u8>, FsError>>;
    fn write_binary_chunk(
        &self,
        file_name: &str,
        pos: u32,
        buf: &[u8],
        truncate: bool,
    ) -> impl Future<Output = Result<(), FsError>>;
    fn read_text_file(&self, file_name: &str) -> impl Future<Output = Result<String, FsError>>;
    fn write_text_file(&self, file_name: &str, text: String) -> impl Future<Output = Result<(), FsError>>;
    fn delete_file(&self, file_name: &str) -> impl Future<Output = Result<(), FsError>>;
}

pub struct LocalFs<STORAGE: Storage> {
    fs: Arc<Mutex<CriticalSectionRawMutex, Filesystem<STORAGE>>>,
}

impl<STORAGE: Storage> Clone for LocalFs<STORAGE> {
    fn clone(&self) -> Self {
        Self { fs: self.fs.clone() }
    }
}

impl<STORAGE: Storage> LocalFs<STORAGE> {
    pub fn format(io: &mut STORAGE) -> Result<(), FsError> {
        let config = Config::new(BLOCK_SIZE, FILESYSTEM_SIZE / BLOCK_SIZE);

        Filesystem::format(io, &config).map_err(|err| FsError::WriteError(err))?;

        Ok(())
    }

    pub fn new(io: STORAGE) -> Result<Self, FsError> {
        let config = Config::new(BLOCK_SIZE, FILESYSTEM_SIZE / BLOCK_SIZE);

        let fs = Filesystem::mount(io, config).map_err(|(err, _)| FsError::ReadError(err))?;
        let fs = Arc::new(Mutex::new(fs));

        Ok(Self { fs })
    }
}

impl<STORAGE: Storage> LocalFsTrait for LocalFs<STORAGE> {
    async fn list_files(&self) -> Result<Vec<String>, FsError> {
        let fs: &Filesystem<STORAGE> = &*self.fs.lock().await;

        let list = fs.list_dir("/").map_err(|err| FsError::OpenError(err))?;

        Ok(list.iter().map(|entry| entry.name.clone()).collect())
    }

    async fn get_file_size(&self, file_name: &str) -> Result<u32, FsError> {
        let fs: &Filesystem<STORAGE> = &*self.fs.lock().await;

        let metadata = fs.stat(file_name).map_err(|err| FsError::OpenError(err))?;

        Ok(metadata.size)
    }

    async fn read_binary_chunk(&self, file_name: &str, pos: u32, size: u32) -> Result<Vec<u8>, FsError> {
        if pos == 0 {
            info!("==== read_binary_chunk: {} {} {}", file_name, pos, size);
        }

        let mut buf = Vec::new();
        buf.resize(size as usize, 0u8);

        let fs: &Filesystem<STORAGE> = &*self.fs.lock().await;

        let file = fs
            .open(file_name, OpenFlags::READ)
            .map_err(|err| FsError::OpenError(err))?;

        file.seek(SeekFrom::Start(pos)).map_err(|err| FsError::SeekError(err))?;

        let bytes_read = file.read(&mut buf).map_err(|err| FsError::ReadError(err))?;

        buf.resize(bytes_read as usize, 0u8);

        Ok(buf)
    }

    async fn write_binary_chunk(&self, file_name: &str, pos: u32, buf: &[u8], truncate: bool) -> Result<(), FsError> {
        if pos == 0 {
            info!("==== write_binary_chunk: {} {}", file_name, pos);
        }

        let fs: &Filesystem<STORAGE> = &*self.fs.lock().await;

        let file = fs
            .open(file_name, OpenFlags::WRITE | OpenFlags::CREATE | OpenFlags::TRUNC)
            .map_err(|err| FsError::OpenError(err))?;

        file.seek(SeekFrom::Start(pos)).map_err(|err| FsError::SeekError(err))?;

        file.write(&buf).map_err(|err| FsError::WriteError(err))?;

        if truncate {
            file.truncate(pos + buf.len() as u32)
                .map_err(|err| FsError::WriteError(err))?;
        }

        Ok(())
    }

    async fn read_text_file(&self, file_name: &str) -> Result<String, FsError> {
        let chunk = &self.read_binary_chunk(file_name, 0, 32 * 1024).await?;

        let text = from_utf8(chunk).map_err(|err| FsError::DecodingError(err))?;

        Ok(text.to_string())
    }

    async fn write_text_file(&self, file_name: &str, text: String) -> Result<(), FsError> {
        let buf = text.as_bytes();

        self.write_binary_chunk(file_name, 0, &buf, true).await?;

        Ok(())
    }

    async fn delete_file(&self, file_name: &str) -> Result<(), FsError> {
        let fs: &Filesystem<STORAGE> = &*self.fs.lock().await;

        fs.remove(file_name).map_err(|err| FsError::OpenError(err))?;

        Ok(())
    }
}

#[derive(Debug)]
pub enum FsError {
    OpenError(littlefs_rust::Error),
    SeekError(littlefs_rust::Error),
    ReadError(littlefs_rust::Error),
    WriteError(littlefs_rust::Error),
    DecodingError(Utf8Error),
}
