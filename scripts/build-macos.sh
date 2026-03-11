#!/bin/bash

# macOS build script for OpenixCLI
# Usage: ./scripts/build-macos.sh <target> <version> [output_dir]

set -e

# Get arguments
TARGET="${1:-aarch64-apple-darwin}"
VERSION="${2:-$RELEASE_TAG}"
VERSION="${VERSION#v}"  # Remove v prefix
OUTPUT_DIR="${3:-.}"

echo "Building macOS package for target: $TARGET"
echo "Version: $VERSION"

# Check if target is provided and valid
if [[ "$TARGET" != *"apple-darwin"* ]]; then
    echo "Error: Target must be an Apple Darwin target"
    exit 1
fi

# Set paths
BINARY_PATH="target/$TARGET/release/openixcli"
DMG_NAME="openixcli-v${VERSION}-${TARGET}.dmg"
TARBALL_NAME="openixcli-v${VERSION}-${TARGET}.tar.gz"

# Check if binary exists
if [[ ! -f "$BINARY_PATH" ]]; then
    echo "Error: Binary not found at $BINARY_PATH"
    echo "Please run 'cargo build --release --target $TARGET' first"
    exit 1
fi

# Create tarball
echo "Creating tarball..."
mkdir -p release
cp "$BINARY_PATH" release/
cd release
tar -czvf "../$TARBALL_NAME" openixcli
cd ..
rm -rf release
echo "Created: $TARBALL_NAME"

# Create DMG
echo "Creating DMG..."
mkdir -p dmg_temp
cp "$BINARY_PATH" dmg_temp/

# Check if libusb is dynamically linked and bundle it
LIBUSB_DYLIB=$(find "target/$TARGET/release/build" -name "libusb-1.0.dylib" 2>/dev/null | head -1)

if [[ -n "$LIBUSB_DYLIB" ]]; then
    echo "Bundling libusb dylib: $LIBUSB_DYLIB"
    cp "$LIBUSB_DYLIB" dmg_temp/

    # Fix install name and rpath
    install_name_tool -id @executable_path/libusb-1.0.dylib dmg_temp/libusb-1.0.dylib
    install_name_tool -add_rpath @executable_path dmg_temp/openixcli

    echo "LibUSB bundled successfully"
else
    # Check if libusb is linked from Homebrew or system
    if otool -L "$BINARY_PATH" | grep -q "libusb"; then
        echo "Warning: libusb is dynamically linked but dylib not found in build directory"
        echo "Binary may require libusb to be installed via Homebrew"
    else
        echo "No libusb dylib found (likely statically linked or not used)"
    fi
fi

# Create DMG
hdiutil create -volname "OpenixCLI" -srcfolder dmg_temp -ov -format UDZO "$DMG_NAME"
rm -rf dmg_temp
echo "Created: $DMG_NAME"

# Move to output directory if specified
if [[ "$OUTPUT_DIR" != "." ]]; then
    mkdir -p "$OUTPUT_DIR"
    mv "$TARBALL_NAME" "$OUTPUT_DIR/"
    mv "$DMG_NAME" "$OUTPUT_DIR/"
fi

echo ""
echo "Build complete!"
echo "  Tarball: $TARBALL_NAME"
echo "  DMG: $DMG_NAME"