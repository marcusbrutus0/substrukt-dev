use std::sync::Arc;

use dashmap::DashMap;
use minijinja::Environment;
use sqlx::SqlitePool;
use tokio::sync::RwLock;

use crate::config::Config;

pub type ContentCache = DashMap<String, serde_json::Value>;

pub struct AppStateInner {
    pub pool: SqlitePool,
    pub config: Config,
    pub templates: RwLock<Environment<'static>>,
    pub cache: ContentCache,
}

pub type AppState = Arc<AppStateInner>;
