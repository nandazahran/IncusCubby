#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Build a portable AppImage for IncusCubby.
# Usage: packaging/linux/build-appimage.sh [ARCH] [BIN_PATH] [OUT_PATH]
set -euo pipefail

ARCH="${1:-$(uname -m)}"
case "$ARCH" in
    x86_64|aarch64) ;;
    arm64) ARCH="aarch64" ;;
    *)
        echo "Unsupported architecture: $ARCH (expected x86_64 or aarch64)" >&2
        exit 1
        ;;
esac

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' "$REPO_ROOT/Cargo.toml" | head -n1)"
BIN_PATH="${2:-$REPO_ROOT/target/release/incus-cubby}"
OUT_PATH="${3:-$REPO_ROOT/incus-cubby-${VERSION}-${ARCH}.AppImage}"

if [ ! -f "$BIN_PATH" ]; then
    echo "Binary not found at $BIN_PATH; run 'cargo build --release' first." >&2
    exit 1
fi

WORK_DIR="$(mktemp -d)"
trap 'rm -rf "$WORK_DIR"' EXIT

APPDIR="$WORK_DIR/IncusCubby.AppDir"
mkdir -p \
    "$APPDIR/usr/bin" \
    "$APPDIR/usr/share/applications" \
    "$APPDIR/usr/share/icons/hicolor/512x512/apps" \
    "$APPDIR/usr/share/icons/hicolor/scalable/apps" \
    "$APPDIR/usr/share/licenses/incus-cubby"

install -Dm755 "$BIN_PATH" "$APPDIR/usr/bin/incus-cubby"
install -Dm644 "$REPO_ROOT/packaging/linux/io.github.nandazahran.incuscubby.desktop" \
    "$APPDIR/usr/share/applications/io.github.nandazahran.incuscubby.desktop"
install -Dm644 "$REPO_ROOT/packaging/linux/io.github.nandazahran.incuscubby.desktop" \
    "$APPDIR/io.github.nandazahran.incuscubby.desktop"
install -Dm644 "$REPO_ROOT/packaging/linux/io.github.nandazahran.incuscubby.png" \
    "$APPDIR/usr/share/icons/hicolor/512x512/apps/io.github.nandazahran.incuscubby.png"
install -Dm644 "$REPO_ROOT/ui/icons/incuscubby.svg" \
    "$APPDIR/usr/share/icons/hicolor/scalable/apps/io.github.nandazahran.incuscubby.svg"
install -Dm644 "$REPO_ROOT/packaging/linux/io.github.nandazahran.incuscubby.png" \
    "$APPDIR/io.github.nandazahran.incuscubby.png"
ln -sf io.github.nandazahran.incuscubby.png "$APPDIR/.DirIcon"
install -Dm644 "$REPO_ROOT/LICENSE" "$APPDIR/usr/share/licenses/incus-cubby/LICENSE"

cat > "$APPDIR/AppRun" <<'EOF'
#!/bin/sh
HERE="$(dirname "$(readlink -f "$0")")"
export PATH="${HERE}/usr/bin:${PATH}"
exec "${HERE}/usr/bin/incus-cubby" "$@"
EOF
chmod 0755 "$APPDIR/AppRun"

if command -v appimagetool >/dev/null 2>&1; then
    APPIMAGETOOL="appimagetool"
else
    APPIMAGETOOL="$WORK_DIR/appimagetool-${ARCH}.AppImage"
    curl -fsSL \
        "https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-${ARCH}.AppImage" \
        -o "$APPIMAGETOOL"
    chmod +x "$APPIMAGETOOL"
fi

ARCH="$ARCH" VERSION="$VERSION" APPIMAGE_EXTRACT_AND_RUN=1 \
    "$APPIMAGETOOL" --no-appstream "$APPDIR" "$OUT_PATH"
chmod +x "$OUT_PATH"
echo "Built $OUT_PATH"
