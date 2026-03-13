# Sync API

Export and import content bundles via the API.

## Export

```
POST /api/v1/export
```

Downloads a tar.gz bundle containing all schemas, content, and uploads.

```sh
curl -X POST \
  -H "Authorization: Bearer $TOKEN" \
  http://localhost:3000/api/v1/export \
  -o backup.tar.gz
```

Response headers:

```
Content-Type: application/gzip
Content-Disposition: attachment; filename="bundle.tar.gz"
```

## Import

```
POST /api/v1/import
Content-Type: multipart/form-data
```

Uploads and extracts a tar.gz bundle, replacing existing content.

```sh
curl -X POST \
  -H "Authorization: Bearer $TOKEN" \
  -F "bundle=@backup.tar.gz" \
  http://localhost:3000/api/v1/import
```

Response:

```json
{
  "status": "ok",
  "warnings": [
    "blog-posts/bad-entry: /title: \"title\" is a required property"
  ]
}
```

The `warnings` array lists any content validation issues found during import. The import proceeds regardless of validation warnings -- data is still imported.

After import:
- Upload metadata is synced to SQLite (from manifest or legacy sidecars)
- Upload references are rebuilt from content files
- The in-memory cache is fully rebuilt

See [Import and Export](./import-export.md) for the full sync workflow.
