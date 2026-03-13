# Stage 1: Build
FROM rustlang/rust:nightly-slim AS builder

WORKDIR /build

# Install build dependencies
RUN apt-get update && apt-get install -y pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*

# Copy manifests and toolchain for dependency caching
COPY Cargo.toml Cargo.lock rust-toolchain.toml build.rs ./

# Copy templates early (needed by build.rs for minijinja-embed in release)
COPY templates/ templates/

# Create dummy source to cache dependency build
RUN mkdir src && echo "fn main() {}" > src/main.rs && echo "" > src/lib.rs
RUN cargo build --release 2>/dev/null || true
RUN rm -rf src

# Copy actual source and build
COPY src/ src/
COPY migrations/ migrations/
COPY audit_migrations/ audit_migrations/
COPY templates/ templates/
RUN touch src/main.rs src/lib.rs && cargo build --release

# Stage 2: Runtime
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*

RUN useradd --create-home --shell /bin/bash substrukt

COPY --from=builder /build/target/release/substrukt /usr/local/bin/substrukt
COPY --from=builder /build/templates /opt/substrukt/templates

WORKDIR /opt/substrukt

# Create data directories as mount points
RUN mkdir -p /data/schemas /data/content /data/uploads && chown -R substrukt:substrukt /data

USER substrukt

EXPOSE 3000

VOLUME ["/data"]

ENTRYPOINT ["substrukt"]
CMD ["serve", "--data-dir", "/data", "--db-path", "/data/substrukt.db", "--port", "3000"]
