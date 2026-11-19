#!/bin/bash
# Eustress Engine (Studio): Linux desktop installation
# Run from the unpacked release:  ./install.sh
# Remove it again:                ./install.sh --uninstall
#
# Installs:
#   - Studio and its assets to ~/.local/share/eustress-engine/
#   - A link to it at ~/.local/bin/eustress-engine
#   - The desktop entry, which makes Studio the handler for eustress:// links
#     (the website's Edit button) and .eustress files
#   - Icons to ~/.local/share/icons/hicolor/

set -e
: "${HOME:?HOME is not set}"

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
APP_HOME="$HOME/.local/share/eustress-engine"
BIN_DIR="$HOME/.local/bin"
APP_DIR="$HOME/.local/share/applications"
ICON_BASE="$HOME/.local/share/icons/hicolor"
SIZES="16 32 48 128 256 512"

refresh() {
    if command -v gtk-update-icon-cache &> /dev/null; then
        gtk-update-icon-cache -f -t "$ICON_BASE" 2>/dev/null || true
    fi
    if command -v update-desktop-database &> /dev/null; then
        update-desktop-database "$APP_DIR" 2>/dev/null || true
    fi
}

if [ "$1" = "--uninstall" ]; then
    echo "Removing Eustress Engine..."
    rm -rf "$APP_HOME"
    rm -f "$BIN_DIR/eustress-engine" "$APP_DIR/eustress-engine.desktop"
    for size in $SIZES; do
        rm -f "$ICON_BASE/${size}x${size}/apps/eustress-engine.png"
    done
    refresh
    echo "Eustress Engine removed. Your Universes in ~/Documents/Eustress are kept."
    exit 0
fi

if [ ! -f "$SCRIPT_DIR/eustress-engine" ] || [ ! -d "$SCRIPT_DIR/assets" ] || [ ! -d "$SCRIPT_DIR/common/assets" ]; then
    echo "ERROR: eustress-engine, assets and common/assets must sit beside install.sh." >&2
    echo "Run install.sh from the unpacked release." >&2
    exit 1
fi

echo "Installing Eustress Engine..."

# Studio finds its assets beside its executable, so they install together.
# An upgrade replaces the whole folder, so no asset from an older release
# outlives it. Universes live in ~/Documents/Eustress, not here.
rm -rf "$APP_HOME"
mkdir -p "$APP_HOME"
cp "$SCRIPT_DIR/eustress-engine" "$APP_HOME/eustress-engine"
chmod +x "$APP_HOME/eustress-engine"
cp -R "$SCRIPT_DIR/assets" "$APP_HOME/assets"
cp -R "$SCRIPT_DIR/common" "$APP_HOME/common"
if [ -d "$SCRIPT_DIR/docs" ]; then
    cp -R "$SCRIPT_DIR/docs" "$APP_HOME/docs"
fi
echo "  Studio → $APP_HOME"

# A link, not a copy: Studio resolves its own path through the link and
# still finds its assets.
mkdir -p "$BIN_DIR"
ln -sf "$APP_HOME/eustress-engine" "$BIN_DIR/eustress-engine"
echo "  Link → $BIN_DIR/eustress-engine"

# Desktop entry, with Exec pointing at the installed Studio
mkdir -p "$APP_DIR"
sed "s|^Exec=eustress-engine|Exec=\"$APP_HOME/eustress-engine\"|" \
    "$SCRIPT_DIR/eustress-engine.desktop" > "$APP_DIR/eustress-engine.desktop"
echo "  Desktop entry → $APP_DIR/eustress-engine.desktop"

# Icons (all sizes from the icons/ subdirectory)
for size in $SIZES; do
    if [ -f "$SCRIPT_DIR/icons/eustress-engine-${size}.png" ]; then
        ICON_DIR="$ICON_BASE/${size}x${size}/apps"
        mkdir -p "$ICON_DIR"
        cp "$SCRIPT_DIR/icons/eustress-engine-${size}.png" "$ICON_DIR/eustress-engine.png"
    fi
done
echo "  Icons → $ICON_BASE"

refresh

# eustress:// links. The desktop entry declares the scheme; this makes Studio
# the default handler, so a browser hands the website's Edit link to Studio.
if command -v xdg-mime &> /dev/null \
    && xdg-mime default eustress-engine.desktop x-scheme-handler/eustress; then
    echo "  eustress:// links → Eustress Engine"
else
    echo "  NOTE: xdg-mime could not register the link handler, so eustress://"
    echo "  links (the website's Edit button) will not open Studio until the"
    echo "  desktop is told to use eustress-engine.desktop for"
    echo "  x-scheme-handler/eustress."
fi

# Ensure ~/.local/bin is in PATH
if [[ ":$PATH:" != *":$BIN_DIR:"* ]]; then
    echo ""
    echo "NOTE: Add $BIN_DIR to your PATH:"
    echo "  echo 'export PATH=\"\$HOME/.local/bin:\$PATH\"' >> ~/.bashrc"
fi

echo ""
echo "Eustress Engine installed. Press Edit on one of your Spaces on eustress.dev,"
echo "launch it from your application menu, or run:"
echo "  eustress-engine"
