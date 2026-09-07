#!/usr/bin/env bash
set -euo pipefail

# OpenWhisper Setup & Installation Script
# Builds the release binary and deploys all desktop assets, icons, and systemd units.

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$SCRIPT_DIR"

echo "==> Building OpenWhisper release binary..."
export CARGO_HOME="${CARGO_HOME:-$HOME/.cache/puccinialin/cargo}"
export PATH="$HOME/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$CARGO_HOME/bin:$PATH"
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
mkdir -p "$ICON_DIR/scalable/apps" "$ICON_DIR/scalable/status"
install -Dm644 assets/icons/openwhisper.svg "$ICON_DIR/scalable/apps/openwhisper.svg"
install -Dm644 assets/icons/openwhisper-tray-idle.svg "$ICON_DIR/scalable/status/openwhisper-tray-idle.svg"
install -Dm644 assets/icons/openwhisper-tray-recording.svg "$ICON_DIR/scalable/status/openwhisper-tray-recording.svg"
install -Dm644 assets/icons/openwhisper-tray-transcribing.svg "$ICON_DIR/scalable/status/openwhisper-tray-transcribing.svg"
install -Dm644 assets/icons/openwhisper-tray-error.svg "$ICON_DIR/scalable/status/openwhisper-tray-error.svg"

# Update icon cache if tools are available
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
    gtk-update-icon-cache -q -t -f "$ICON_DIR" 2>/dev/null || true
fi

echo "==> Installing desktop entries..."
APP_DIR="$HOME/.local/share/applications"
mkdir -p "$APP_DIR"
install -Dm644 desktop/net.local.openwhisper.desktop "$APP_DIR/net.local.openwhisper.desktop"
install -Dm644 desktop/net.local.openwhisper.settings.desktop "$APP_DIR/net.local.openwhisper.settings.desktop"

if command -v kbuildsycoca6 >/dev/null 2>&1; then
    kbuildsycoca6 --noincremental >/dev/null 2>&1 || true
elif command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database "$APP_DIR" 2>/dev/null || true
fi

echo "==> Installing systemd user service..."
SYSTEMD_USER_DIR="$HOME/.config/systemd/user"
mkdir -p "$SYSTEMD_USER_DIR"
install -Dm644 systemd/openwhisper.service "$SYSTEMD_USER_DIR/openwhisper.service"

echo "==> Reloading and restarting openwhisper.service..."
if [ -d "/run/user/$(id -u)" ]; then
    export XDG_RUNTIME_DIR="/run/user/$(id -u)"
    export DBUS_SESSION_BUS_ADDRESS="unix:path=/run/user/$(id -u)/bus"
    systemctl --user daemon-reload 2>/dev/null || true
    systemctl --user enable openwhisper.service 2>/dev/null || true
    systemctl --user restart openwhisper.service 2>/dev/null || true
fi

echo "==> Installation complete! OpenWhisper is updated and running."
