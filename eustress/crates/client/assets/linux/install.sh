#!/bin/bash
# Eustress Player: Linux desktop installation
# Run from the unpacked release:  ./install.sh
# Remove it again:                ./install.sh --uninstall
#
# Installs:
#   - The Player and its assets to ~/.local/share/eustress-player/
#   - A link to it at ~/.local/bin/eustress-client
#   - The desktop entry, which makes the Player the handler for
#     eustress-player:// links
#   - Icons to ~/.local/share/icons/hicolor/

set -e
: "${HOME:?HOME is not set}"

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
APP_HOME="$HOME/.local/share/eustress-player"
BIN_DIR="$HOME/.local/bin"
APP_DIR="$HOME/.local/share/applications"
ICON_BASE="$HOME/.local/share/icons/hicolor"
SIZES="16 32 48 64 128 256 512"

refresh() {
    if command -v gtk-update-icon-cache &> /dev/null; then
        gtk-update-icon-cache -f -t "$ICON_BASE" 2>/dev/null || true
    fi
    if command -v update-desktop-database &> /dev/null; then
        update-desktop-database "$APP_DIR" 2>/dev/null || true
    fi
}

if [ "$1" = "--uninstall" ]; then
    echo "Removing Eustress Player..."
    rm -rf "$APP_HOME"
    rm -f "$BIN_DIR/eustress-client" "$APP_DIR/eustress-player.desktop"
    for size in $SIZES; do
        rm -f "$ICON_BASE/${size}x${size}/apps/eustress-player.png"
    done
    refresh
    echo "Eustress Player removed."
    exit 0
fi

if [ ! -f "$SCRIPT_DIR/eustress-client" ] || [ ! -d "$SCRIPT_DIR/common/assets" ]; then
    echo "ERROR: eustress-client and common/assets must sit beside install.sh." >&2
    echo "Run install.sh from the unpacked release." >&2
    exit 1
fi

echo "Installing Eustress Player..."

# The Player finds its assets beside its executable, so the two install
# together. An upgrade replaces the whole folder, so no asset from an older
# release outlives it.
rm -rf "$APP_HOME"
mkdir -p "$APP_HOME"
cp "$SCRIPT_DIR/eustress-client" "$APP_HOME/eustress-client"
chmod +x "$APP_HOME/eustress-client"
cp -R "$SCRIPT_DIR/common" "$APP_HOME/common"
echo "  Player → $APP_HOME"

# A link, not a copy: the Player resolves its own path through the link and
# still finds its assets.
mkdir -p "$BIN_DIR"
ln -sf "$APP_HOME/eustress-client" "$BIN_DIR/eustress-client"
echo "  Link → $BIN_DIR/eustress-client"

# Desktop entry, with Exec pointing at the installed Player
mkdir -p "$APP_DIR"
sed "s|^Exec=eustress-client|Exec=\"$APP_HOME/eustress-client\"|" \
    "$SCRIPT_DIR/eustress-player.desktop" > "$APP_DIR/eustress-player.desktop"
echo "  Desktop entry → $APP_DIR/eustress-player.desktop"

# Icons (all sizes from the icons/ subdirectory)
for size in $SIZES; do
    if [ -f "$SCRIPT_DIR/icons/eustress-player-${size}.png" ]; then
        ICON_DIR="$ICON_BASE/${size}x${size}/apps"
        mkdir -p "$ICON_DIR"
        cp "$SCRIPT_DIR/icons/eustress-player-${size}.png" "$ICON_DIR/eustress-player.png"
    fi
done
echo "  Icons → $ICON_BASE"

refresh

# eustress-player:// links. The desktop entry declares the scheme; this makes
# it the default handler, so a browser hands the link to the Player.
if command -v xdg-mime &> /dev/null \
    && xdg-mime default eustress-player.desktop x-scheme-handler/eustress-player; then
    echo "  eustress-player:// links → Eustress Player"
else
    echo "  NOTE: xdg-mime could not register the link handler, so eustress-player://"
    echo "  links will not open the Player until the desktop is told to use"
    echo "  eustress-player.desktop for x-scheme-handler/eustress-player."
fi

# Ensure ~/.local/bin is in PATH
if [[ ":$PATH:" != *":$BIN_DIR:"* ]]; then
    echo ""
    echo "NOTE: Add $BIN_DIR to your PATH:"
    echo "  echo 'export PATH=\"\$HOME/.local/bin:\$PATH\"' >> ~/.bashrc"
fi

echo ""
echo "Eustress Player installed. Open an eustress-player:// link, launch it from"
echo "your application menu, or run:"
echo "  eustress-client"
