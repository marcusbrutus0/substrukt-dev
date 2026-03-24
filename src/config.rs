use std::path::PathBuf;

use serde::Deserialize;

#[derive(Debug, Clone)]
pub struct Config {
    pub data_dir: PathBuf,
    pub db_path: PathBuf,
    pub listen_addr: String,
    pub listen_port: u16,
    pub secure_cookies: bool,
    pub staging_webhook_url: Option<String>,
    pub staging_webhook_auth_token: Option<String>,
    pub production_webhook_url: Option<String>,
    pub production_webhook_auth_token: Option<String>,
    pub webhook_check_interval: u64,
    pub serve_llms_txt: bool,
}

#[derive(Debug, Deserialize, Default)]
struct TomlConfig {
    #[serde(default)]
    features: FeaturesConfig,
}

#[derive(Debug, Deserialize, Default)]
struct FeaturesConfig {
    #[serde(default)]
    serve_llms_txt: bool,
}

fn load_toml_config() -> TomlConfig {
    let path = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("substrukt.toml")))
        .unwrap_or_else(|| std::path::PathBuf::from("substrukt.toml"));
    let Ok(contents) = std::fs::read_to_string(&path) else {
        return TomlConfig::default();
    };
    toml::from_str(&contents).unwrap_or_default()
}

impl Config {
    pub fn new(
        data_dir: Option<PathBuf>,
        db_path: Option<PathBuf>,
        port: Option<u16>,
        secure_cookies: bool,
        staging_webhook_url: Option<String>,
        staging_webhook_auth_token: Option<String>,
        production_webhook_url: Option<String>,
        production_webhook_auth_token: Option<String>,
        webhook_check_interval: Option<u64>,
    ) -> Self {
        let toml = load_toml_config();
        let data_dir = data_dir.unwrap_or_else(|| PathBuf::from("data"));
        let db_path = db_path.unwrap_or_else(|| data_dir.join("substrukt.db"));
        Self {
            data_dir,
            db_path,
            listen_addr: "0.0.0.0".into(),
            listen_port: port.unwrap_or(3000),
            secure_cookies,
            staging_webhook_url,
            staging_webhook_auth_token,
            production_webhook_url,
            production_webhook_auth_token,
            webhook_check_interval: webhook_check_interval.unwrap_or(300),
            serve_llms_txt: toml.features.serve_llms_txt,
        }
    }

    pub fn with_serve_llms_txt(mut self, value: bool) -> Self {
        self.serve_llms_txt = value;
        self
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
