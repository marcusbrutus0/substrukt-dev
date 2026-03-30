# S3-Compatible Backups Design

## Overview

Full-system backups to any S3-compatible storage (AWS S3, Minio, R2, Backblaze, etc.) with user-defined frequency. Backs up all app directories and both SQLite databases as a single tar.gz. Credentials configured via environment variables, frequency and retention configured via admin UI. Includes manual trigger and basic status display.

## Configuration

### Environment Variables (credentials)

- `SUBSTRUKT_S3_ENDPOINT` — S3-compatible endpoint URL
- `SUBSTRUKT_S3_BUCKET` — bucket name
- `SUBSTRUKT_S3_ACCESS_KEY`
- `SUBSTRUKT_S3_SECRET_KEY`
- `SUBSTRUKT_S3_REGION` — optional, defaults to `us-east-1`

If any required env var is missing, the backup feature is disabled entirely (no background task, UI shows "not configured").

### Database Tables

```sql
CREATE TABLE backup_config (
    id INTEGER PRIMARY KEY CHECK (id = 1),  -- singleton row
    frequency_hours INTEGER NOT NULL DEFAULT 24,
    retention_count INTEGER NOT NULL DEFAULT 7,
    enabled INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE backup_status (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    started_at TEXT NOT NULL,
    completed_at TEXT,
    status TEXT NOT NULL,          -- 'running', 'success', 'failed'
    error_message TEXT,
    size_bytes INTEGER,
    s3_key TEXT
);
```

Only the most recent `backup_status` row matters for the UI. Older rows cleaned up on a rolling basis (keep last 10 for debugging).

### S3 Key Format

`backups/{timestamp}.tar.gz` (e.g. `backups/2026-03-30T12-00-00Z.tar.gz`)

## Backup Format

### Contents (tar.gz)

```
├── manifest.json
├── substrukt.db
├── audit.db
├── {app-slug}/
│   ├── schemas/
│   ├── content/
│   ├── uploads/
│   └── _history/
├── {another-app}/
│   └── ...
```

### manifest.json

```json
{
    "version": 1,
    "timestamp": "2026-03-30T12:00:00Z",
    "apps": ["blog", "docs"],
    "substrukt_version": "0.1.0"
}
```

## Backup Process

1. Insert `backup_status` row with status `running`
2. Use SQLite's online backup API to snapshot both `substrukt.db` and `audit.db` to temp files (consistent point-in-time copy without locking)
3. Build tar.gz in a temp file: manifest + DB snapshots + all app directories from `data/`
4. Upload tar.gz to S3
5. Update `backup_status` row: `success` + `size_bytes` + `s3_key`
6. Run retention cleanup: list objects in `backups/` prefix, delete oldest beyond `retention_count`
7. On any failure: update status to `failed` with `error_message`, clean up temp files

## Background Task & Scheduling

### Startup

If S3 env vars are present, spawn a tokio task that:

1. Reads `backup_config` from DB (frequency, enabled)
2. Reads latest `backup_status` to determine last successful backup time
3. Calculates next backup time
4. Sleeps until then (using `tokio::time::sleep`)
5. Runs backup
6. Loops back to step 1

Re-reads config each cycle so UI changes to frequency/enabled take effect without restart.

### Manual Trigger

UI posts to an endpoint which sends a message via `tokio::sync::mpsc` channel to the background task, waking it to run immediately. Same channel used for config changes so the sleep duration recalculates.

### Graceful Shutdown

Background task listens for a cancellation token. If a backup is in progress during shutdown, it finishes the current step (upload) before exiting.

## Admin UI

### Location

`/settings/backups` — global, not per-app. Admin only.

Nav link under global settings alongside Users and Invitations.

### Page Content

- **Status banner:** "Last backup: 2 hours ago (success)" or "Last backup failed: connection timeout" or "No backups yet" or "Backups not configured (S3 credentials missing)"
- **Next backup:** "In 4 hours" (or "Disabled" if not enabled)
- **"Back up now" button:** triggers immediate backup, disabled while one is running
- **Configuration form:**
  - Enabled toggle
  - Frequency dropdown: every 6h, 12h, 24h, 48h, 7d
  - Retention count: number input, min 1
- **Credential status:** green check / red x per env var, no values displayed

## Audit & Error Handling

### Audit Log Entries

- `backup_started` — manual or scheduled
- `backup_completed` — with size
- `backup_failed` — with error message
- `backup_config_changed` — when admin updates frequency/retention/enabled

All with `app_id = NULL` (global events).

### Error Handling

- S3 connection failure → log error, mark status `failed`, retry next cycle
- Disk read failure → same
- No retry within the same cycle — wait for next scheduled run or manual trigger
- Temp files cleaned up in all cases (success or failure)

### Monitoring

No built-in alerting. Admins check the UI or monitor `backup_status` table. Prometheus metric `substrukt_backup_last_success_timestamp` exposed for external alerting.

## Restore

Manual process for now. Download backup from S3, extract, replace `data/` directory contents. No in-app restore mechanism.
