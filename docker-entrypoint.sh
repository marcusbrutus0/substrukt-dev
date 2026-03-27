#!/bin/sh
set -e

# Ensure data directories exist and are writable by substrukt user
# (volume mounts may override permissions set during image build)
mkdir -p /data/schemas /data/content /data/uploads
chown -R substrukt:substrukt /data

# Drop privileges and exec the actual command
exec gosu substrukt "$@"
