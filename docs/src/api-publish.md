# Publish API

Trigger webhook notifications to rebuild your frontend.

## Trigger a publish

```
POST /api/v1/publish/:environment
```

Fires the configured webhook for the specified environment.

| Environment | Webhook flag | Behavior |
|-------------|-------------|----------|
| `staging` | `--staging-webhook-url` | Also fired automatically by the background cron |
| `production` | `--production-webhook-url` | Only fired manually via this endpoint or the UI |

### Example

```sh
curl -X POST \
  -H "Authorization: Bearer $TOKEN" \
  http://localhost:3000/api/v1/publish/production
```

### Responses

Success:

```json
{
  "status": "triggered"
}
```

Webhook URL not configured:

```
404 Not Found
```

```json
{
  "error": "Webhook URL not configured"
}
```

Webhook endpoint returned an error:

```
502 Bad Gateway
```

```json
{
  "error": "Webhook returned HTTP 500"
}
```

Invalid environment (not "staging" or "production"):

```
404 Not Found
```

```json
{
  "error": "Unknown environment"
}
```

See [Webhooks](./webhooks.md) for details on how the webhook system works.
