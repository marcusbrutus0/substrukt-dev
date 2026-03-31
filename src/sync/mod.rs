use std::path::Path;

use flate2::Compression;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use sqlx::SqlitePool;

/// Export schemas, content, and uploads into a tar.gz bundle.
/// Includes uploads-manifest.json from SQLite instead of .meta.json sidecars.
pub async fn export_bundle(data_dir: &Path, pool: &SqlitePool, output: &Path) -> eyre::Result<()> {
    let file = std::fs::File::create(output)?;
    let enc = GzEncoder::new(file, Compression::default());
    let mut tar = tar::Builder::new(enc);

    // Write uploads-manifest.json from SQLite
    // TODO: filter by app_id in Task 10
    let upload_rows = sqlx::query_as::<_, (String, String, String, i64, String)>(
        "SELECT hash, filename, mime, size, created_at FROM uploads",
    )
    .fetch_all(pool)
    .await?;

    let manifest: Vec<serde_json::Value> = upload_rows
        .iter()
        .map(|(hash, filename, mime, size, created_at)| {
            serde_json::json!({
                "hash": hash,
                "filename": filename,
                "mime": mime,
                "size": size,
                "created_at": created_at,
            })
        })
        .collect();

    let manifest_json = serde_json::to_string_pretty(&manifest)?;
    let manifest_bytes = manifest_json.as_bytes();
    let mut header = tar::Header::new_gnu();
    header.set_path("uploads-manifest.json")?;
    header.set_size(manifest_bytes.len() as u64);
    header.set_mode(0o644);
    header.set_cksum();
    tar.append(&header, manifest_bytes)?;

    let dirs = ["schemas", "content", "uploads"];
    for dir_name in &dirs {
        let dir = data_dir.join(dir_name);
        if dir.exists() {
            tar.append_dir_all(*dir_name, &dir)?;
        }
    }

    tar.finish()?;
    Ok(())
}

/// Import a tar.gz bundle into the data directory (overwrite strategy).
pub async fn import_bundle(
    data_dir: &Path,
    pool: &SqlitePool,
    input: &Path,
) -> eyre::Result<Vec<String>> {
    let file = std::fs::File::open(input)?;
    let dec = GzDecoder::new(file);
    let mut archive = tar::Archive::new(dec);
    archive.unpack(data_dir)?;

    import_upload_metadata(data_dir, pool, 1).await?; // TODO: pass real app_id in Task 10

    let warnings = validate_imported_content(data_dir);
    Ok(warnings)
}

/// Import from bytes (for API endpoint).
pub async fn import_bundle_from_bytes(
    data_dir: &Path,
    pool: &SqlitePool,
    data: &[u8],
) -> eyre::Result<Vec<String>> {
    let dec = GzDecoder::new(data);
    let mut archive = tar::Archive::new(dec);
    archive.unpack(data_dir)?;

    import_upload_metadata(data_dir, pool, 1).await?; // TODO: pass real app_id in Task 10

    let warnings = validate_imported_content(data_dir);
    Ok(warnings)
}

/// Handle upload metadata after import — manifest or legacy sidecars.
async fn import_upload_metadata(
    app_dir: &Path,
    pool: &SqlitePool,
    app_id: i64,
) -> eyre::Result<()> {
    let manifest_path = app_dir.join("uploads-manifest.json");
    if manifest_path.exists() {
        // New format: read manifest
        let manifest_str = std::fs::read_to_string(&manifest_path)?;
        let manifest: Vec<crate::uploads::UploadMeta> = serde_json::from_str(&manifest_str)?;
        for meta in &manifest {
            crate::uploads::db_insert_upload(pool, app_id, meta).await?;
        }
        std::fs::remove_file(&manifest_path)?;
    }
    // Note: legacy sidecar migration is now handled by migrate_meta_sidecars at startup

    // Rebuild upload references from imported content
    crate::uploads::populate_references_from_content(app_dir, pool, app_id).await?;

    Ok(())
}

fn validate_imported_content(data_dir: &Path) -> Vec<String> {
    let mut warnings = Vec::new();
    let schemas_dir = data_dir.join("schemas");
    let content_dir = data_dir.join("content");

    let schemas = match crate::schema::list_schemas(&schemas_dir) {
        Ok(s) => s,
        Err(e) => {
            warnings.push(format!("Failed to list schemas: {e}"));
            return warnings;
        }
    };

    for schema in &schemas {
        let entries = match crate::content::list_entries(&content_dir, schema) {
            Ok(e) => e,
            Err(e) => {
                warnings.push(format!(
                    "Failed to list entries for {}: {e}",
                    schema.meta.slug
                ));
                continue;
            }
        };

        for entry in &entries {
            // Strip _-prefixed keys before validation — _status and _id are
            // internal metadata that may not be in the JSON Schema
            let mut data = entry.data.clone();
            if let Some(obj) = data.as_object_mut() {
                obj.retain(|k, _| !k.starts_with('_'));
            }
            if let Err(errors) = crate::content::validate_content(schema, &data) {
                for err in errors {
                    warnings.push(format!("{}/{}: {}", schema.meta.slug, entry.id, err));
                }
            }
        }
    }

    warnings
}
