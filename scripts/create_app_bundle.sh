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

# Prefer exporting PNG frames from RunCat; fallback to copying the .car bundle.
RUNCAT_UI_BUNDLE="/Applications/RunCat.app/Contents/Resources/LocalPackage_UserInterface.bundle"
RUNCAT_EXPORTED_FRAMES="$RESOURCES_PATH/runcat-frames"
RUNCAT_EXPORTED_WHITE_FRAMES="$RESOURCES_PATH/runcat-frames-white"
if [ -d "$RUNCAT_UI_BUNDLE" ]; then
    if swift "$SCRIPT_DIR/export_runcat_frames.swift" "$RUNCAT_UI_BUNDLE" "$RUNCAT_EXPORTED_FRAMES"; then
        echo "RunCat frames exported to: $RUNCAT_EXPORTED_FRAMES"
        if swift "$SCRIPT_DIR/export_runcat_frames.swift" "$RUNCAT_UI_BUNDLE" "$RUNCAT_EXPORTED_WHITE_FRAMES" --white; then
            echo "RunCat white frames exported to: $RUNCAT_EXPORTED_WHITE_FRAMES"
        else
            echo "RunCat white frame export failed; copying normal frames as fallback"
            rm -rf "$RUNCAT_EXPORTED_WHITE_FRAMES"
            cp -R "$RUNCAT_EXPORTED_FRAMES" "$RUNCAT_EXPORTED_WHITE_FRAMES"
        fi
    else
        echo "RunCat frame export failed; falling back to .car bundle copy"
        rm -rf "$RUNCAT_EXPORTED_FRAMES"
        rm -rf "$RUNCAT_EXPORTED_WHITE_FRAMES"
        cp -R "$RUNCAT_UI_BUNDLE" "$RESOURCES_PATH/LocalPackage_UserInterface.bundle"
    fi
fi

# Ad-hoc code sign
codesign --force --deep --sign - "$BUNDLE_PATH"

echo "App bundle created: $BUNDLE_PATH"
