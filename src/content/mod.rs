pub mod form;

use std::path::Path;

use serde_json::Value;
use uuid::Uuid;

use crate::schema::models::{Kind, SchemaFile, StorageMode};

/// A single content entry
#[derive(Debug, Clone, serde::Serialize)]
pub struct ContentEntry {
    pub id: String,
    pub data: Value,
}

pub fn list_entries(content_dir: &Path, schema: &SchemaFile) -> eyre::Result<Vec<ContentEntry>> {
    let slug = &schema.meta.slug;
    match schema.meta.storage {
        StorageMode::Directory => list_directory_entries(content_dir, slug),
        StorageMode::SingleFile => list_single_file_entries(content_dir, slug, &schema.meta.kind),
    }
}

fn list_directory_entries(content_dir: &Path, slug: &str) -> eyre::Result<Vec<ContentEntry>> {
    let dir = content_dir.join(slug);
    let mut entries = Vec::new();
    if !dir.exists() {
        return Ok(entries);
    }
    for entry in std::fs::read_dir(&dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "json") {
            let id = path
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            let content = std::fs::read_to_string(&path)?;
            let data: Value = serde_json::from_str(&content)?;
            entries.push(ContentEntry { id, data });
        }
    }
    entries.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(entries)
}

fn list_single_file_entries(
    content_dir: &Path,
    slug: &str,
    kind: &Kind,
) -> eyre::Result<Vec<ContentEntry>> {
    let path = content_dir.join(format!("{slug}.json"));
    if !path.exists() {
        return Ok(Vec::new());
    }
    let content = std::fs::read_to_string(&path)?;
    let items: Vec<Value> = match kind {
        Kind::Single => {
            let obj: Value = serde_json::from_str(&content)?;
            vec![obj]
        }
        Kind::Collection => serde_json::from_str(&content)?,
    };
    Ok(items
        .into_iter()
        .enumerate()
        .map(|(i, data)| {
            let id = data
                .get("_id")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| i.to_string());
            ContentEntry { id, data }
        })
        .collect())
}

pub fn get_entry(
    content_dir: &Path,
    schema: &SchemaFile,
    entry_id: &str,
) -> eyre::Result<Option<ContentEntry>> {
    let slug = &schema.meta.slug;
    match schema.meta.storage {
        StorageMode::Directory => {
            let path = content_dir.join(slug).join(format!("{entry_id}.json"));
            if !path.exists() {
                return Ok(None);
            }
            let content = std::fs::read_to_string(&path)?;
            let data: Value = serde_json::from_str(&content)?;
            Ok(Some(ContentEntry {
                id: entry_id.to_string(),
                data,
            }))
        }
        StorageMode::SingleFile => {
            let entries = list_single_file_entries(content_dir, slug, &schema.meta.kind)?;
            Ok(entries.into_iter().find(|e| e.id == entry_id))
        }
    }
}

pub fn save_entry(
    content_dir: &Path,
    schema: &SchemaFile,
    entry_id: Option<&str>,
    mut data: Value,
) -> eyre::Result<String> {
    let slug = &schema.meta.slug;

    // Determine _status: draft for new entries, preserve existing for updates
    let status = if let Some(eid) = entry_id {
        // Update path: try to read existing _status
        get_entry(content_dir, schema, eid)
            .ok()
            .flatten()
            .and_then(|e| {
                e.data
                    .get("_status")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
            })
            .unwrap_or_else(|| "draft".to_string())
    } else {
        "draft".to_string()
    };

    // Inject _status into data
    if let Some(obj) = data.as_object_mut() {
        obj.insert("_status".to_string(), Value::String(status));
    }

    match schema.meta.storage {
        StorageMode::Directory => {
            let dir = content_dir.join(slug);
            std::fs::create_dir_all(&dir)?;
            let id = entry_id
                .map(|s| s.to_string())
                .unwrap_or_else(|| generate_entry_id(schema, &data));
            let path = dir.join(format!("{id}.json"));
            let content = serde_json::to_string_pretty(&data)?;
            std::fs::write(path, content)?;
            Ok(id)
        }
        StorageMode::SingleFile => {
            let path = content_dir.join(format!("{slug}.json"));

            let id = entry_id
                .map(|s| s.to_string())
                .unwrap_or_else(|| Uuid::new_v4().to_string());

            // Insert _id into data
            let mut data = data;
            if let Some(obj) = data.as_object_mut() {
                obj.insert("_id".to_string(), Value::String(id.clone()));
            }

            if schema.meta.kind == Kind::Single {
                let content = serde_json::to_string_pretty(&data)?;
                std::fs::write(path, content)?;
            } else {
                let mut entries = if path.exists() {
                    let content = std::fs::read_to_string(&path)?;
                    serde_json::from_str::<Vec<Value>>(&content)?
                } else {
                    Vec::new()
                };

                if let Some(existing_id) = entry_id {
                    if let Some(pos) = entries.iter().position(|e| {
                        e.get("_id")
                            .and_then(|v| v.as_str())
                            .is_some_and(|s| s == existing_id)
                    }) {
                        entries[pos] = data;
                    } else {
                        entries.push(data);
                    }
                } else {
                    entries.push(data);
                }

                let content = serde_json::to_string_pretty(&entries)?;
                std::fs::write(path, content)?;
            }
            Ok(id)
        }
    }
}

pub fn delete_entry(content_dir: &Path, schema: &SchemaFile, entry_id: &str) -> eyre::Result<()> {
    let slug = &schema.meta.slug;
    match schema.meta.storage {
        StorageMode::Directory => {
            let path = content_dir.join(slug).join(format!("{entry_id}.json"));
            if path.exists() {
                std::fs::remove_file(path)?;
            }
        }
        StorageMode::SingleFile => {
            let path = content_dir.join(format!("{slug}.json"));
            if path.exists() {
                if schema.meta.kind == Kind::Single {
                    std::fs::remove_file(&path)?;
                } else {
                    let content = std::fs::read_to_string(&path)?;
                    let mut entries: Vec<Value> = serde_json::from_str(&content)?;
                    entries.retain(|e| {
                        e.get("_id")
                            .and_then(|v| v.as_str())
                            .is_none_or(|s| s != entry_id)
                    });
                    let content = serde_json::to_string_pretty(&entries)?;
                    std::fs::write(path, content)?;
                }
            }
        }
    }
    Ok(())
}

/// Set the _status of an entry without modifying its content.
/// Does not create a history snapshot (metadata-only change).
pub fn set_entry_status(
    content_dir: &Path,
    schema: &SchemaFile,
    entry_id: &str,
    status: &str,
) -> eyre::Result<()> {
    if !matches!(status, "draft" | "published") {
        eyre::bail!("Invalid status: {status}. Must be \"draft\" or \"published\".");
    }

    let slug = &schema.meta.slug;
    match schema.meta.storage {
        StorageMode::Directory => {
            let path = content_dir.join(slug).join(format!("{entry_id}.json"));
            if !path.exists() {
                eyre::bail!("Entry not found: {slug}/{entry_id}");
            }
            let content = std::fs::read_to_string(&path)?;
            let mut data: Value = serde_json::from_str(&content)?;
            if let Some(obj) = data.as_object_mut() {
                obj.insert("_status".to_string(), Value::String(status.to_string()));
            }
            std::fs::write(&path, serde_json::to_string_pretty(&data)?)?;
        }
        StorageMode::SingleFile => {
            let path = content_dir.join(format!("{slug}.json"));
            if !path.exists() {
                eyre::bail!("Entry not found: {slug}/{entry_id}");
            }
            if schema.meta.kind == Kind::Single {
                let content = std::fs::read_to_string(&path)?;
                let mut data: Value = serde_json::from_str(&content)?;
                if let Some(obj) = data.as_object_mut() {
                    obj.insert("_status".to_string(), Value::String(status.to_string()));
                }
                std::fs::write(&path, serde_json::to_string_pretty(&data)?)?;
            } else {
                // Collection in single file
                let content = std::fs::read_to_string(&path)?;
                let mut entries: Vec<Value> = serde_json::from_str(&content)?;
                let found = entries.iter_mut().any(|e| {
                    let matches = e
                        .get("_id")
                        .and_then(|v| v.as_str())
                        .is_some_and(|s| s == entry_id);
                    if matches {
                        if let Some(obj) = e.as_object_mut() {
                            obj.insert(
                                "_status".to_string(),
                                Value::String(status.to_string()),
                            );
                        }
                    }
                    matches
                });
                if !found {
                    eyre::bail!("Entry not found: {slug}/{entry_id}");
                }
                std::fs::write(&path, serde_json::to_string_pretty(&entries)?)?;
            }
        }
    }
    Ok(())
}

/// Check if any string value in the JSON data contains the query (case-insensitive).
/// The query must already be lowercased by the caller.
pub fn matches_query(data: &Value, query_lower: &str) -> bool {
    match data {
        Value::String(s) => s.to_lowercase().contains(query_lower),
        Value::Object(map) => map
            .iter()
            .filter(|(k, _)| !k.starts_with('_'))
            .any(|(_, v)| matches_query(v, query_lower)),
        Value::Array(arr) => arr.iter().any(|v| matches_query(v, query_lower)),
        _ => false,
    }
}

/// Filter entries by a search query. Case-insensitive substring match on all string values.
pub fn filter_entries(entries: Vec<ContentEntry>, query: &str) -> Vec<ContentEntry> {
    let query_lower = query.to_lowercase();
    entries
        .into_iter()
        .filter(|e| matches_query(&e.data, &query_lower))
        .collect()
}

/// Get the status of an entry. Returns "published" if no _status field (backwards compat).
pub fn get_entry_status(data: &Value) -> &str {
    data.get("_status")
        .and_then(|v| v.as_str())
        .unwrap_or("published")
}

/// Filter entries by status. "all" returns everything.
/// "published" returns entries with _status=published or missing _status (backwards compat).
/// "draft" returns only entries with _status=draft.
pub fn filter_by_status(entries: Vec<ContentEntry>, status: &str) -> Vec<ContentEntry> {
    match status {
        "all" => entries,
        "draft" => entries
            .into_iter()
            .filter(|e| get_entry_status(&e.data) == "draft")
            .collect(),
        _ => entries
            .into_iter()
            .filter(|e| get_entry_status(&e.data) == "published")
            .collect(),
    }
}

/// Flip all draft entries to published across all schemas. Returns count of entries published.
/// Bypasses save_entry to avoid validation/snapshot overhead (metadata-only change).
pub fn publish_all_drafts(schemas_dir: &Path, content_dir: &Path) -> eyre::Result<usize> {
    let schemas = crate::schema::list_schemas(schemas_dir)?;
    let mut count = 0;

    for schema in &schemas {
        let entries = list_entries(content_dir, schema)?;
        let draft_entries: Vec<&ContentEntry> = entries
            .iter()
            .filter(|e| get_entry_status(&e.data) == "draft")
            .collect();

        if draft_entries.is_empty() {
            continue;
        }

        match schema.meta.storage {
            StorageMode::Directory => {
                let dir = content_dir.join(&schema.meta.slug);
                for entry in &draft_entries {
                    let mut data = entry.data.clone();
                    if let Some(obj) = data.as_object_mut() {
                        obj.insert("_status".to_string(), Value::String("published".to_string()));
                    }
                    let path = dir.join(format!("{}.json", entry.id));
                    std::fs::write(&path, serde_json::to_string_pretty(&data)?)?;
                    count += 1;
                }
            }
            StorageMode::SingleFile => {
                let path = content_dir.join(format!("{}.json", schema.meta.slug));
                if schema.meta.kind == Kind::Single {
                    // Single entry
                    if let Some(entry) = draft_entries.first() {
                        let mut data = entry.data.clone();
                        if let Some(obj) = data.as_object_mut() {
                            obj.insert(
                                "_status".to_string(),
                                Value::String("published".to_string()),
                            );
                        }
                        std::fs::write(&path, serde_json::to_string_pretty(&data)?)?;
                        count += 1;
                    }
                } else {
                    // Collection in single file — rewrite entire file
                    let content = std::fs::read_to_string(&path)?;
                    let mut all: Vec<Value> = serde_json::from_str(&content)?;
                    for item in &mut all {
                        if get_entry_status(item) == "draft" {
                            if let Some(obj) = item.as_object_mut() {
                                obj.insert(
                                    "_status".to_string(),
                                    Value::String("published".to_string()),
                                );
                            }
                            count += 1;
                        }
                    }
                    std::fs::write(&path, serde_json::to_string_pretty(&all)?)?;
                }
            }
        }
    }

    Ok(count)
}

/// Strip `_status` from entry data for API responses.
pub fn strip_internal_status(data: &Value) -> Value {
    let mut data = data.clone();
    if let Some(obj) = data.as_object_mut() {
        obj.remove("_status");
    }
    data
}

pub fn validate_content(schema: &SchemaFile, data: &Value) -> Result<(), Vec<String>> {
    // Patch schema to accept objects for upload fields, since uploads are stored
    // as {hash, filename, mime} objects rather than plain strings.
    let patched = patch_upload_types(&schema.schema);
    match jsonschema::validator_for(&patched) {
        Ok(validator) => {
            let errors: Vec<String> = validator
                .iter_errors(data)
                .map(|e| format!("{}: {}", e.instance_path, e))
                .collect();
            if errors.is_empty() {
                Ok(())
            } else {
                Err(errors)
            }
        }
        Err(e) => Err(vec![format!("Invalid schema: {e}")]),
    }
}

/// Rewrite `{"type": "string", "format": "upload"}` properties to accept
/// either a string or an object so that stored upload references pass validation.
fn patch_upload_types(schema: &Value) -> Value {
    let mut schema = schema.clone();
    if let Some(props) = schema.get_mut("properties").and_then(|p| p.as_object_mut()) {
        for (_key, prop) in props.iter_mut() {
            let is_upload = prop.get("type").and_then(|t| t.as_str()) == Some("string")
                && prop.get("format").and_then(|f| f.as_str()) == Some("upload");
            if is_upload {
                // Allow string or object
                if let Some(obj) = prop.as_object_mut() {
                    obj.remove("type");
                    obj.insert("type".to_string(), serde_json::json!(["string", "object"]));
                }
            }
        }
    }
    schema
}

fn generate_entry_id(schema: &SchemaFile, data: &Value) -> String {
    // Try to use the id_field from meta, or find first string field
    let id_field = schema.meta.id_field.clone().or_else(|| {
        schema
            .schema
            .get("properties")
            .and_then(|p| p.as_object())
            .and_then(|props| {
                props.iter().find_map(|(key, val)| {
                    if val.get("type").and_then(|t| t.as_str()) == Some("string")
                        && !matches!(
                            val.get("format").and_then(|f| f.as_str()),
                            Some("upload") | Some("reference")
                        )
                    {
                        Some(key.clone())
                    } else {
                        None
                    }
                })
            })
    });

    if let Some(field) = id_field
        && let Some(val) = data.get(&field).and_then(|v| v.as_str())
    {
        let slugified = slug::slugify(val);
        if !slugified.is_empty() {
            return slugified;
        }
    }

    Uuid::new_v4().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::TempDir;

    fn test_schema(kind: Kind, storage: StorageMode) -> SchemaFile {
        SchemaFile {
            meta: crate::schema::models::SubstruktMeta {
                title: "Test".to_string(),
                slug: "test".to_string(),
                kind,
                storage,
                id_field: None,
            },
            schema: json!({
                "type": "object",
                "properties": {
                    "title": { "type": "string" }
                }
            }),
        }
    }

    #[test]
    fn save_entry_create_injects_draft_status() {
        let tmp = TempDir::new().unwrap();
        let schema = test_schema(Kind::Collection, StorageMode::Directory);
        let data = json!({"title": "Hello"});
        let id = save_entry(tmp.path(), &schema, None, data).unwrap();

        let entry = get_entry(tmp.path(), &schema, &id).unwrap().unwrap();
        assert_eq!(
            entry.data.get("_status").and_then(|v| v.as_str()),
            Some("draft"),
            "new entry should have _status: draft"
        );
    }

    #[test]
    fn save_entry_update_preserves_status() {
        let tmp = TempDir::new().unwrap();
        let schema = test_schema(Kind::Collection, StorageMode::Directory);

        // Create entry (gets _status: draft)
        let data = json!({"title": "Hello"});
        let id = save_entry(tmp.path(), &schema, None, data).unwrap();

        // Manually set to published
        let mut entry = get_entry(tmp.path(), &schema, &id).unwrap().unwrap();
        entry
            .data
            .as_object_mut()
            .unwrap()
            .insert("_status".to_string(), json!("published"));
        let path = tmp.path().join("test").join(format!("{id}.json"));
        std::fs::write(&path, serde_json::to_string_pretty(&entry.data).unwrap()).unwrap();

        // Update via save_entry
        let new_data = json!({"title": "Updated"});
        save_entry(tmp.path(), &schema, Some(&id), new_data).unwrap();

        let updated = get_entry(tmp.path(), &schema, &id).unwrap().unwrap();
        assert_eq!(
            updated.data.get("_status").and_then(|v| v.as_str()),
            Some("published"),
            "updated entry should preserve _status: published"
        );
        assert_eq!(
            updated.data.get("title").and_then(|v| v.as_str()),
            Some("Updated")
        );
    }

    #[test]
    fn save_entry_update_no_existing_falls_back_to_draft() {
        let tmp = TempDir::new().unwrap();
        let schema = test_schema(Kind::Single, StorageMode::SingleFile);

        // First upsert — no existing entry
        let data = json!({"title": "Settings"});
        save_entry(tmp.path(), &schema, Some("_single"), data).unwrap();

        let entry = get_entry(tmp.path(), &schema, "_single").unwrap().unwrap();
        assert_eq!(
            entry.data.get("_status").and_then(|v| v.as_str()),
            Some("draft"),
            "first upsert with no existing should default to draft"
        );
    }

    #[test]
    fn strip_internal_status_removes_status_only() {
        let data = json!({"_id": "test", "_status": "draft", "title": "Hello"});
        let stripped = strip_internal_status(&data);
        assert!(
            stripped.get("_status").is_none(),
            "_status should be stripped"
        );
        assert!(stripped.get("_id").is_some(), "_id should remain");
        assert!(stripped.get("title").is_some(), "title should remain");
    }

    #[test]
    fn matches_query_skips_underscore_prefixed_keys() {
        let data = json!({"_status": "draft", "_id": "my-id", "title": "Hello World"});
        assert!(!matches_query(&data, "draft"), "should not match _status");
        assert!(!matches_query(&data, "my-id"), "should not match _id");
        assert!(matches_query(&data, "hello"), "should match title");
    }

    #[test]
    fn missing_status_treated_as_published() {
        // Entry data without _status (legacy)
        let data = json!({"title": "Old entry"});
        let status = data
            .get("_status")
            .and_then(|v| v.as_str())
            .unwrap_or("published");
        assert_eq!(status, "published");
    }

    #[test]
    fn filter_by_status_published_only() {
        let entries = vec![
            ContentEntry { id: "a".into(), data: json!({"_status": "draft", "title": "Draft"}) },
            ContentEntry { id: "b".into(), data: json!({"_status": "published", "title": "Published"}) },
            ContentEntry { id: "c".into(), data: json!({"title": "Legacy"}) },
        ];
        let filtered = filter_by_status(entries, "published");
        assert_eq!(filtered.len(), 2, "should return published + legacy (no _status = published)");
        assert!(filtered.iter().any(|e| e.id == "b"));
        assert!(filtered.iter().any(|e| e.id == "c"));
    }

    #[test]
    fn filter_by_status_draft_only() {
        let entries = vec![
            ContentEntry { id: "a".into(), data: json!({"_status": "draft", "title": "Draft"}) },
            ContentEntry { id: "b".into(), data: json!({"_status": "published", "title": "Published"}) },
        ];
        let filtered = filter_by_status(entries, "draft");
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].id, "a");
    }

    #[test]
    fn filter_by_status_all_returns_everything() {
        let entries = vec![
            ContentEntry { id: "a".into(), data: json!({"_status": "draft"}) },
            ContentEntry { id: "b".into(), data: json!({"_status": "published"}) },
        ];
        let filtered = filter_by_status(entries, "all");
        assert_eq!(filtered.len(), 2);
    }

    #[test]
    fn publish_all_drafts_flips_status() {
        let tmp = TempDir::new().unwrap();
        let schema = test_schema(Kind::Collection, StorageMode::Directory);

        // Create two entries (both draft)
        save_entry(tmp.path(), &schema, None, json!({"title": "A"})).unwrap();
        save_entry(tmp.path(), &schema, None, json!({"title": "B"})).unwrap();

        let schemas_dir = tmp.path().join("schemas");
        std::fs::create_dir_all(&schemas_dir).unwrap();
        // Write schema JSON so list_schemas can find it
        let schema_json = json!({
            "x-substrukt": {
                "title": "Test",
                "slug": "test",
                "storage": "directory"
            },
            "type": "object",
            "properties": {
                "title": { "type": "string" }
            }
        });
        std::fs::write(
            schemas_dir.join("test.json"),
            serde_json::to_string_pretty(&schema_json).unwrap(),
        ).unwrap();

        let count = publish_all_drafts(&schemas_dir, tmp.path()).unwrap();
        assert_eq!(count, 2, "should publish 2 draft entries");

        let entries = list_entries(tmp.path(), &schema).unwrap();
        for entry in &entries {
            assert_eq!(
                entry.data.get("_status").and_then(|v| v.as_str()),
                Some("published"),
                "entry {} should be published",
                entry.id
            );
        }

        // Running again should publish 0
        let count = publish_all_drafts(&schemas_dir, tmp.path()).unwrap();
        assert_eq!(count, 0, "no drafts left to publish");
    }

    #[test]
    fn get_entry_status_returns_correct_status() {
        let data_draft = json!({"_status": "draft", "title": "Test"});
        let data_published = json!({"_status": "published", "title": "Test"});
        let data_legacy = json!({"title": "Test"});

        assert_eq!(get_entry_status(&data_draft), "draft");
        assert_eq!(get_entry_status(&data_published), "published");
        assert_eq!(get_entry_status(&data_legacy), "published");
    }

    #[test]
    fn set_entry_status_directory_mode() {
        let tmp = TempDir::new().unwrap();
        let schema = test_schema(Kind::Collection, StorageMode::Directory);
        let data = json!({"title": "Hello"});
        let id = save_entry(tmp.path(), &schema, None, data).unwrap();

        // Starts as draft
        let entry = get_entry(tmp.path(), &schema, &id).unwrap().unwrap();
        assert_eq!(get_entry_status(&entry.data), "draft");

        // Publish it
        set_entry_status(tmp.path(), &schema, &id, "published").unwrap();
        let entry = get_entry(tmp.path(), &schema, &id).unwrap().unwrap();
        assert_eq!(get_entry_status(&entry.data), "published");
        // Content untouched
        assert_eq!(entry.data.get("title").and_then(|v| v.as_str()), Some("Hello"));

        // Unpublish it
        set_entry_status(tmp.path(), &schema, &id, "draft").unwrap();
        let entry = get_entry(tmp.path(), &schema, &id).unwrap().unwrap();
        assert_eq!(get_entry_status(&entry.data), "draft");
    }

    #[test]
    fn set_entry_status_single_file_single() {
        let tmp = TempDir::new().unwrap();
        let schema = test_schema(Kind::Single, StorageMode::SingleFile);
        save_entry(tmp.path(), &schema, Some("_single"), json!({"title": "Settings"})).unwrap();

        set_entry_status(tmp.path(), &schema, "_single", "published").unwrap();
        let entry = get_entry(tmp.path(), &schema, "_single").unwrap().unwrap();
        assert_eq!(get_entry_status(&entry.data), "published");
    }

    #[test]
    fn set_entry_status_single_file_collection() {
        let tmp = TempDir::new().unwrap();
        let schema = test_schema(Kind::Collection, StorageMode::SingleFile);
        let id_a = save_entry(tmp.path(), &schema, None, json!({"title": "A"})).unwrap();
        let id_b = save_entry(tmp.path(), &schema, None, json!({"title": "B"})).unwrap();

        // Publish only entry A
        set_entry_status(tmp.path(), &schema, &id_a, "published").unwrap();

        let entry_a = get_entry(tmp.path(), &schema, &id_a).unwrap().unwrap();
        let entry_b = get_entry(tmp.path(), &schema, &id_b).unwrap().unwrap();
        assert_eq!(get_entry_status(&entry_a.data), "published");
        assert_eq!(get_entry_status(&entry_b.data), "draft", "other entry should be untouched");
    }

    #[test]
    fn set_entry_status_nonexistent_directory() {
        let tmp = TempDir::new().unwrap();
        let schema = test_schema(Kind::Collection, StorageMode::Directory);
        let result = set_entry_status(tmp.path(), &schema, "nonexistent", "published");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    #[test]
    fn set_entry_status_nonexistent_single_file_collection() {
        let tmp = TempDir::new().unwrap();
        let schema = test_schema(Kind::Collection, StorageMode::SingleFile);
        // Create one entry so the file exists
        save_entry(tmp.path(), &schema, None, json!({"title": "A"})).unwrap();

        let result = set_entry_status(tmp.path(), &schema, "nonexistent", "published");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    #[test]
    fn set_entry_status_nonexistent_file() {
        let tmp = TempDir::new().unwrap();
        let schema = test_schema(Kind::Single, StorageMode::SingleFile);
        // File does not exist at all
        let result = set_entry_status(tmp.path(), &schema, "_single", "published");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    #[test]
    fn set_entry_status_invalid_status() {
        let tmp = TempDir::new().unwrap();
        let schema = test_schema(Kind::Collection, StorageMode::Directory);
        save_entry(tmp.path(), &schema, None, json!({"title": "Hello"})).unwrap();

        let result = set_entry_status(tmp.path(), &schema, "hello", "archived");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Invalid status"));
    }

    #[test]
    fn set_entry_status_idempotent() {
        let tmp = TempDir::new().unwrap();
        let schema = test_schema(Kind::Collection, StorageMode::Directory);
        let id = save_entry(tmp.path(), &schema, None, json!({"title": "Hello"})).unwrap();

        // Entry starts as draft — unpublishing again should succeed (idempotent)
        set_entry_status(tmp.path(), &schema, &id, "draft").unwrap();
        let entry = get_entry(tmp.path(), &schema, &id).unwrap().unwrap();
        assert_eq!(get_entry_status(&entry.data), "draft");
    }

    #[test]
    fn set_entry_status_adds_field_to_legacy_entry() {
        let tmp = TempDir::new().unwrap();
        let schema = test_schema(Kind::Collection, StorageMode::Directory);
        // Write a legacy entry with no _status field
        let dir = tmp.path().join("test");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("legacy.json"), r#"{"title": "Old"}"#).unwrap();

        set_entry_status(tmp.path(), &schema, "legacy", "published").unwrap();
        let entry = get_entry(tmp.path(), &schema, "legacy").unwrap().unwrap();
        assert_eq!(get_entry_status(&entry.data), "published");
        assert_eq!(entry.data.get("title").and_then(|v| v.as_str()), Some("Old"));
    }
}
