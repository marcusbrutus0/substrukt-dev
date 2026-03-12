use std::path::Path;

use flate2::Compression;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;

/// Export schemas, content, and uploads into a tar.gz bundle.
pub fn export_bundle(data_dir: &Path, output: &Path) -> eyre::Result<()> {
    let file = std::fs::File::create(output)?;
    let enc = GzEncoder::new(file, Compression::default());
    let mut tar = tar::Builder::new(enc);

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
pub fn import_bundle(data_dir: &Path, input: &Path) -> eyre::Result<Vec<String>> {
    let file = std::fs::File::open(input)?;
    let dec = GzDecoder::new(file);
    let mut archive = tar::Archive::new(dec);

    archive.unpack(data_dir)?;

    // Validate content against schemas
    let warnings = validate_imported_content(data_dir);
    Ok(warnings)
}

/// Import from bytes (for API endpoint).
pub fn import_bundle_from_bytes(data_dir: &Path, data: &[u8]) -> eyre::Result<Vec<String>> {
    let dec = GzDecoder::new(data);
    let mut archive = tar::Archive::new(dec);

    archive.unpack(data_dir)?;

    let warnings = validate_imported_content(data_dir);
    Ok(warnings)
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
            if let Err(errors) = crate::content::validate_content(schema, &entry.data) {
                for err in errors {
                    warnings.push(format!("{}/{}: {}", schema.meta.slug, entry.id, err));
                }
            }
        }
    }

    warnings
}
