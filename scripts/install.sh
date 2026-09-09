#!/usr/bin/env bash
set -euo pipefail

# OpenWhisper Setup & Installation Script
# Builds the release binary and deploys all desktop assets, icons, and systemd units.

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$SCRIPT_DIR"

echo "==> Building OpenWhisper release binary..."
export CARGO_HOME="${CARGO_HOME:-$HOME/.cache/puccinialin/cargo}"
export PATH="$HOME/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$CARGO_HOME/bin:$PATH"
export PKG_CONFIG_PATH="$HOME/.local/lib/pkgconfig:$HOME/.local/share/pkgconfig:${PKG_CONFIG_PATH:-}:/usr/lib64/pkgconfig:/usr/share/pkgconfig"
cargo build --release

BIN_SOURCE="$SCRIPT_DIR/target/release/openwhisper"

echo "==> Installing binary..."
# Prefer /usr/local/bin if writable or via sudo, fallback to ~/.local/bin
if [ -w /usr/local/bin ]; then
    install -Dm755 "$BIN_SOURCE" /usr/local/bin/openwhisper
    echo "    Installed to /usr/local/bin/openwhisper"
elif command -v sudo >/dev/null 2>&1 && sudo -n true 2>/dev/null; then
    sudo install -Dm755 "$BIN_SOURCE" /usr/local/bin/openwhisper
    echo "    Installed to /usr/local/bin/openwhisper (via sudo)"
else
    mkdir -p "$HOME/.local/bin"
    install -Dm755 "$BIN_SOURCE" "$HOME/.local/bin/openwhisper"
    echo "    Installed to $HOME/.local/bin/openwhisper"
fi

# Always maintain ~/.local/bin copy as well
mkdir -p "$HOME/.local/bin"
install -Dm755 "$BIN_SOURCE" "$HOME/.local/bin/openwhisper"

echo "==> Installing icons..."
ICON_DIR="$HOME/.local/share/icons/hicolor"
mkdir -p "$ICON_DIR/scalable/apps" "$ICON_DIR/scalable/status" "$HOME/.local/share/pixmaps"

# Ensure hicolor index.theme exists so gtk-update-icon-cache and KDE icon loaders recognize the directory
if [ ! -f "$ICON_DIR/index.theme" ]; then
    if [ -f "/usr/share/icons/hicolor/index.theme" ]; then
        cp "/usr/share/icons/hicolor/index.theme" "$ICON_DIR/index.theme"
    else
        cat > "$ICON_DIR/index.theme" << 'EOF'
[Icon Theme]
Name=Hicolor
Comment=Fallback icon theme
Hidden=true
Directories=16x16/apps,24x24/apps,32x32/apps,48x48/apps,64x64/apps,128x128/apps,256x256/apps,512x512/apps,scalable/apps,scalable/status
EOF
    fi
fi

install -Dm644 assets/icons/openwhisper.svg "$ICON_DIR/scalable/apps/openwhisper.svg"
install -Dm644 assets/icons/openwhisper-tray-idle.svg "$ICON_DIR/scalable/status/openwhisper-tray-idle.svg"
install -Dm644 assets/icons/openwhisper-tray-recording.svg "$ICON_DIR/scalable/status/openwhisper-tray-recording.svg"
install -Dm644 assets/icons/openwhisper-tray-transcribing.svg "$ICON_DIR/scalable/status/openwhisper-tray-transcribing.svg"
install -Dm644 assets/icons/openwhisper-tray-degraded.svg "$ICON_DIR/scalable/status/openwhisper-tray-degraded.svg"
install -Dm644 assets/icons/openwhisper-tray-error.svg "$ICON_DIR/scalable/status/openwhisper-tray-error.svg"

# Also install into pixmaps as universal fallback
install -Dm644 assets/icons/openwhisper.svg "$HOME/.local/share/pixmaps/openwhisper.svg"
install -Dm644 assets/icons/openwhisper-tray-idle.svg "$HOME/.local/share/pixmaps/openwhisper-tray-idle.svg"
install -Dm644 assets/icons/openwhisper-tray-recording.svg "$HOME/.local/share/pixmaps/openwhisper-tray-recording.svg"
install -Dm644 assets/icons/openwhisper-tray-transcribing.svg "$HOME/.local/share/pixmaps/openwhisper-tray-transcribing.svg"
install -Dm644 assets/icons/openwhisper-tray-degraded.svg "$HOME/.local/share/pixmaps/openwhisper-tray-degraded.svg"
install -Dm644 assets/icons/openwhisper-tray-error.svg "$HOME/.local/share/pixmaps/openwhisper-tray-error.svg"

# Clean up any previously generated low-resolution raster status PNGs to prevent blurry scaling
rm -f "$ICON_DIR"/*/status/openwhisper-tray-*.png "$HOME/.local/share/pixmaps"/openwhisper-tray-*.png 2>/dev/null || true

# Install vector SVGs and symbolic aliases for all tray states
mkdir -p "$ICON_DIR/scalable/status" "$ICON_DIR/symbolic/status"
for icon in idle recording transcribing degraded error; do
    install -Dm644 "assets/icons/openwhisper-tray-${icon}.svg" "$ICON_DIR/scalable/status/openwhisper-tray-${icon}.svg"
    install -Dm644 "assets/icons/openwhisper-tray-${icon}.svg" "$ICON_DIR/symbolic/status/openwhisper-tray-${icon}.svg"
    install -Dm644 "assets/icons/openwhisper-tray-${icon}.svg" "$ICON_DIR/symbolic/status/openwhisper-tray-${icon}-symbolic.svg"
    install -Dm644 "assets/icons/openwhisper-tray-${icon}.svg" "$HOME/.local/share/pixmaps/openwhisper-tray-${icon}.svg"
done

# Render multi-resolution PNGs for the main application icon only
CONVERT_TOOL=""
if command -v magick >/dev/null 2>&1; then
    CONVERT_TOOL="magick"
elif command -v convert >/dev/null 2>&1; then
    CONVERT_TOOL="convert"
fi

if [ -n "$CONVERT_TOOL" ]; then
    for size in 16 22 24 32 48 64 128 256 512; do
        mkdir -p "$ICON_DIR/${size}x${size}/apps"
        $CONVERT_TOOL -background none assets/icons/openwhisper.svg -resize "${size}x${size}" "$ICON_DIR/${size}x${size}/apps/openwhisper.png" 2>/dev/null || true
    done
    cp "$ICON_DIR/256x256/apps/openwhisper.png" "$HOME/.local/share/pixmaps/openwhisper.png" 2>/dev/null || true
fi

# Update icon cache if tools are available
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
    gtk-update-icon-cache -q -t -f "$ICON_DIR" 2>/dev/null || true
fi
if command -v kbuildsycoca6 >/dev/null 2>&1; then
    kbuildsycoca6 --noincremental 2>/dev/null || true
fi

echo "==> Installing desktop entries..."
APP_DIR="$HOME/.local/share/applications"
mkdir -p "$APP_DIR" "$HOME/.config/autostart"
install -Dm644 desktop/net.local.openwhisper.desktop "$APP_DIR/net.local.openwhisper.desktop"
install -Dm644 desktop/net.local.openwhisper.settings.desktop "$APP_DIR/net.local.openwhisper.settings.desktop"
install -Dm644 desktop/net.local.openwhisper-ui.desktop "$APP_DIR/net.local.openwhisper-ui.desktop"
install -Dm644 desktop/net.local.openwhisper-ui.desktop "$HOME/.config/autostart/net.local.openwhisper-ui.desktop"

if command -v kbuildsycoca6 >/dev/null 2>&1; then
    kbuildsycoca6 --noincremental >/dev/null 2>&1 || true
elif command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database "$APP_DIR" 2>/dev/null || true
fi

echo "==> Installing systemd user services..."
SYSTEMD_USER_DIR="$HOME/.config/systemd/user"
mkdir -p "$SYSTEMD_USER_DIR"
install -Dm644 systemd/openwhisper.service "$SYSTEMD_USER_DIR/openwhisper.service"
install -Dm644 systemd/openwhisper-ui.service "$SYSTEMD_USER_DIR/openwhisper-ui.service"

echo "==> Reloading and restarting user services..."
if [ -d "/run/user/$(id -u)" ]; then
    export XDG_RUNTIME_DIR="/run/user/$(id -u)"
    export DBUS_SESSION_BUS_ADDRESS="unix:path=/run/user/$(id -u)/bus"
    systemctl --user daemon-reload 2>/dev/null || true
    systemctl --user enable openwhisper.service openwhisper-ui.service 2>/dev/null || true
    systemctl --user restart openwhisper.service openwhisper-ui.service 2>/dev/null || true
fi

echo "==> Installation complete! OpenWhisper is updated and running."
