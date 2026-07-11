use crate::local_fs::LocalFsTrait;
use alloc::string::String;

pub trait ConfigFileStorage: Clone {
    fn read_json(&self) -> impl Future<Output = Result<String, ()>>;
    fn write_json(&self, json: String) -> impl Future<Output = Result<(), ()>>;
}

#[derive(Clone)]
pub struct LocalFsConfigFileStorage<FS> {
    local_fs: FS,
    file_name: String,
}

impl<FS: LocalFsTrait> LocalFsConfigFileStorage<FS> {
    pub fn new(local_fs: FS, file_name: String) -> Self {
        Self { local_fs, file_name }
    }
}

impl<FS: LocalFsTrait> ConfigFileStorage for LocalFsConfigFileStorage<FS> {
    async fn read_json(&self) -> Result<String, ()> {
        self.local_fs.read_text_file(&self.file_name).await.map_err(|_| ())
    }

    async fn write_json(&self, json: String) -> Result<(), ()> {
        self.local_fs
            .write_text_file(&self.file_name, json)
            .await
            .map_err(|_| ())
    }
}
