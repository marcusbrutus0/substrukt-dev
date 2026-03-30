#!/bin/sh
set -e

# Ensure data directories exist
mkdir -p /data/schemas /data/content /data/uploads /data/_history

if [ "$(id -u)" = "0" ]; then
    # Running as root: fix ownership and drop privileges
    chown -R substrukt:substrukt /data
    exec gosu substrukt "$@"
else
    # Running as non-root (e.g. Coolify, rootless Docker):
    # just verify we can write, then exec directly
    if [ ! -w /data ]; then
        echo "ERROR: /data is not writable by uid $(id -u). Mount the volume with appropriate permissions." >&2
        exit 1
    fi
    exec "$@"
fi
