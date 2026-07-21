#!/bin/sh
# Install local git hooks for this clone (does not affect other clones).
set -eu
ROOT=$(CDPATH= cd -- "$(dirname "$0")/../.." && pwd)
cd "$ROOT"
HOOK_DIR="$ROOT/.git/hooks"
if [ ! -d "$HOOK_DIR" ]; then
	echo "error: $HOOK_DIR missing (not a git checkout?)" >&2
	exit 1
fi
src="$ROOT/scripts/git-hooks/pre-commit"
dst="$HOOK_DIR/pre-commit"
if [ ! -f "$src" ]; then
	echo "error: missing $src" >&2
	exit 1
fi
ln -sf ../../scripts/git-hooks/pre-commit "$dst"
chmod +x "$src" "$ROOT/scripts/check-pii.sh" 2>/dev/null || true
echo "Installed pre-commit → $dst"
echo "Runs: scripts/check-pii.sh --staged on every commit"
echo "Bypass: git commit --no-verify  (not recommended)"
