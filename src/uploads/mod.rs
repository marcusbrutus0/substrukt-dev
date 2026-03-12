use std::path::Path;

use sha2::{Digest, Sha256};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UploadMeta {
    pub hash: String,
    pub filename: String,
    pub mime: String,
    pub size: u64,
}

pub fn store_upload(
    uploads_dir: &Path,
    filename: &str,
    mime: &str,
    data: &[u8],
) -> eyre::Result<UploadMeta> {
    let mut hasher = Sha256::new();
    hasher.update(data);
    let hash = hex::encode(hasher.finalize());

    let prefix = &hash[..2];
    let rest = &hash[2..];
    let dir = uploads_dir.join(prefix);
    std::fs::create_dir_all(&dir)?;

    let file_path = dir.join(rest);
    if !file_path.exists() {
        std::fs::write(&file_path, data)?;
    }

    // Write sidecar metadata
    let meta = UploadMeta {
        hash: hash.clone(),
        filename: filename.to_string(),
        mime: mime.to_string(),
        size: data.len() as u64,
    };
    let meta_path = dir.join(format!("{rest}.meta.json"));
    if !meta_path.exists() {
        let meta_json = serde_json::to_string_pretty(&meta)?;
        std::fs::write(meta_path, meta_json)?;
    }

    Ok(meta)
}

pub fn get_upload_path(uploads_dir: &Path, hash: &str) -> Option<std::path::PathBuf> {
    if hash.len() < 3 {
        return None;
    }
    let prefix = &hash[..2];
    let rest = &hash[2..];
    let path = uploads_dir.join(prefix).join(rest);
    if path.exists() { Some(path) } else { None }
}

pub fn get_upload_meta(uploads_dir: &Path, hash: &str) -> Option<UploadMeta> {
    if hash.len() < 3 {
        return None;
    }
    let prefix = &hash[..2];
    let rest = &hash[2..];
    let meta_path = uploads_dir.join(prefix).join(format!("{rest}.meta.json"));
    if !meta_path.exists() {
        return None;
    }
    let content = std::fs::read_to_string(meta_path).ok()?;
    serde_json::from_str(&content).ok()
}
