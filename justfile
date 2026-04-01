default:
    @just --list

build:
    cargo build

check:
    cargo check

dev:
    cargo run -- serve --port 3001

test:
    cargo test

test-integration:
    cargo test --test integration -- --test-threads=4

clippy:
    cargo clippy

fmt:
    cargo fmt

watch:
    bacon

# Documentation (requires mdBook: cargo install mdbook)
docs-build:
    mdbook build docs

docs-serve:
    mdbook serve docs --open
