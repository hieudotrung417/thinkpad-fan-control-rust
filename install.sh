#!/bin/sh
set -eu

if [ "$(id -u)" -eq 0 ]; then
    echo "Run this installer as your normal user: ./install.sh" >&2
    echo "It will request sudo only when installing the built files." >&2
    exit 1
fi

SOURCE_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)

if ! command -v cargo >/dev/null 2>&1; then
    echo "Rust 1.85 or newer is required. Install it from https://rustup.rs and retry." >&2
    exit 1
fi

if ! pkg-config --exists gtk+-3.0 2>/dev/null || \
   ! ldconfig -p 2>/dev/null | grep -q 'libayatana-appindicator3'; then
    if command -v apt-get >/dev/null 2>&1; then
        echo "Installing GTK build dependencies..."
        sudo apt-get update
        sudo apt-get install -y pkg-config libgtk-3-dev libayatana-appindicator3-dev
    else
        echo "Cargo, GTK 3 development files, and Ayatana AppIndicator are required." >&2
        exit 1
    fi
fi

cd "$SOURCE_DIR"
cargo build --release --locked

if command -v pkill >/dev/null 2>&1; then
    sudo pkill -TERM -f '^/usr/local/bin/fanspeed([[:space:]]|$)' 2>/dev/null || true
    sudo pkill -TERM -f \
        '^/usr/bin/python3 /usr/local/lib/fanspeed/fanspeed_app.py([[:space:]]|$)' \
        2>/dev/null || true
fi

sudo install -d -m 755 /usr/local/bin /usr/local/libexec /usr/local/sbin
sudo install -d -m 755 /usr/local/share/applications
sudo install -d -m 755 /usr/local/share/icons/hicolor/scalable/apps
sudo install -d -m 755 /usr/share/polkit-1/actions

sudo install -m 755 target/release/fanspeed /usr/local/bin/fanspeed
sudo install -m 755 target/release/fanspeed-helper /usr/local/libexec/fanspeed-helper
sudo install -m 755 uninstall.sh /usr/local/sbin/fanspeed-uninstall
sudo install -m 644 io.github.fanspeed.FanSpeed.desktop \
    /usr/local/share/applications/io.github.fanspeed.FanSpeed.desktop
sudo install -m 644 io.github.fanspeed.FanSpeed.svg \
    /usr/local/share/icons/hicolor/scalable/apps/io.github.fanspeed.FanSpeed.svg
sudo install -m 644 io.github.fanspeed.policy \
    /usr/share/polkit-1/actions/io.github.fanspeed.policy

# Remove the one obsolete file left by an earlier Python installation.
sudo rm -f /usr/local/lib/fanspeed/fanspeed_app.py
sudo rmdir /usr/local/lib/fanspeed 2>/dev/null || true

if command -v update-desktop-database >/dev/null 2>&1; then
    sudo update-desktop-database /usr/local/share/applications >/dev/null 2>&1 || true
fi
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
    sudo gtk-update-icon-cache -q -t -f /usr/local/share/icons/hicolor >/dev/null 2>&1 || true
fi

echo "Fan Speed Control installed. Open it from the app launcher or run: fanspeed"
echo "To remove it later, run: sudo fanspeed-uninstall"

