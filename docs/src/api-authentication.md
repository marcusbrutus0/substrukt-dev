# API Authentication

All API endpoints under `/api/v1/` require a bearer token in the `Authorization` header.

## Creating tokens

### Via the UI

1. Log in to the web interface
2. Go to **Settings > API Tokens**
3. Enter a name for the token and click **Create**
4. Copy the displayed token immediately -- it is shown only once

### Via the CLI

```sh
substrukt create-token "My token name"
```

Prints the raw token to stdout. Requires at least one user to exist.

## Using tokens

Include the token in the `Authorization` header:

```sh
curl -H "Authorization: Bearer YOUR_TOKEN" \
  http://localhost:3000/api/v1/schemas
```

## Token storage

Tokens are hashed with SHA-256 before storage. The raw token is never stored -- only the hash. This means:

- Lost tokens cannot be recovered; create a new one
- Token names are for your reference only

## Managing tokens

Tokens are scoped to the user who created them. Each user can:

- View their own tokens (name and creation date)
- Delete their own tokens

Token management is available at **Settings > API Tokens** in the UI.

## Rate limiting

API requests are rate-limited to **100 requests per minute** per IP address. When the limit is exceeded, the API returns:

```
HTTP/1.1 429 Too Many Requests
```

```json
{
  "error": "Rate limit exceeded"
}
```

The rate limiter uses a sliding window per IP, determined by the `X-Forwarded-For` header (for requests behind a reverse proxy) or falls back to a default identifier.

## Error responses

| Status | Meaning |
|--------|---------|
| `401 Unauthorized` | Missing or invalid bearer token |
| `404 Not Found` | Schema or entry not found |
| `429 Too Many Requests` | Rate limit exceeded |
| `400 Bad Request` | Invalid request body or validation errors |
| `500 Internal Server Error` | Server-side error |
