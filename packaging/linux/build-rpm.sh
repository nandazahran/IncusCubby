#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Build a binary .rpm package for IncusCubby.
# Usage: packaging/linux/build-rpm.sh [ARCH] [BIN_PATH] [OUT_PATH]
set -euo pipefail

ARCH="${1:-$(uname -m)}"
case "$ARCH" in
    x86_64)
        RPM_ARCH="x86_64"
        ;;
    aarch64|arm64)
        RPM_ARCH="aarch64"
        ;;
    *)
        echo "Unsupported architecture: $ARCH (expected x86_64 or aarch64)" >&2
        exit 1
        ;;
esac

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' "$REPO_ROOT/Cargo.toml" | head -n1)"
BIN_PATH="${2:-$REPO_ROOT/target/release/incus-cubby}"
OUT_PATH="${3:-$REPO_ROOT/incus-cubby-${VERSION}-1.${RPM_ARCH}.rpm}"

if [ ! -f "$BIN_PATH" ]; then
    echo "Binary not found at $BIN_PATH; run 'cargo build --release' first." >&2
    exit 1
fi

ABS_BIN_PATH="$(cd "$(dirname "$BIN_PATH")" && pwd)/$(basename "$BIN_PATH")"
WORK_DIR="$(mktemp -d)"
trap 'rm -rf "$WORK_DIR"' EXIT

TOPDIR="$WORK_DIR/rpmbuild"
mkdir -p "$TOPDIR"/{BUILD,BUILDROOT,RPMS,SOURCES,SPECS,SRPMS}

SPEC_FILE="$TOPDIR/SPECS/incus-cubby.spec"
cat > "$SPEC_FILE" <<'EOF'
%global debug_package %{nil}
%global _missing_build_ids_terminate_build 0
%global _build_id_links none

Name:           incus-cubby
Version:        %{pkg_version}
Release:        1
Summary:        Native Linux desktop GUI for managing local Incus/LXC developer sandboxes
License:        GPL-3.0-or-later
URL:            https://github.com/nandazahran/IncusCubby
AutoReqProv:    no
Requires:       libfontconfig.so.1()(64bit)
Requires:       libfreetype.so.6()(64bit)
Requires:       libgcc_s.so.1()(64bit)
Requires:       libc.so.6()(64bit)
Recommends:     libxkbcommon.so.0()(64bit)
Suggests:       incus

%description
Direct Unix socket REST client and Slint desktop GUI for managing local Incus
containers, virtual machines, snapshots, mounts, storage, networks, and profiles.

%install
rm -rf %{buildroot}
install -Dm755 "%{bin_path}" "%{buildroot}%{_bindir}/incus-cubby"
install -Dm644 "%{repo_root}/packaging/linux/io.github.nandazahran.incuscubby.desktop" \
    "%{buildroot}%{_datadir}/applications/io.github.nandazahran.incuscubby.desktop"
install -Dm644 "%{repo_root}/packaging/linux/io.github.nandazahran.incuscubby.png" \
    "%{buildroot}%{_datadir}/icons/hicolor/512x512/apps/io.github.nandazahran.incuscubby.png"
install -Dm644 "%{repo_root}/ui/icons/incuscubby.svg" \
    "%{buildroot}%{_datadir}/icons/hicolor/scalable/apps/io.github.nandazahran.incuscubby.svg"
install -Dm644 "%{repo_root}/LICENSE" \
    "%{buildroot}%{_datadir}/licenses/incus-cubby/LICENSE"

%files
%{_bindir}/incus-cubby
%{_datadir}/applications/io.github.nandazahran.incuscubby.desktop
%{_datadir}/icons/hicolor/512x512/apps/io.github.nandazahran.incuscubby.png
%{_datadir}/icons/hicolor/scalable/apps/io.github.nandazahran.incuscubby.svg
%{_datadir}/licenses/incus-cubby/LICENSE
EOF

rpmbuild -bb --nodeps \
    --target "$RPM_ARCH" \
    --define "_topdir $TOPDIR" \
    --define "pkg_version $VERSION" \
    --define "bin_path $ABS_BIN_PATH" \
    --define "repo_root $REPO_ROOT" \
    "$SPEC_FILE"

RPM_OUT="$TOPDIR/RPMS/${RPM_ARCH}/incus-cubby-${VERSION}-1.${RPM_ARCH}.rpm"
if [ ! -f "$RPM_OUT" ]; then
    echo "Expected RPM not produced at $RPM_OUT" >&2
    exit 1
fi

cp "$RPM_OUT" "$OUT_PATH"
echo "Built $OUT_PATH"
