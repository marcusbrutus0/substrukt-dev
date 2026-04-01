# Upload UI Improvement — Design Spec

## Goal

Improve the file upload experience in the content editor with drag-and-drop zones, image previews, file info display, and upload spinners.

## Decisions

- **Drag-and-drop**: Styled wrapper around native `<input type="file">`. Dropping a file populates the native input. No custom file handling.
- **Image previews**: Both existing uploads (server-rendered `<img>` pointing at `/uploads/file/{hash}/{filename}`) and new selections (client-side `URL.createObjectURL`).
- **Progress indication**: Spinner/disabled submit button on form submit when files are selected. No async upload — keep standard form POST.
- **File info**: Shown only for newly selected files (client-side `File.name`, `File.size`, `File.type`).
- **Preservation**: Existing `__current` hidden field strategy unchanged.

## What Changes

### `src/content/form.rs` (lines 100-123)

Replace the upload field HTML generation. New structure per upload field:

```html
<div class="mb-4">
  <label class="block text-sm font-medium text-secondary mb-1">Label *</label>
  <!-- Existing upload preview (only when editing, value exists) -->
  <div class="mb-2 text-sm text-secondary flex items-center gap-2">
    <img src="/uploads/file/{hash}/{filename}" class="h-16 w-16 object-cover rounded" />
    Current: <a href="/uploads/file/{hash}/{filename}" class="text-accent underline" target="_blank">{filename}</a>
    <input type="hidden" name="{name}.__current" value='{json}'>
  </div>
  <!-- Drop zone wrapping native file input -->
  <div class="upload-zone border-2 border-dashed border-border rounded-lg p-6 text-center cursor-pointer
              hover:border-accent transition-colors relative" data-upload-zone>
    <div class="upload-zone-prompt text-muted text-sm">
      Drag a file here or click to browse
    </div>
    <div class="upload-zone-info hidden text-sm mt-2"></div>
    <div class="upload-zone-preview hidden mt-2"></div>
    <input type="file" name="{name}" class="absolute inset-0 w-full h-full opacity-0 cursor-pointer"
           data-upload-input>
  </div>
</div>
```

For existing uploads: show `<img>` thumbnail only when MIME starts with `image/`. The MIME is available from the stored metadata object.

### `templates/base.html`

Add JS functions after the existing `initMarkdownEditors` block:

- `initUploadZones()` — attaches drag-over/drag-leave/drop visual feedback, file `change` listener
- On file selection: show file name, formatted size, type in `.upload-zone-info`; if image, show preview in `.upload-zone-preview`
- `initSubmitSpinner()` — on form submit, if any file inputs have files, disable button and show "Uploading..." text
- Both called on load and on `htmx:afterSwap` (same pattern as `initMarkdownEditors`)

## What Does NOT Change

- Upload storage (`src/uploads/mod.rs`)
- Content routes / multipart handling (`src/routes/content.rs`)
- Upload routes (`src/routes/uploads.rs`)
- Hidden `__current` field strategy
- `form_data_to_json` parsing
- No new endpoints, no new dependencies
