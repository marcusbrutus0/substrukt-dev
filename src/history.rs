use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;

#[derive(Debug, Clone, serde::Serialize)]
pub struct VersionInfo {
    pub timestamp: u64,
    pub size: u64,
}

fn history_dir(data_dir: &Path, schema_slug: &str, entry_id: &str) -> std::path::PathBuf {
    data_dir.join("_history").join(schema_slug).join(entry_id)
}

/// Snapshot the current entry data before overwriting.
pub fn snapshot_entry(
    data_dir: &Path,
    schema_slug: &str,
    entry_id: &str,
    current_data: &Value,
    max_versions: usize,
) -> eyre::Result<()> {
    if max_versions == 0 {
        return Ok(());
    }

    let dir = history_dir(data_dir, schema_slug, entry_id);
    std::fs::create_dir_all(&dir)?;

    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let path = dir.join(format!("{timestamp}.json"));
    let content = serde_json::to_string_pretty(current_data)?;
    std::fs::write(path, content)?;

    prune_versions(&dir, max_versions)?;

    Ok(())
}

/// List available versions for an entry, newest first.
pub fn list_versions(
    data_dir: &Path,
    schema_slug: &str,
    entry_id: &str,
) -> eyre::Result<Vec<VersionInfo>> {
    let dir = history_dir(data_dir, schema_slug, entry_id);
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut versions = Vec::new();
    for entry in std::fs::read_dir(&dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "json") {
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                if let Ok(ts) = stem.parse::<u64>() {
                    let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
                    versions.push(VersionInfo {
                        timestamp: ts,
                        size,
                    });
                }
            }
        }
    }

    versions.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    Ok(versions)
}

/// Load a specific version by timestamp.
pub fn get_version(
    data_dir: &Path,
    schema_slug: &str,
    entry_id: &str,
    timestamp: u64,
) -> eyre::Result<Option<Value>> {
    let path = history_dir(data_dir, schema_slug, entry_id).join(format!("{timestamp}.json"));
    if !path.exists() {
        return Ok(None);
    }
    let content = std::fs::read_to_string(&path)?;
    let data: Value = serde_json::from_str(&content)?;
    Ok(Some(data))
}

fn prune_versions(dir: &Path, max_versions: usize) -> eyre::Result<()> {
    let mut files: Vec<(u64, std::path::PathBuf)> = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "json") {
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                if let Ok(ts) = stem.parse::<u64>() {
                    files.push((ts, path));
                }
            }
        }
    }

    if files.len() <= max_versions {
        return Ok(());
    }

    files.sort_by_key(|(ts, _)| *ts);
    let to_remove = files.len() - max_versions;
    for (_, path) in files.into_iter().take(to_remove) {
        std::fs::remove_file(path)?;
    }

    Ok(())
}
