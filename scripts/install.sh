#!/usr/bin/env bash
# Copyright 2026 Kitefold Labs
# SPDX-License-Identifier: Apache-2.0
# Install Masthead from a built tree.
#
#   scripts/install.sh                     # into ~/.local for the current user
#   PREFIX=/usr DESTDIR=pkg scripts/install.sh   # staged system install (packaging)
#   scripts/install.sh --uninstall         # remove what a user install added
#
# BIN is the directory holding the release binaries (default target/release).
set -euo pipefail

BIN=${BIN:-target/release}
DESTDIR=${DESTDIR:-}
if [ -z "${PREFIX:-}" ]; then
    PREFIX="$HOME/.local"
    USER_INSTALL=1
else
    USER_INSTALL=0
fi
bindir="$PREFIX/bin"
datadir="$PREFIX/share"
if [ "$USER_INSTALL" = 1 ]; then
    systemd_user="$HOME/.config/systemd/user"
    autostart="${XDG_CONFIG_HOME:-$HOME/.config}/autostart"
else
    systemd_user="$PREFIX/lib/systemd/user"
    if [ "$PREFIX" = /usr ]; then
        autostart=/etc/xdg/autostart
    else
        autostart="$PREFIX/etc/xdg/autostart"
    fi
fi
ext_uuid="masthead@kitefoldlabs.com"

files=(
    "$bindir/masthead-daemon"
    "$bindir/masthead-window"
    "$bindir/masthead-bar"
    "$bindir/masthead-cosmic"
    "$datadir/dbus-1/services/com.kitefoldlabs.Masthead1.service"
    "$datadir/dbus-1/services/com.kitefoldlabs.Masthead.Window.service"
    "$systemd_user/masthead.service"
    "$autostart/com.kitefoldlabs.Masthead.Autostart.desktop"
    "$datadir/applications/com.kitefoldlabs.Masthead.desktop"
    "$datadir/applications/com.kitefoldlabs.Masthead.CosmicApplet.desktop"
    "$datadir/icons/hicolor/scalable/apps/com.kitefoldlabs.Masthead.CosmicApplet-symbolic.svg"
    "$datadir/icons/hicolor/scalable/apps/com.kitefoldlabs.Masthead.svg"
    "$datadir/icons/hicolor/symbolic/apps/com.kitefoldlabs.Masthead-symbolic.svg"
    "$datadir/metainfo/com.kitefoldlabs.Masthead.metainfo.xml"
)
# Metric icons, shared by the GNOME extension and the tray fallback.
metrics=(cpu mem gpu net disk temp battery)
for m in "${metrics[@]}"; do
    files+=("$datadir/icons/hicolor/symbolic/apps/com.kitefoldlabs.Masthead-$m-symbolic.svg")
done

if [ "${1:-}" = "--uninstall" ]; then
    if [ "$USER_INSTALL" = 1 ] && [ -z "$DESTDIR" ]; then
        systemctl --user stop masthead.service 2>/dev/null || true
        pkill -x masthead-window 2>/dev/null || true
    fi
    for f in "${files[@]}"; do rm -f "$DESTDIR$f"; done
    rm -rf "$DESTDIR$datadir/gnome-shell/extensions/$ext_uuid"
    if [ "$USER_INSTALL" = 1 ] && [ -z "$DESTDIR" ]; then
        systemctl --user daemon-reload 2>/dev/null || true
    fi
    echo "Removed Masthead from $PREFIX."
    exit 0
fi

subst() { sed "s|@bindir@|$bindir|g" "$1"; }
put() { # put SRC DEST [MODE]
    install -D -m "${3:-644}" "$1" "$DESTDIR$2"
}

for b in masthead-daemon masthead-window masthead-bar masthead-cosmic; do
    if [ -x "$BIN/$b" ]; then
        put "$BIN/$b" "$bindir/$b" 755
    else
        echo "note: $BIN/$b not built; skipping" >&2
    fi
done

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
subst data/dbus/com.kitefoldlabs.Masthead1.service >"$tmp/a"
subst data/dbus/com.kitefoldlabs.Masthead.Window.service >"$tmp/b"
subst data/systemd/masthead.service >"$tmp/c"
put "$tmp/a" "$datadir/dbus-1/services/com.kitefoldlabs.Masthead1.service"
put "$tmp/b" "$datadir/dbus-1/services/com.kitefoldlabs.Masthead.Window.service"
put "$tmp/c" "$systemd_user/masthead.service"
subst data/com.kitefoldlabs.Masthead.desktop >"$tmp/e"
put "$tmp/e" "$datadir/applications/com.kitefoldlabs.Masthead.desktop"
# Turning off the login item in the desktop's settings writes Hidden=true into
# this file; a reinstall keeps that choice.
auto="$DESTDIR$autostart/com.kitefoldlabs.Masthead.Autostart.desktop"
if ! grep -qx 'Hidden=true' "$auto" 2>/dev/null; then
    subst data/com.kitefoldlabs.Masthead.Autostart.desktop >"$tmp/f"
    put "$tmp/f" "$autostart/com.kitefoldlabs.Masthead.Autostart.desktop"
fi
if [ -f crates/masthead-cosmic/data/com.kitefoldlabs.Masthead.CosmicApplet.desktop ]; then
    subst crates/masthead-cosmic/data/com.kitefoldlabs.Masthead.CosmicApplet.desktop >"$tmp/d"
    put "$tmp/d" "$datadir/applications/com.kitefoldlabs.Masthead.CosmicApplet.desktop"
    put crates/masthead-cosmic/data/icons/scalable/apps/com.kitefoldlabs.Masthead.CosmicApplet-symbolic.svg "$datadir/icons/hicolor/scalable/apps/com.kitefoldlabs.Masthead.CosmicApplet-symbolic.svg"
fi
put data/icons/hicolor/scalable/apps/com.kitefoldlabs.Masthead.svg "$datadir/icons/hicolor/scalable/apps/com.kitefoldlabs.Masthead.svg"
put data/icons/hicolor/symbolic/apps/com.kitefoldlabs.Masthead-symbolic.svg "$datadir/icons/hicolor/symbolic/apps/com.kitefoldlabs.Masthead-symbolic.svg"
put data/com.kitefoldlabs.Masthead.metainfo.xml "$datadir/metainfo/com.kitefoldlabs.Masthead.metainfo.xml"
for m in "${metrics[@]}"; do
    put "shell/gnome/icons/masthead-$m-symbolic.svg" "$datadir/icons/hicolor/symbolic/apps/com.kitefoldlabs.Masthead-$m-symbolic.svg"
done

# A digest of the installed extension, to tell whether this install changed it.
ext_digest() {
    [ -d "$1" ] || return 0
    (cd "$1" && find . -type f -print0 | sort -z | xargs -0 sha256sum) | sha256sum | cut -d' ' -f1
}

ext_changed=0
if [ -f "shell/gnome/metadata.json" ]; then
    ext="$DESTDIR$datadir/gnome-shell/extensions/$ext_uuid"
    ext_before=$(ext_digest "$ext")
    rm -rf "$ext"
    mkdir -p "$ext"
    (cd shell/gnome && for f in *.js metadata.json stylesheet.css icons/*.svg; do
        install -D -m 644 "$f" "$ext/$f"
    done)
    [ "$ext_before" = "$(ext_digest "$ext")" ] || ext_changed=1
fi

if [ "$USER_INSTALL" = 1 ] && [ -z "$DESTDIR" ]; then
    systemctl --user daemon-reload 2>/dev/null || true
    echo "Installed into $PREFIX."
    # Programs already running keep the old code until they restart. Faces
    # reconnect to a restarted daemon on their own.
    if systemctl --user is-active --quiet masthead.service 2>/dev/null; then
        systemctl --user restart masthead.service && echo "Restarted the daemon."
    fi
    if pkill -x masthead-window 2>/dev/null; then
        echo "Closed the detail window; open it again to get the new version."
    fi
    if pgrep -x masthead-cosmic >/dev/null 2>&1; then
        echo "COSMIC: the panel applet keeps the old version until you log out and back in."
    fi
    if pgrep -x gnome-shell >/dev/null 2>&1; then
        enabled=$(gsettings get org.gnome.shell enabled-extensions 2>/dev/null || true)
        if [[ "$enabled" != *"'$ext_uuid'"* ]]; then
            echo "GNOME: log out and back in, then run: gnome-extensions enable $ext_uuid"
        elif [ "$ext_changed" = 1 ]; then
            # GNOME Shell on Wayland loads extension code only at login.
            echo "GNOME: log out and back in to load the updated extension."
        fi
    fi
fi
