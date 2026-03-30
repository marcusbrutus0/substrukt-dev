# Per-Entry Draft/Publish Workflow Design

## Overview

Decouple draft/publish from deployments. Each content entry has a publish/unpublish toggle on its edit page. New entries default to draft. Editors and admins can publish or unpublish individual entries. The bulk `publish_all_drafts()` function is removed.

## Behavior

### Status

Every entry has a `_status` field: `"draft"` or `"published"`. New entries default to `"draft"`. Entries without `_status` (legacy) are treated as `"published"` for backwards compatibility.

### Publish/Unpublish

- Available on the content edit page as a button or toggle
- Publish: sets `_status` to `"published"`
- Unpublish: sets `_status` to `"draft"`
- Requires editor role or above
- Viewers can see status but cannot change it

### Content List

Status is already shown as a visual indicator on the content list page (no change needed).

## Edit Page UI

- Status indicator near the top of the edit form showing current state (draft/published badge)
- "Publish" button when entry is draft, "Unpublish" button when entry is published
- Separate from the "Save" button — publishing/unpublishing is its own action, not tied to saving content changes
- If the entry has unsaved changes and the user clicks publish, save first then publish (or warn)

## API

Existing content update endpoints (`PUT /api/v1/apps/{app-slug}/content/{schema_slug}/{entry_id}`) already accept `_status` in the body. No new API endpoints needed — clients set `_status` directly.

## Removed

- `publish_all_drafts()` function removed
- The old `POST /publish/{environment}` route is already removed in the configurable deployments spec

## Interaction with Deployments

Deployments with `include_drafts = true` serve both draft and published entries via the API. Deployments with `include_drafts = false` serve only published entries. Publishing/unpublishing an entry affects what those deployments serve on next fetch — no webhook is fired by the publish action itself.

## Audit

- `entry_published` — with app_id, schema_slug, entry_id
- `entry_unpublished` — same fields
