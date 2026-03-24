# Configuration

Most configuration is passed as CLI flags. Optional features are controlled via a `substrukt.toml` file placed next to the binary.

## substrukt.toml

Place a `substrukt.toml` file in the same directory as the `substrukt` binary to enable optional features. If the file is absent, all features default to off.

```toml
[features]
serve_llms_txt = true  # Serve /llms.txt — AI agent instructions for this CMS instance
```

### Features

| Key | Default | Description |
|-----|---------|-------------|
| `serve_llms_txt` | `false` | Serve the built-in `/llms.txt` file describing the Substrukt API for AI agents |

## CLI flags

| Flag | Default | Description |
|------|---------|-------------|
| `--data-dir <PATH>` | `data` | Root directory for schemas, content, uploads, and databases |
| `--db-path <PATH>` | `<data-dir>/substrukt.db` | Path to the main SQLite database |
| `-p, --port <PORT>` | `3000` | HTTP listen port |
| `--secure-cookies` | off | Set the `Secure` flag on session cookies (required for HTTPS) |
| `--staging-webhook-url <URL>` | none | Webhook URL fired automatically when content changes |
| `--staging-webhook-auth-token <TOKEN>` | none | Bearer token sent with staging webhook requests |
| `--production-webhook-url <URL>` | none | Webhook URL fired on manual publish |
| `--production-webhook-auth-token <TOKEN>` | none | Bearer token sent with production webhook requests |
| `--webhook-check-interval <SECONDS>` | `300` | How often (in seconds) to check if the staging webhook should fire |

## Commands

Substrukt has four commands. If no command is specified, `serve` is the default.

```
substrukt serve                    # Start the web server
substrukt import <path.tar.gz>     # Import a content bundle
substrukt export <path.tar.gz>     # Export a content bundle
substrukt create-token <name>      # Create an API token from the command line
```

### serve

Starts the web server. All flags listed above apply.

```sh
substrukt serve --port 8080 --data-dir /var/lib/substrukt --secure-cookies
```

### import

Imports a tar.gz bundle into the data directory. Overwrites existing schemas and content. Validates all imported content against its schema and prints warnings for any validation errors.

```sh
substrukt import backup.tar.gz --data-dir /var/lib/substrukt
```

### export

Exports all schemas, content, and uploads into a tar.gz bundle.

```sh
substrukt export backup.tar.gz --data-dir /var/lib/substrukt
```

### create-token

Creates an API token without starting the server. Requires at least one user to exist (run the server and complete setup first).

```sh
substrukt create-token "CI deploy"
```

The raw token is printed to stdout. Save it -- it cannot be retrieved again.

## Logging

Substrukt uses the `RUST_LOG` environment variable for log filtering. The default level is `substrukt=info,tower_http=info`.

```sh
# Debug logging
RUST_LOG=substrukt=debug ./substrukt serve

# Trace everything
RUST_LOG=trace ./substrukt serve

# Only errors
RUST_LOG=error ./substrukt serve
```

The server listens on `0.0.0.0` (all interfaces) by default. The listen address is not configurable via CLI -- bind to a specific interface using a reverse proxy.
