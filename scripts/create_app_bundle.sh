#!/bin/bash
set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"

cd "$PROJECT_DIR"

BINARY_NAME="mac-state-monitor"
BUNDLE_NAME="Mac State Monitor.app"
BUNDLE_PATH="target/release/$BUNDLE_NAME"
CONTENTS_PATH="$BUNDLE_PATH/Contents"
MACOS_PATH="$CONTENTS_PATH/MacOS"
RESOURCES_PATH="$CONTENTS_PATH/Resources"

# Clean previous bundle
rm -rf "$BUNDLE_PATH"

# Create bundle structure
mkdir -p "$MACOS_PATH"
mkdir -p "$RESOURCES_PATH"

# Copy binary
cp "target/release/$BINARY_NAME" "$MACOS_PATH/$BINARY_NAME"
chmod +x "$MACOS_PATH/$BINARY_NAME"

# Copy Info.plist
cp "resources/Info.plist" "$CONTENTS_PATH/Info.plist"

# Copy icon if exists
if [ -f "assets/AppIcon.icns" ]; then
    cp "assets/AppIcon.icns" "$RESOURCES_PATH/AppIcon.icns"
fi

# Ad-hoc code sign
codesign --force --deep --sign - "$BUNDLE_PATH"

echo "App bundle created: $BUNDLE_PATH"
