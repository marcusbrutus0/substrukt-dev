use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct Config {
    pub data_dir: PathBuf,
    pub db_path: PathBuf,
    pub listen_addr: String,
    pub listen_port: u16,
    pub secure_cookies: bool,
    pub version_history_count: usize,
    pub max_body_size: usize,
}

impl Config {
    pub fn new(
        data_dir: Option<PathBuf>,
        db_path: Option<PathBuf>,
        port: Option<u16>,
        secure_cookies: bool,
        version_history_count: usize,
        max_body_size_mb: usize,
    ) -> Self {
        let data_dir = data_dir.unwrap_or_else(|| PathBuf::from("data"));
        let db_path = db_path.unwrap_or_else(|| data_dir.join("substrukt.db"));
        Self {
            data_dir,
            db_path,
            listen_addr: "0.0.0.0".into(),
            listen_port: port.unwrap_or(3000),
            secure_cookies,
            version_history_count,
            max_body_size: max_body_size_mb * 1024 * 1024,
        }
    }

    pub fn schemas_dir(&self) -> PathBuf {
        self.data_dir.join("schemas")
    }

    pub fn content_dir(&self) -> PathBuf {
        self.data_dir.join("content")
    }

    pub fn uploads_dir(&self) -> PathBuf {
        self.data_dir.join("uploads")
    }

    pub fn ensure_dirs(&self) -> eyre::Result<()> {
        std::fs::create_dir_all(self.schemas_dir())?;
        std::fs::create_dir_all(self.content_dir())?;
        std::fs::create_dir_all(self.uploads_dir())?;
        Ok(())
    }
}
