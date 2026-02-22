#!/bin/bash
set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"

cd "$PROJECT_DIR"

echo "==> Building release binary..."
cargo build --release

echo "==> Creating app bundle..."
bash scripts/create_app_bundle.sh

echo "==> Creating pkg installer..."
bash scripts/create_pkg.sh

echo ""
echo "Done! Output files:"
ls -lh target/release/mac-state-monitor-*.pkg
