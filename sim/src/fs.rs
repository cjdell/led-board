//! The simulator's file store: a plain directory (`tmp/host_board/`)
//! implementing the same `LocalFsTrait` the Pico answers from littlefs on
//! flash — so config.json and the default playlist behave the same way and
//! survive restarts (and are human-editable between runs).

use std::path::PathBuf;

use std::string::String;
use std::vec::Vec;

use embedded_tools::local_fs::{DirEntry, FileType, FsError, LocalFsTrait};

#[derive(Clone)]
pub struct HostFs {
    root: std::sync::Arc<std::path::Path>,
}

fn not_found() -> FsError {
    FsError::NotFound
}

impl HostFs {
    pub fn new(root: impl AsRef<std::path::Path>) -> Self {
        let root = root.as_ref();
        let _ = std::fs::create_dir_all(root);

        Self {
            root: std::sync::Arc::from(root),
        }
    }

    fn path(&self, name: &str) -> PathBuf {
        // Names come from the local config surface; keep traversal out.
        let safe = name.trim_start_matches('/').replace("..", "_");
        self.root.join(safe)
    }
}

impl LocalFsTrait for HostFs {
    async fn list_files(&self) -> Result<Vec<DirEntry>, FsError> {
        self.list_dir("/").await
    }

    async fn list_dir(&self, path: &str) -> Result<Vec<DirEntry>, FsError> {
        let dir = if path == "/" || path.is_empty() {
            self.root.to_path_buf()
        } else {
            self.path(path)
        };

        let mut out = Vec::new();

        for entry in std::fs::read_dir(&dir).map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => not_found(),
            _ => FsError::Io,
        })? {
            let entry = entry.map_err(|_| FsError::Io)?;
            let file_type = entry.file_type().map_err(|_| FsError::Io)?;
            let metadata = entry.metadata().map_err(|_| FsError::Io)?;

            out.push(DirEntry {
                name: entry.file_name().to_string_lossy().into_owned(),
                file_type: if file_type.is_dir() {
                    FileType::Dir
                } else {
                    FileType::File
                },
                size: metadata.len() as u32,
            });
        }

        Ok(out)
    }

    async fn get_file_size(&self, file_name: &str) -> Result<u32, FsError> {
        let meta = std::fs::metadata(self.path(file_name)).map_err(|_| not_found())?;
        Ok(meta.len() as u32)
    }

    async fn read_binary_chunk(&self, file_name: &str, pos: u32, size: u32) -> Result<Vec<u8>, FsError> {
        use std::io::{Read, Seek, SeekFrom};

        let mut file = std::fs::File::open(self.path(file_name)).map_err(|_| not_found())?;
        file.seek(SeekFrom::Start(pos as u64)).map_err(|_| not_found())?;

        let mut buf = vec![0u8; size as usize];
        let mut filled = 0;

        while filled < buf.len() {
            match file.read(&mut buf[filled..]).map_err(|_| FsError::Io)? {
                0 => break,
                n => filled += n,
            }
        }

        buf.truncate(filled);
        Ok(buf)
    }

    async fn write_binary_chunk(&self, file_name: &str, pos: u32, buf: &[u8], truncate: bool) -> Result<(), FsError> {
        use std::io::{Seek, SeekFrom, Write};

        let path = self.path(file_name);

        let mut file = if truncate {
            std::fs::File::create(&path).map_err(|_| FsError::Io)?
        } else {
            match std::fs::OpenOptions::new().write(true).create(true).open(&path) {
                Ok(f) => f,
                Err(_) => return Err(FsError::Io),
            }
        };

        file.seek(SeekFrom::Start(pos as u64)).map_err(|_| FsError::Io)?;
        file.write_all(buf).map_err(|_| FsError::Io)?;
        file.flush().map_err(|_| FsError::Io)?;

        Ok(())
    }

    async fn read_text_file(&self, file_name: &str) -> Result<String, FsError> {
        let bytes = std::fs::read(self.path(file_name)).map_err(|_| not_found())?;
        String::from_utf8(bytes).map_err(|e| FsError::Decoding(e.utf8_error()))
    }

    async fn write_text_file(&self, file_name: &str, text: String) -> Result<(), FsError> {
        std::fs::write(self.path(file_name), text).map_err(|_| FsError::Io)
    }

    async fn delete(&self, name: &str) -> Result<(), FsError> {
        let path = self.path(name);

        let result = if path.is_dir() {
            std::fs::remove_dir(&path)
        } else {
            std::fs::remove_file(&path)
        };

        match result {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(not_found()),
            Err(e) if e.kind() == std::io::ErrorKind::DirectoryNotEmpty => Err(FsError::NotEmpty),
            Err(_) => Err(FsError::Io),
        }
    }

    async fn mkdir(&self, dir_name: &str) -> Result<(), FsError> {
        match std::fs::create_dir(self.path(dir_name)) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Err(FsError::AlreadyExists),
            Err(_) => Err(FsError::Io),
        }
    }

    async fn file_exists(&self, name: &str) -> bool {
        self.path(name).exists()
    }

    async fn get_file_type(&self, name: &str) -> Result<FileType, FsError> {
        let meta = std::fs::metadata(self.path(name)).map_err(|_| not_found())?;

        if meta.is_dir() {
            Ok(FileType::Dir)
        } else {
            Ok(FileType::File)
        }
    }
}
