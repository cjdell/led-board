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

/// Type of a filesystem entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileType {
    File,
    Dir,
}

/// A single entry returned by directory listing operations.
#[derive(Debug, Clone)]
pub struct DirEntry {
    pub name: String,
    pub file_type: FileType,
    pub size: u32,
}

pub trait LocalFsTrait: Clone {
    fn list_files(&self) -> impl Future<Output = Result<Vec<DirEntry>, FsError>>;
    fn list_dir(&self, path: &str) -> impl Future<Output = Result<Vec<DirEntry>, FsError>>;
    fn get_file_size(&self, file_name: &str) -> impl Future<Output = Result<u32, FsError>>;
    fn read_binary_chunk(
        &self,
        file_name: &str,
        pos: u32,
        size: u32,
    ) -> impl Future<Output = Result<Vec<u8>, FsError>>;
    fn write_binary_chunk(
        &self,
        file_name: &str,
        pos: u32,
        buf: &[u8],
        truncate: bool,
    ) -> impl Future<Output = Result<(), FsError>>;
    fn read_text_file(&self, file_name: &str) -> impl Future<Output = Result<String, FsError>>;
    fn write_text_file(
        &self,
        file_name: &str,
        text: String,
    ) -> impl Future<Output = Result<(), FsError>>;
    fn delete(&self, name: &str) -> impl Future<Output = Result<(), FsError>>;
    fn mkdir(&self, dir_name: &str) -> impl Future<Output = Result<(), FsError>>;
    fn file_exists(&self, name: &str) -> impl Future<Output = bool>;
    fn get_file_type(&self, name: &str) -> impl Future<Output = Result<FileType, FsError>>;
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

        Filesystem::format(io, &config).map_err(FsError::from)?;

        Ok(())
    }

    pub fn new(io: STORAGE) -> Result<Self, FsError> {
        let config = Config::new(BLOCK_SIZE, FILESYSTEM_SIZE / BLOCK_SIZE);

        let fs = Filesystem::mount(io, config).map_err(|(err, _)| FsError::from(err))?;
        let fs = Arc::new(Mutex::new(fs));

        Ok(Self { fs })
    }
}

impl<STORAGE: Storage> LocalFsTrait for LocalFs<STORAGE> {
    async fn list_files(&self) -> Result<Vec<DirEntry>, FsError> {
        self.list_dir("/").await
    }

    async fn list_dir(&self, path: &str) -> Result<Vec<DirEntry>, FsError> {
        let fs: &Filesystem<STORAGE> = &*self.fs.lock().await;

        let list = fs.list_dir(path).map_err(FsError::from)?;

        Ok(list.into_iter().map(DirEntry::from).collect())
    }

    async fn get_file_size(&self, file_name: &str) -> Result<u32, FsError> {
        let fs: &Filesystem<STORAGE> = &*self.fs.lock().await;

        let metadata = fs.stat(file_name).map_err(FsError::from)?;

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
            .map_err(FsError::from)?;

        file.seek(SeekFrom::Start(pos)).map_err(FsError::from)?;

        let bytes_read = file.read(&mut buf).map_err(FsError::from)?;

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
            .map_err(FsError::from)?;

        file.seek(SeekFrom::Start(pos)).map_err(FsError::from)?;

        file.write(&buf).map_err(FsError::from)?;

        if truncate {
            file.truncate(pos + buf.len() as u32)
                .map_err(FsError::from)?;
        }

        Ok(())
    }

    async fn read_text_file(&self, file_name: &str) -> Result<String, FsError> {
        let chunk = &self.read_binary_chunk(file_name, 0, 32 * 1024).await?;

        let text = from_utf8(chunk).map_err(FsError::Decoding)?;

        Ok(text.to_string())
    }

    async fn write_text_file(&self, file_name: &str, text: String) -> Result<(), FsError> {
        let buf = text.as_bytes();

        self.write_binary_chunk(file_name, 0, &buf, true).await?;

        Ok(())
    }

    async fn delete(&self, name: &str) -> Result<(), FsError> {
        let fs: &Filesystem<STORAGE> = &*self.fs.lock().await;

        fs.remove(name).map_err(FsError::from)?;

        Ok(())
    }

    async fn mkdir(&self, dir_name: &str) -> Result<(), FsError> {
        let fs: &Filesystem<STORAGE> = &*self.fs.lock().await;

        fs.mkdir(dir_name).map_err(FsError::from)?;

        Ok(())
    }

    async fn file_exists(&self, name: &str) -> bool {
        let fs: &Filesystem<STORAGE> = &*self.fs.lock().await;

        fs.exists(name)
    }

    async fn get_file_type(&self, name: &str) -> Result<FileType, FsError> {
        let fs: &Filesystem<STORAGE> = &*self.fs.lock().await;

        let metadata = fs.stat(name).map_err(FsError::from)?;

        Ok(FileType::from(metadata.file_type))
    }
}

impl From<littlefs_rust::FileType> for FileType {
    fn from(file_type: littlefs_rust::FileType) -> Self {
        match file_type {
            littlefs_rust::FileType::File => FileType::File,
            littlefs_rust::FileType::Dir => FileType::Dir,
        }
    }
}

impl From<littlefs_rust::DirEntry> for DirEntry {
    fn from(entry: littlefs_rust::DirEntry) -> Self {
        DirEntry {
            name: entry.name,
            file_type: FileType::from(entry.file_type),
            size: entry.size,
        }
    }
}

impl From<littlefs_rust::Error> for FsError {
    fn from(err: littlefs_rust::Error) -> Self {
        match err {
            littlefs_rust::Error::Io => FsError::Io,
            littlefs_rust::Error::Corrupt => FsError::Corrupt,
            littlefs_rust::Error::NoEntry => FsError::NotFound,
            littlefs_rust::Error::Exists => FsError::AlreadyExists,
            littlefs_rust::Error::NotDir => FsError::NotDir,
            littlefs_rust::Error::IsDir => FsError::IsDir,
            littlefs_rust::Error::NotEmpty => FsError::NotEmpty,
            littlefs_rust::Error::Invalid => FsError::Invalid,
            littlefs_rust::Error::NoSpace => FsError::NoSpace,
            littlefs_rust::Error::NoMemory => FsError::NoMemory,
            littlefs_rust::Error::NoAttribute => FsError::NoAttribute,
            littlefs_rust::Error::NameTooLong => FsError::NameTooLong,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FsError {
    NotFound,
    AlreadyExists,
    IsDir,
    NotDir,
    NotEmpty,
    Io,
    Corrupt,
    Invalid,
    NoSpace,
    NoMemory,
    NameTooLong,
    NoAttribute,
    Decoding(Utf8Error),
}
