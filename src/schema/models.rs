use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubstruktMeta {
    pub title: String,
    pub slug: String,
    #[serde(default = "default_storage")]
    pub storage: StorageMode,
    /// Which field to use as entry ID (for directory mode). Defaults to first string field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id_field: Option<String>,
}

fn default_storage() -> StorageMode {
    StorageMode::Directory
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum StorageMode {
    Directory,
    SingleFile,
}

impl std::fmt::Display for StorageMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StorageMode::Directory => write!(f, "directory"),
            StorageMode::SingleFile => write!(f, "single-file"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct SchemaFile {
    pub meta: SubstruktMeta,
    pub schema: serde_json::Value,
}
