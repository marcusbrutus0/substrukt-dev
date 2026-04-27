# Milkdown Rich Text Editor — Design Spec

## Motivation

Substrukt has `format: "markdown"` fields backed by EasyMDE — a plain-text markdown editor with toolbar and preview. This works for simple content, but falls short for blog authoring: no drag-drop image uploads, no block-level editing (reordering paragraphs), no slash commands, and the split-pane preview is a poor substitute for true WYSIWYG.

This spec adds a new `format: "markdown-richtext"` field type that opens a full-screen Milkdown editor in a modal overlay. Milkdown is a ProseMirror-based WYSIWYG markdown editor with plugin support for drag-drop uploads, block handles, slash commands, and inline formatting toolbars. The existing `format: "markdown"` + EasyMDE is preserved unchanged.

## Goals

1. New `format: "markdown-richtext"` schema field type with Milkdown WYSIWYG editing in a full-screen modal
2. Drag-drop image uploads that go through Substrukt's existing upload system
3. Portable image references using `upload:hash/filename` URI scheme, resolved to real URLs at display time
4. Dual storage: raw markdown (source of truth for re-editing) + pre-rendered HTML (for API consumers)
5. API serves the field as a string — `html` by default, `markdown` with `?render=raw`
6. No JS build step in the normal workflow — Milkdown is bundled once and checked into the repo

## What Is NOT In Scope

- Replacing the existing `format: "markdown"` + EasyMDE — both coexist
- Custom Milkdown plugins beyond the Crepe defaults (can be added later)
- Collaborative editing / real-time sync
- Server-side re-rendering of stored HTML — the client-rendered HTML is authoritative

## Architecture

### Schema Definition

A `markdown-richtext` field is declared as:

```json
{
  "body": {
    "type": "string",
    "format": "markdown-richtext",
    "description": "Rich blog post content"
  }
}
```

The schema says `type: "string"` because the field behaves as a string from the API consumer's perspective. Internally, it stores a JSON object.

**Schema validation bypass**: Substrukt's content validation runs JSON Schema checks against stored data. For `markdown-richtext` fields, the content layer skips the standard `type: "string"` validation — the field is validated by its own logic (must be an object with `markdown` and `html` string keys) rather than by JSON Schema's type checker. This is the same pattern as `format: "upload"`, which declares `type: "string"` but stores a JSON object `{hash, filename, mime}`.

### Storage Format

The content layer stores the field as a JSON object with two keys:

```json
{
  "body": {
    "markdown": "# Hello\n\n![photo](upload:a1b2c3def4/photo.jpg)\n\nSome text...",
    "html": "<h1>Hello</h1><figure><img src=\"/apps/blog/uploads/file/a1b2c3def4/photo.jpg\" alt=\"photo\"></figure><p>Some text...</p>"
  }
}
```

- `markdown`: raw markdown with `upload:hash/filename` URIs for images. Source of truth for re-editing.
- `html`: pre-rendered HTML from Milkdown's `getHTML()`, with `upload:` URIs resolved to real `/apps/{slug}/uploads/file/...` paths. Ready for API consumption.

### API Behavior

The API projects the stored object down to a string, matching the `type: "string"` schema declaration:

| Render mode | Field value returned |
|---|---|
| Default / `?render=html` | The `html` string |
| `?render=raw` | The `markdown` string |

**Intentional divergence from `format: "markdown"`**: the existing markdown format defaults to raw unless `x-substrukt.render: html` is set. The `markdown-richtext` format defaults to `html` always — the whole point is pre-rendered content. Consumers get HTML unless they explicitly ask for raw markdown.

Consumers like eigen just read the field as an HTML string. No special handling needed.

### Upload URI Scheme

Images uploaded via the Milkdown editor use a custom `upload:` URI scheme in the markdown source:

```markdown
![alt text](upload:a1b2c3def4/photo.jpg)
```

This is resolved to real URLs in two places:

1. **In the editor** — Crepe's `proxyDomURL` hook resolves `upload:hash/filename` to `/apps/{slug}/uploads/file/hash/filename` for display
2. **At save time** — the `html` output from `getHTML()` is post-processed server-side by `resolve_upload_uris()` to ensure all `upload:` references are resolved to absolute paths

The `markdown` source retains `upload:` URIs — they are portable across app renames.

### Upload Flow

When a user drag-drops or pastes an image into the Milkdown editor:

1. Crepe's `onUpload` callback fires with the `File`
2. JS builds a `FormData` and POSTs to `/apps/{slug}/uploads` (session-authenticated web upload endpoint)
3. Server returns `{hash, filename, mime, size}`
4. Callback returns `upload:{hash}/{filename}` — stored as the image `src` in the ProseMirror document and serialized to markdown
5. Crepe's `proxyDomURL` resolves the URI for immediate display in the editor

No new upload endpoints needed — reuses the existing infrastructure entirely.

## Frontend

### Milkdown Integration via Crepe

Use `@milkdown/crepe` — the batteries-included Milkdown package. Crepe bundles: clipboard, history, listener, upload, GFM (tables, strikethrough, task lists), block handles, slash commands, inline tooltip, image blocks, and cursor plugins.

Crepe's `ImageBlock` feature provides two hooks that enable our custom upload scheme:

- `onUpload(file: File) → Promise<string>`: upload the file, return `upload:hash/filename`
- `proxyDomURL(url: string) → string`: resolve `upload:` URIs to real paths for editor display

### Bundle Strategy

- **Source**: `editor/milkdown-editor.js` — Crepe initialization, upload handler, URI resolver, modal lifecycle, theme overrides
- **Dependencies**: `package.json` in `editor/` with `@milkdown/crepe` and `@milkdown/kit`
- **Build**: `just bundle-editor` runs `esbuild` (from Nix devshell) to produce a single self-contained bundle
- **Output**: `static/js/milkdown-editor.bundle.js` — checked into the repo
- **When to rebuild**: only when updating Milkdown or changing the editor source

The `flake.nix` devshell adds `esbuild` to the packages list. For one-off use before rebuilding the shell: `nix run nixpkgs#esbuild -- <args>`.

### Theme

Crepe ships dark themes (`@milkdown/crepe/theme/frame-dark.css`). The CSS is inlined into the bundle by esbuild. Wavefunk design token overrides are applied via CSS variables on `.milkdown`:

```css
.milkdown {
  --crepe-color-background: var(--bg);
  --crepe-color-surface: var(--bg-raised);
  --crepe-color-on-background: var(--fg);
  --crepe-color-primary: var(--accent);
  --crepe-color-outline: var(--hairline);
  --crepe-font-default: var(--font-body);
  --crepe-font-code: var(--font-mono);
}
```

### Modal UX

Uses the wavefunk design system modal (`wf-modal`, `wf-overlay`) with a new `wf-modal--lg` size modifier:

```css
.wf-modal.wf-modal--lg {
  width: min(1200px, calc(100vw - 64px));
  max-height: calc(100vh - 64px);
  top: 32px;
}
```

Keeps the existing centering (`left: 50%; transform: translateX(-50%)`), `is-open` toggle, and `data-modal-close` hooks. Just wider and taller than the default 560px modal.

Markup structure:

```html
<div class="wf-overlay" id="richtext-overlay-{name}"></div>
<div class="wf-modal wf-modal--lg" id="richtext-modal-{name}">
  <div class="wf-modal-head">
    <span class="wf-modal-title">EDIT: {LABEL}</span>
    <div style="display: flex; gap: var(--space-2);">
      <button class="wf-btn" data-richtext-discard>Discard</button>
      <button class="wf-btn primary" data-richtext-save>Save & Close</button>
    </div>
  </div>
  <div class="wf-modal-body" data-richtext-root
       style="padding: 0; flex: 1; overflow: auto;">
    <!-- Crepe mounts here -->
  </div>
</div>
```

**Modal lifecycle:**

1. User clicks "Open Editor" button on the form field
2. JS opens the modal (`is-open` class), initializes Crepe in `[data-richtext-root]` with the current markdown value and the app slug (for upload URL resolution)
3. User edits content — drag-drop images upload immediately, slash commands available, block handles for reordering
4. "Save & Close": calls `crepe.getMarkdown()` and `getHTML()`, writes `{markdown, html}` JSON into the hidden form input, updates the preview area, destroys Crepe, closes modal
5. "Discard": destroys Crepe without writing back, closes modal
6. Form submission proceeds as normal — the hidden input contains the serialized JSON

No auto-save. Content only writes back on explicit "Save & Close".

### Form Field Rendering

The `form.rs` match arm for `("string", Some("markdown-richtext"))` renders:

- A **preview area** showing a truncated text excerpt from the current `html` value (or "Click to edit" placeholder if empty)
- A **hidden input** (`type="hidden"`) holding the JSON-serialized `{markdown, html}` value
- An **"Open Editor" button** that triggers the modal
- The **modal markup** (overlay + modal container)

The hidden input's `name` matches the field name so form submission includes the value in the multipart payload.

## Server-Side Changes

### `src/content/form.rs`

New match arm in `render_form_field()`:

```
("string", Some("markdown-richtext")) => { ... }
```

Renders preview area + hidden input + "Open Editor" button + modal markup. Reads the current value as a JSON object, extracts `markdown` for editor initialization and `html` for the preview.

### `src/content/mod.rs`

**Validation**: when saving content, if the schema declares `format: "markdown-richtext"`, the field value must be a JSON object with `markdown` (string) and `html` (string) keys. Reject anything else with a validation error.

**`resolve_upload_uris(html: &str, app_slug: &str) -> String`**: replaces `upload:hash/filename` references in HTML `src` and `href` attributes with `/apps/{slug}/uploads/file/hash/filename`. Runs at save time on the stored `html` value. Uses a regex anchored to `src="upload:` and `href="upload:` patterns to avoid false matches in alt text, code blocks, or other content.

**`render_markdown_fields`**: add a new match for `format: "markdown-richtext"`. Unlike the plain `markdown` format (which renders server-side), this format already has pre-rendered HTML. The function projects the stored object to either `html` or `markdown` string based on the render mode.

### `src/routes/api.rs`

Modify the rendering logic in `list_entries`, `get_entry`, and `get_single` to handle `markdown-richtext` fields. When rendering:

- Default / `render=html`: replace the `{markdown, html}` object with just the `html` string value
- `render=raw`: replace with just the `markdown` string value

This projection happens alongside the existing `render_markdown_fields` call.

### `src/routes/content.rs`

Modify form submission handling to accept the `markdown-richtext` field value as a JSON string (from the hidden input) and parse it into the `{markdown, html}` object for storage.

Run `resolve_upload_uris()` on the `html` value before storing to ensure all `upload:` URIs are resolved.

## Security: HTML Trust Model

The existing `format: "markdown"` strips raw HTML via pulldown-cmark as a defense-in-depth measure. The `markdown-richtext` format takes a different approach: the HTML is generated by Milkdown's ProseMirror serializer, not authored directly. Users cannot inject arbitrary HTML through the WYSIWYG editor — ProseMirror only produces HTML from its schema-defined node types.

The stored HTML is trusted as-is. No server-side sanitization is applied. This is the same trust level as any CMS that stores HTML content — editors and admins are trusted authors.

## Bundle Loading

The Milkdown bundle (`milkdown-editor.bundle.js`) is loaded conditionally: only on pages that contain a `[data-richtext]` form field. The template passes a flag (e.g., `has_richtext`) when rendering the content edit form, and the `<script>` tag is gated on that flag. Non-editor pages (dashboards, uploads list, schema list) don't load the bundle.

## Files Changed

| File | Change |
|---|---|
| `flake.nix` | Add `esbuild` to devshell packages |
| `justfile` | Add `bundle-editor` command |
| `editor/package.json` | New — `@milkdown/crepe`, `@milkdown/kit` dependencies |
| `editor/milkdown-editor.js` | New — Crepe setup, upload handler, URI resolver, modal lifecycle, theme |
| `static/js/milkdown-editor.bundle.js` | New — bundled output (checked in) |
| `static/css/04-components.css` | Add `wf-modal--lg` modifier class |
| `templates/base.html` | Conditionally load milkdown bundle when `[data-richtext]` fields exist, add theme overrides |
| `src/content/form.rs` | New `markdown-richtext` match arm for form rendering |
| `src/content/mod.rs` | Validation for `{markdown, html}` object, `resolve_upload_uris()`, field projection in `render_markdown_fields` |
| `src/routes/api.rs` | Handle `markdown-richtext` in render logic — project to string |
| `src/routes/content.rs` | Parse JSON value from hidden input, resolve upload URIs at save time |

### No changes to

- Upload routes or upload system — fully reused, session-authenticated (no CSRF on the upload POST endpoint)
- Existing `format: "markdown"` + EasyMDE — untouched

## Testing

### Unit Tests

- `resolve_upload_uris` correctly replaces `upload:hash/filename` in img src attributes
- `resolve_upload_uris` leaves non-upload URLs unchanged
- `resolve_upload_uris` handles multiple images in one HTML string
- `render_markdown_fields` projects `markdown-richtext` objects to `html` string in html render mode
- `render_markdown_fields` projects `markdown-richtext` objects to `markdown` string in raw render mode
- Validation rejects `markdown-richtext` fields that are plain strings (not objects)
- Validation rejects objects missing `markdown` or `html` keys
- Validation accepts well-formed `{markdown, html}` objects

### Integration Tests

- Create entry with `markdown-richtext` field, GET via API returns HTML string
- GET with `?render=raw` returns raw markdown string with `upload:` URIs
- Upload images are accessible at the URLs embedded in the HTML
- Round-trip: create entry, GET it, verify the HTML matches what was stored

### Manual Testing

- Open content form with a `markdown-richtext` field, verify modal opens
- Type markdown content, verify WYSIWYG rendering
- Drag-drop an image, verify it uploads and appears in the editor
- Save & Close, verify the preview updates
- Submit the form, verify the content is saved correctly
- Re-edit the entry, verify the markdown loads back into the editor
- Verify the API returns clean HTML with resolved image URLs

## Implementation Order

1. Add `esbuild` to `flake.nix`, add `bundle-editor` to justfile
2. Create `editor/` directory with `package.json` and `milkdown-editor.js` source
3. Bundle and check in `static/js/milkdown-editor.bundle.js`
4. Add `wf-modal--lg` to `static/css/04-components.css`
5. Add `markdown-richtext` match arm in `form.rs` (preview + hidden input + modal markup)
6. Add `resolve_upload_uris()` and validation in `content/mod.rs`
7. Update `render_markdown_fields` to handle `markdown-richtext` projection
8. Update API routes to project `markdown-richtext` fields to strings
9. Update `content.rs` form submission to parse and store the JSON value
10. Load milkdown bundle and theme overrides in `base.html`
11. Integration tests
12. Manual end-to-end testing
