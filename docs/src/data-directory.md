# Data Directory Layout

All persistent data lives under the data directory (default: `data/`, configurable via `--data-dir`).

```
data/
  substrukt.db                  # Main SQLite database
  audit.db                      # Audit log database
  schemas/                      # JSON Schema files
    blog-posts.json
    site-settings.json
    faq.json
  content/                      # Content entries
    blog-posts/                 # Directory mode: one file per entry
      my-first-post.json
      another-post.json
    site-settings/              # Single kind uses directory mode internally
      _single.json
    faq.json                    # Single-file mode: all entries in one file
  uploads/                      # Content-addressed file storage
    a1/                         # First 2 hex chars of SHA-256 hash
      b2c3d4e5f6...             # Remaining hash chars (file data)
    c7/
      d8e9f0a1b2...
```

## Databases

### substrukt.db

The main SQLite database. Stores:

- **users** -- usernames and Argon2 password hashes
- **sessions** -- active login sessions (managed by tower-sessions)
- **api_tokens** -- hashed bearer tokens with names and creation dates
- **uploads** -- upload metadata (hash, filename, MIME type, size)
- **upload_references** -- mapping of which content entries reference which uploads

This database is created automatically on first run. Migrations run at startup.

### audit.db

A separate SQLite database for audit logging. Stores:

- **audit_log** -- timestamped records of all mutations
- **webhook_state** -- last-fired timestamps for staging and production webhooks

Separated from the main database so audit writes (async) don't contend with request-handling queries.

## Schemas directory

Each schema is a single JSON file named `<slug>.json`. The file contains the full JSON Schema document including the `x-substrukt` extension.

## Content directory

Content layout depends on the schema's storage mode:

### Directory mode

```
content/<slug>/
  <entry-id>.json
```

Each entry is a standalone JSON object.

### Single-file mode

```
content/<slug>.json
```

All entries in a JSON array, each with an `_id` field.

### Single kind

Single schemas (where `kind: "single"`) use directory mode with a fixed entry ID of `_single`:

```
content/<slug>/
  _single.json
```

## Uploads directory

Files are stored by their SHA-256 hash split into a 2-character prefix and the remaining characters:

```
uploads/
  <hash[0..2]>/
    <hash[2..]>       # The file data
```

This directory structure prevents any single directory from having too many files. Upload metadata (original filename, MIME type, size) is stored in `substrukt.db`, not on the filesystem.

## Backup

To back up a Substrukt instance, you need:

1. The data directory (schemas, content, uploads)
2. `substrukt.db` (users and tokens)
3. Optionally, `audit.db` (audit history)

Or use `substrukt export` to create a tar.gz bundle of schemas, content, and uploads. Note that the export does not include users or tokens.
