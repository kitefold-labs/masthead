#!/usr/bin/env bash
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
else
    systemd_user="$PREFIX/lib/systemd/user"
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
    "$datadir/applications/com.kitefoldlabs.Masthead.desktop"
    "$datadir/applications/com.kitefoldlabs.Masthead.CosmicApplet.desktop"
    "$datadir/icons/hicolor/scalable/apps/com.kitefoldlabs.Masthead.CosmicApplet-symbolic.svg"
    "$datadir/icons/hicolor/scalable/apps/com.kitefoldlabs.Masthead.svg"
    "$datadir/icons/hicolor/symbolic/apps/com.kitefoldlabs.Masthead-symbolic.svg"
    "$datadir/metainfo/com.kitefoldlabs.Masthead.metainfo.xml"
)

if [ "${1:-}" = "--uninstall" ]; then
    for f in "${files[@]}"; do rm -f "$DESTDIR$f"; done
    rm -rf "$DESTDIR$datadir/gnome-shell/extensions/$ext_uuid"
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
put data/com.kitefoldlabs.Masthead.desktop "$datadir/applications/com.kitefoldlabs.Masthead.desktop"
if [ -f crates/masthead-cosmic/data/com.kitefoldlabs.Masthead.CosmicApplet.desktop ]; then
    subst crates/masthead-cosmic/data/com.kitefoldlabs.Masthead.CosmicApplet.desktop >"$tmp/d"
    put "$tmp/d" "$datadir/applications/com.kitefoldlabs.Masthead.CosmicApplet.desktop"
    put crates/masthead-cosmic/data/icons/scalable/apps/com.kitefoldlabs.Masthead.CosmicApplet-symbolic.svg "$datadir/icons/hicolor/scalable/apps/com.kitefoldlabs.Masthead.CosmicApplet-symbolic.svg"
fi
put data/icons/hicolor/scalable/apps/com.kitefoldlabs.Masthead.svg "$datadir/icons/hicolor/scalable/apps/com.kitefoldlabs.Masthead.svg"
put data/icons/hicolor/symbolic/apps/com.kitefoldlabs.Masthead-symbolic.svg "$datadir/icons/hicolor/symbolic/apps/com.kitefoldlabs.Masthead-symbolic.svg"
put data/com.kitefoldlabs.Masthead.metainfo.xml "$datadir/metainfo/com.kitefoldlabs.Masthead.metainfo.xml"

if [ -f "shell/gnome/metadata.json" ]; then
    ext="$DESTDIR$datadir/gnome-shell/extensions/$ext_uuid"
    rm -rf "$ext"
    mkdir -p "$ext"
    (cd shell/gnome && for f in *.js metadata.json stylesheet.css icons/*.svg; do
        install -D -m 644 "$f" "$ext/$f"
    done)
fi

if [ "$USER_INSTALL" = 1 ] && [ -z "$DESTDIR" ]; then
    systemctl --user daemon-reload 2>/dev/null || true
    echo "Installed into $PREFIX."
    echo "GNOME: log out and back in, then run: gnome-extensions enable $ext_uuid"
fi
