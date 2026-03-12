use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct Config {
    pub data_dir: PathBuf,
    pub db_path: PathBuf,
    pub listen_addr: String,
    pub listen_port: u16,
    pub secure_cookies: bool,
    pub staging_webhook_url: Option<String>,
    pub production_webhook_url: Option<String>,
    pub webhook_check_interval: u64,
}

impl Config {
    pub fn new(
        data_dir: Option<PathBuf>,
        db_path: Option<PathBuf>,
        port: Option<u16>,
        secure_cookies: bool,
        staging_webhook_url: Option<String>,
        production_webhook_url: Option<String>,
        webhook_check_interval: Option<u64>,
    ) -> Self {
        let data_dir = data_dir.unwrap_or_else(|| PathBuf::from("data"));
        let db_path = db_path.unwrap_or_else(|| data_dir.join("substrukt.db"));
        Self {
            data_dir,
            db_path,
            listen_addr: "0.0.0.0".into(),
            listen_port: port.unwrap_or(3000),
            secure_cookies,
            staging_webhook_url,
            production_webhook_url,
            webhook_check_interval: webhook_check_interval.unwrap_or(300),
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
