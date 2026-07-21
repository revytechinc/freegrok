#!/bin/sh
# tui-regression.sh — isolated TUI smoke via PTY harness (not the operator home).
#
#   ./scripts/tui-regression.sh
#   PAGER_BINARY=/path/to/grok-build ./scripts/tui-regression.sh
#   SKIP_TUI=1 ./scripts/tui-regression.sh   # no-op pass for CLI-only hosts
#
# Runs a curated set of *ignored* scripted PTY scenarios with:
#   • temporary HOME + GROK_HOME (via harness ContentController)
#   • API keys / SSH agent stripped from the child (apply_child_env)
#   • mock inference server (no network LLM)
#
# Exit 0 on pass/skip; non-zero on failure.
set -eu

ROOT=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
cd "$ROOT"

export PATH="/usr/local/bin:${PATH}"
export PROTOC="${PROTOC:-$(command -v protoc 2>/dev/null || echo protoc)}"
export CARGO="${CARGO:-cargo}"

if [ "${SKIP_TUI:-0}" = "1" ]; then
	echo "SKIP tui-regression (SKIP_TUI=1)"
	exit 0
fi

# Need a TTY-capable host; PTY harness uses portable-pty (no real display).
if ! command -v "$CARGO" >/dev/null 2>&1; then
	echo "SKIP tui-regression (cargo missing)"
	exit 0
fi

REG="${REG_DIR:-/tmp/grok-tui-reg-$$}"
mkdir -p "$REG"
LOG="$REG/tui-regression.log"
: >"$LOG"

# Resolve pager binary: explicit → release target → build
if [ -n "${PAGER_BINARY:-}" ] && [ -x "$PAGER_BINARY" ]; then
	:
elif [ -x target/release/xai-grok-pager ]; then
	PAGER_BINARY=$(CDPATH= cd -- target/release && pwd)/xai-grok-pager
elif [ -x target/debug/xai-grok-pager ]; then
	PAGER_BINARY=$(CDPATH= cd -- target/debug && pwd)/xai-grok-pager
else
	echo "---- build xai-grok-pager (debug, for PTY) ----" | tee -a "$LOG"
	env PROTOC="$PROTOC" "$CARGO" build -p xai-grok-pager-bin --bin xai-grok-pager >>"$LOG" 2>&1
	PAGER_BINARY=$(CDPATH= cd -- target/debug && pwd)/xai-grok-pager
fi
export PAGER_BINARY
echo "PAGER_BINARY=$PAGER_BINARY" | tee -a "$LOG"
file "$PAGER_BINARY" >>"$LOG" 2>&1 || true

# Isolation for the *cargo test* process so even parent-side helpers don't
# read operator ~/.grok when resolving paths outside the harness.
# shellcheck disable=SC1091
. "$ROOT/scripts/lib/isolated-env.sh"
grok_isolation_init "$REG/sandbox"
grok_isolation_seed_fixtures
eval "$(grok_isolation_env_exports)"
grok_isolation_summary | tee -a "$LOG"

# Curated smoke: welcome screen boots, no panic (YAML under tests/scenarios).
# Additional scenarios can be listed via TUI_SCENARIOS="a b".
TESTS="${TUI_SCENARIOS:-scripted_welcome_screen}"

pass=0
fail=0
for t in $TESTS; do
	echo "---- TUI $t ----" | tee -a "$LOG"
	set +e
	env PROTOC="$PROTOC" PAGER_BINARY="$PAGER_BINARY" \
		HOME="$GROK_ISO_HOME" GROK_HOME="$GROK_ISO_GROK_HOME" \
		"$CARGO" test -p xai-grok-pager --test scripted_scenarios "$t" -- --ignored --nocapture \
		>>"$LOG" 2>&1
	ec=$?
	set -e
	if [ "$ec" -eq 0 ]; then
		echo "PASS tui:$t"
		pass=$((pass + 1))
	else
		echo "FAIL tui:$t (exit $ec) — see $LOG"
		tail -40 "$LOG" || true
		fail=$((fail + 1))
	fi
done

echo "tui-regression pass=$pass fail=$fail log=$LOG"
if [ "$fail" -ne 0 ]; then
	exit 1
fi
exit 0
