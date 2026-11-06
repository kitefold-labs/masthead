#!/usr/bin/env bash
# Build a .deb from release binaries: scripts/package-deb.sh [OUTDIR]
set -euo pipefail

out=${1:-dist}
version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
arch=$(dpkg --print-architecture)
stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT

PREFIX=/usr DESTDIR="$stage" scripts/install.sh
install -D -m 644 LICENSE "$stage/usr/share/doc/masthead/copyright"

size=$(du -sk "$stage" | cut -f1)
mkdir -p "$stage/DEBIAN"
cat >"$stage/DEBIAN/control" <<CONTROL
Package: masthead
Version: $version
Architecture: $arch
Maintainer: Kitefold Labs <oss@kitefoldlabs.com>
Installed-Size: $size
Depends: libc6, dbus-user-session | dbus-x11
Recommends: gnome-shell (>= 45) | cosmic-panel | waybar
Section: utils
Priority: optional
Homepage: https://github.com/kitefold-labs/masthead
Description: system statistics in the top bar
 Masthead shows CPU, memory, GPU, network, disk and temperature readings in
 the desktop's top bar, with detail on hover and history graphs on click.
 At rest it uses under 0.1% of one CPU core and writes nothing to disk.
CONTROL

mkdir -p "$out"
dpkg-deb --root-owner-group --build "$stage" "$out/masthead_${version}_${arch}.deb"
