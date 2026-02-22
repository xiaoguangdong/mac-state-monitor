#!/bin/bash
set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"

cd "$PROJECT_DIR"

BINARY_NAME="mac-state-monitor"
BUNDLE_NAME="Mac State Monitor.app"
BUNDLE_PATH="target/release/$BUNDLE_NAME"
VERSION="0.1.0"
IDENTIFIER="com.mac-state-monitor.app"
PKG_OUTPUT="target/release/${BINARY_NAME}-${VERSION}.pkg"

# Check that app bundle exists
if [ ! -d "$BUNDLE_PATH" ]; then
    echo "Error: App bundle not found at $BUNDLE_PATH"
    echo "Run create_app_bundle.sh first."
    exit 1
fi

# Clean previous pkg
rm -f "$PKG_OUTPUT"

# Create component package
pkgbuild \
    --identifier "$IDENTIFIER" \
    --version "$VERSION" \
    --install-location "/Applications" \
    --component "$BUNDLE_PATH" \
    "$PKG_OUTPUT"

echo "Installer created: $PKG_OUTPUT"
