# Webhooks

Substrukt can fire webhooks when content changes, enabling automatic rebuilds of your frontend. There are two webhook environments: **staging** and **production**.

## Configuration

Configure webhook URLs via CLI flags:

```sh
substrukt serve \
  --staging-webhook-url https://api.example.com/hooks/staging-build \
  --production-webhook-url https://api.example.com/hooks/production-deploy \
  --webhook-check-interval 300
```

| Flag | Description |
|------|-------------|
| `--staging-webhook-url` | URL to POST when content changes are detected |
| `--production-webhook-url` | URL to POST on manual publish |
| `--webhook-check-interval` | Seconds between dirty-checks for staging (default: 300) |

## How it works

### Staging (automatic)

A background task runs on a timer (default: every 5 minutes). It checks whether any content mutations (create, update, delete for content or schemas) have occurred since the last webhook fire. If the staging environment is "dirty", the webhook fires automatically.

The dirty check compares the timestamp of the last webhook fire against the most recent mutation in the audit log. Non-mutation events (logins, exports) do not trigger the webhook.

### Production (manual)

Production webhooks are never fired automatically. They require an explicit action:

- **Via the UI**: Click the "Publish" button on the dashboard
- **Via the API**: `POST /api/v1/publish/production` with a bearer token

## Webhook payload

When fired, Substrukt sends a POST request with a JSON body:

```json
{
  "event_type": "substrukt-publish",
  "environment": "staging",
  "triggered_at": "2026-03-13T10:30:00+00:00",
  "triggered_by": "cron"
}
```

| Field | Values |
|-------|--------|
| `event_type` | Always `"substrukt-publish"` |
| `environment` | `"staging"` or `"production"` |
| `triggered_at` | ISO 8601 timestamp |
| `triggered_by` | `"cron"` (automatic) or `"manual"` (UI/API) |

## Staging and production are independent

Each environment tracks its dirty state separately. Firing the staging webhook does not affect the production dirty state, and vice versa. This means content can be previewed on staging before being published to production.

## Error handling

- If the webhook URL returns a non-2xx status, the error is logged and the dirty state is not cleared (the webhook will be retried on the next check)
- If no webhook URL is configured for an environment, the publish action returns an error
- Webhook fires are logged in the audit log with success/failure status

## Using with CI/CD

A common pattern is pointing the staging webhook at a CI pipeline that rebuilds and deploys a preview site:

```sh
# Example: trigger a GitHub Actions workflow
substrukt serve \
  --staging-webhook-url https://api.github.com/repos/org/site/dispatches
```

The receiving endpoint can use the payload to determine which environment to build.
