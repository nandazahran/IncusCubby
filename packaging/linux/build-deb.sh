#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Build a Debian/Ubuntu .deb package for IncusCubby.
# Usage: packaging/linux/build-deb.sh [ARCH] [BIN_PATH] [OUT_PATH]
set -euo pipefail

ARCH="${1:-$(uname -m)}"
case "$ARCH" in
    x86_64|amd64)
        DEB_ARCH="amd64"
        ;;
    aarch64|arm64)
        DEB_ARCH="arm64"
        ;;
    *)
        echo "Unsupported architecture: $ARCH (expected x86_64/amd64 or aarch64/arm64)" >&2
        exit 1
        ;;
esac

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' "$REPO_ROOT/Cargo.toml" | head -n1)"
BIN_PATH="${2:-$REPO_ROOT/target/release/incus-cubby}"
OUT_PATH="${3:-$REPO_ROOT/incus-cubby_${VERSION}_${DEB_ARCH}.deb}"

if [ ! -f "$BIN_PATH" ]; then
    echo "Binary not found at $BIN_PATH; run 'cargo build --release' first." >&2
    exit 1
fi

WORK_DIR="$(mktemp -d)"
trap 'rm -rf "$WORK_DIR"' EXIT

PKG_ROOT="$WORK_DIR/incus-cubby"
mkdir -p \
    "$PKG_ROOT/DEBIAN" \
    "$PKG_ROOT/usr/bin" \
    "$PKG_ROOT/usr/share/applications" \
    "$PKG_ROOT/usr/share/icons/hicolor/512x512/apps" \
    "$PKG_ROOT/usr/share/icons/hicolor/scalable/apps" \
    "$PKG_ROOT/usr/share/doc/incus-cubby"

install -Dm755 "$BIN_PATH" "$PKG_ROOT/usr/bin/incus-cubby"
install -Dm644 "$REPO_ROOT/packaging/linux/io.github.nandazahran.incuscubby.desktop" \
    "$PKG_ROOT/usr/share/applications/io.github.nandazahran.incuscubby.desktop"
install -Dm644 "$REPO_ROOT/packaging/linux/io.github.nandazahran.incuscubby.png" \
    "$PKG_ROOT/usr/share/icons/hicolor/512x512/apps/io.github.nandazahran.incuscubby.png"
install -Dm644 "$REPO_ROOT/ui/icons/incuscubby.svg" \
    "$PKG_ROOT/usr/share/icons/hicolor/scalable/apps/io.github.nandazahran.incuscubby.svg"
install -Dm644 "$REPO_ROOT/LICENSE" "$PKG_ROOT/usr/share/doc/incus-cubby/LICENSE"

cat > "$PKG_ROOT/usr/share/doc/incus-cubby/copyright" <<'EOF'
Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/
Upstream-Name: IncusCubby
Source: https://github.com/nandazahran/IncusCubby

Files: *
Copyright: 2026 Nanda Zahran
License: GPL-3.0-or-later
 On Debian systems, the full text of the GNU General Public License version 3
 can be found in `/usr/share/common-licenses/GPL-3' and in
 `/usr/share/doc/incus-cubby/LICENSE'.
EOF
chmod 0644 "$PKG_ROOT/usr/share/doc/incus-cubby/copyright"

INSTALLED_SIZE="$(du -sk "$PKG_ROOT" | awk '{print $1}')"

cat > "$PKG_ROOT/DEBIAN/control" <<EOF
Package: incus-cubby
Version: ${VERSION}
Section: utils
Priority: optional
Architecture: ${DEB_ARCH}
Maintainer: Nanda Zahran <https://github.com/nandazahran/IncusCubby>
Homepage: https://github.com/nandazahran/IncusCubby
Installed-Size: ${INSTALLED_SIZE}
Depends: libc6, libfontconfig1, libfreetype6, libgcc-s1, libxkbcommon0
Recommends: libwayland-client0, libx11-6, libegl1
Suggests: incus
Description: Native Linux desktop GUI for managing local Incus/LXC developer sandboxes
 Direct Unix socket REST client and Slint desktop GUI for managing local Incus
 containers, virtual machines, snapshots, mounts, storage, networks, and profiles.
EOF
chmod 0644 "$PKG_ROOT/DEBIAN/control"

dpkg-deb --root-owner-group --build "$PKG_ROOT" "$OUT_PATH"
echo "Built $OUT_PATH"
