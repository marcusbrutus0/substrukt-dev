use std::path::Path;

use crate::content;
use crate::schema;
use crate::state::ContentCache;

/// Populate the cache from disk on startup.
pub fn populate(cache: &ContentCache, schemas_dir: &Path, content_dir: &Path) {
    let schemas = match schema::list_schemas(schemas_dir) {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("Failed to list schemas for cache: {e}");
            return;
        }
    };

    for s in &schemas {
        let entries = match content::list_entries(content_dir, s) {
            Ok(e) => e,
            Err(e) => {
                tracing::warn!("Failed to list entries for {}: {e}", s.meta.slug);
                continue;
            }
        };
        for entry in entries {
            let key = format!("{}/{}", s.meta.slug, entry.id);
            cache.insert(key, entry.data);
        }
    }

    tracing::info!("Cache populated with {} entries", cache.len());
}

/// Reload all entries for a specific schema.
pub fn reload_schema(
    cache: &ContentCache,
    content_dir: &Path,
    schema: &schema::models::SchemaFile,
) {
    let prefix = format!("{}/", schema.meta.slug);
    // Remove old entries for this schema
    cache.retain(|k, _| !k.starts_with(&prefix));

    // Reload
    match content::list_entries(content_dir, schema) {
        Ok(entries) => {
            for entry in entries {
                let key = format!("{}/{}", schema.meta.slug, entry.id);
                cache.insert(key, entry.data);
            }
        }
        Err(e) => {
            tracing::warn!("Failed to reload cache for {}: {e}", schema.meta.slug);
        }
    }
}

/// Reload a single entry.
pub fn reload_entry(
    cache: &ContentCache,
    content_dir: &Path,
    schema: &schema::models::SchemaFile,
    entry_id: &str,
) {
    let key = format!("{}/{}", schema.meta.slug, entry_id);
    match content::get_entry(content_dir, schema, entry_id) {
        Ok(Some(entry)) => {
            cache.insert(key, entry.data);
        }
        Ok(None) => {
            cache.remove(&key);
        }
        Err(e) => {
            tracing::warn!("Failed to reload cache entry {key}: {e}");
        }
    }
}

/// Clear and rebuild the entire cache.
pub fn rebuild(cache: &ContentCache, schemas_dir: &Path, content_dir: &Path) {
    cache.clear();
    populate(cache, schemas_dir, content_dir);
}
