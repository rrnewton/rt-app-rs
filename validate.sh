#!/bin/bash
set -euo pipefail

echo "=== Running cargo fmt check ==="
cargo fmt --all -- --check

echo "=== Running cargo clippy ==="
cargo clippy -- -D warnings

echo "=== Running cargo build ==="
cargo build

echo "=== Running cargo test ==="
cargo test

echo "=== All checks passed ==="
