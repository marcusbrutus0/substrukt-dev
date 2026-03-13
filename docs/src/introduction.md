# Introduction

Substrukt is a schema-driven CMS built in Rust. You define content types using JSON Schema, edit data through a web UI, and serve it via a REST API. Content is stored as plain JSON files on disk -- not in a database -- making it easy to version, sync, and inspect.

## Why Substrukt

Most CMS options fall into two camps: heavy database-backed systems that require significant infrastructure, or headless CMSes locked behind SaaS platforms. Substrukt takes a different approach:

- **Schema-first**: Content types are defined as JSON Schema. The UI, validation, and API are all generated from the schema at runtime. No code changes needed to add a new content type.
- **Files on disk**: Content lives as JSON files in a directory. You can read them, version them in git, or sync them between environments with a tar.gz bundle. SQLite is only used for infrastructure (users, sessions, API tokens).
- **Single binary**: One Rust binary handles everything -- the web UI, REST API, file storage, and background jobs. No external services required beyond the filesystem.
- **Minimal frontend**: The UI is server-rendered with htmx for interactivity and twind for styling. No build step, no node_modules, no bundler.

## What it does

1. You create **schemas** that describe your content types (blog posts, settings, pages, etc.)
2. The CMS generates **forms** from those schemas for editing content through the web UI
3. Content is saved as **JSON files** on disk and cached in memory for fast reads
4. A **REST API** serves the content to your frontend, static site generator, or mobile app
5. **Import/export** bundles let you sync content between local and production environments
6. **Webhooks** can trigger rebuilds of your frontend when content changes

## Core concepts

| Concept | Description |
|---------|-------------|
| **Schema** | A JSON Schema document with an `x-substrukt` extension that defines a content type |
| **Content entry** | A JSON object conforming to a schema, stored as a file on disk |
| **Upload** | A file (image, document, etc.) stored with content-addressed deduplication |
| **Bundle** | A tar.gz archive containing all schemas, content, and uploads for syncing |
| **API token** | A bearer token for authenticating API requests |
