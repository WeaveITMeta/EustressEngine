#!/bin/bash
# Eustress Engine — Linux Desktop Installation
# Run: chmod +x install.sh && ./install.sh
#
# Installs:
#   - Binary to ~/.local/bin/eustress-engine
#   - Desktop entry to ~/.local/share/applications/
#   - Icons to ~/.local/share/icons/hicolor/

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
BIN_DIR="$HOME/.local/bin"
APP_DIR="$HOME/.local/share/applications"
ICON_BASE="$HOME/.local/share/icons/hicolor"

echo "Installing Eustress Engine..."

# Binary
mkdir -p "$BIN_DIR"
if [ -f "$SCRIPT_DIR/eustress-engine" ]; then
    cp "$SCRIPT_DIR/eustress-engine" "$BIN_DIR/eustress-engine"
    chmod +x "$BIN_DIR/eustress-engine"
    echo "  Binary → $BIN_DIR/eustress-engine"
else
    echo "  WARNING: eustress-engine binary not found in $SCRIPT_DIR"
fi

# Desktop entry
mkdir -p "$APP_DIR"
cp "$SCRIPT_DIR/eustress-engine.desktop" "$APP_DIR/eustress-engine.desktop"
# Update Exec path to absolute
sed -i "s|^Exec=eustress-engine|Exec=$BIN_DIR/eustress-engine|" "$APP_DIR/eustress-engine.desktop"
echo "  Desktop entry → $APP_DIR/eustress-engine.desktop"

# Icons (all sizes from the icons/ subdirectory)
for size in 16 32 48 128 256 512; do
    ICON_DIR="$ICON_BASE/${size}x${size}/apps"
    mkdir -p "$ICON_DIR"
    if [ -f "$SCRIPT_DIR/icons/eustress-engine-${size}.png" ]; then
        cp "$SCRIPT_DIR/icons/eustress-engine-${size}.png" "$ICON_DIR/eustress-engine.png"
        echo "  Icon ${size}x${size} → $ICON_DIR/eustress-engine.png"
    fi
done

# Update icon cache
if command -v gtk-update-icon-cache &> /dev/null; then
    gtk-update-icon-cache -f -t "$ICON_BASE" 2>/dev/null || true
fi

# Update desktop database
if command -v update-desktop-database &> /dev/null; then
    update-desktop-database "$APP_DIR" 2>/dev/null || true
fi

# Ensure ~/.local/bin is in PATH
if [[ ":$PATH:" != *":$BIN_DIR:"* ]]; then
    echo ""
    echo "NOTE: Add $BIN_DIR to your PATH:"
    echo "  echo 'export PATH=\"\$HOME/.local/bin:\$PATH\"' >> ~/.bashrc"
fi

echo ""
echo "Eustress Engine installed. Launch from your application menu or run:"
echo "  eustress-engine"
