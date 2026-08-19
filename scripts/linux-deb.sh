#!/bin/sh
# Build a FreeGrok .deb from the current checkout (Ubuntu/Debian).
#
#   ./scripts/linux-deb.sh
#   sudo dpkg -i dist/freegrok_VERSION_ARCH.deb
#
# Native arch only (run on amd64 for amd64, on aarch64 for arm64).
set -eu

ROOT=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
cd "$ROOT"

VERSION=$(grep -m1 '^version =' crates/codegen/xai-grok-pager-bin/Cargo.toml | sed 's/.*"\(.*\)"/\1/')
ARCH=$(dpkg --print-architecture 2>/dev/null || uname -m)
case "$ARCH" in
	x86_64) ARCH=amd64 ;;
	aarch64) ARCH=arm64 ;;
esac

export PROTOC="${PROTOC:-$(command -v protoc 2>/dev/null || echo protoc)}"
CARGO="${CARGO:-cargo}"

echo "==> cargo build -p xai-grok-pager-bin --release ($ARCH)"
$CARGO build -p xai-grok-pager-bin --release --locked
test -x target/release/freegrok

STAGE=$(mktemp -d)
trap 'rm -rf "$STAGE"' EXIT
DESTDIR="$STAGE" make install PREFIX=/usr

# Debian control
mkdir -p "$STAGE/DEBIAN"
cat >"$STAGE/DEBIAN/control" <<EOF
Package: freegrok
Version: $VERSION
Section: devel
Priority: optional
Architecture: $ARCH
Maintainer: Mark LaPointe <mark@cloudbsd.org>
Depends: ripgrep
Description: FreeGrok CLI (CloudBSD/RevyTech fork of Grok Build)
 Terminal AI coding agent. Copies ~/.grok configs into ~/.freegrok
 on first run. Not an official xAI package.
Homepage: https://github.com/revytechinc/freegrok
EOF

OUTDIR="$ROOT/dist"
mkdir -p "$OUTDIR"
DEB="$OUTDIR/freegrok_${VERSION}_${ARCH}.deb"
rm -f "$DEB"
dpkg-deb --build --root-owner-group "$STAGE" "$DEB"
echo "==> $DEB"
sha256sum "$DEB" 2>/dev/null || sha256 "$DEB"
ls -lh "$DEB"
