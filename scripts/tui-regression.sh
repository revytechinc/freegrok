#!/bin/sh
# tui-regression.sh — isolated multi-scenario TUI suite (not the operator home).
#
#   ./scripts/tui-regression.sh                 # TUI_TIER=smoke (default)
#   TUI_TIER=standard ./scripts/tui-regression.sh
#   TUI_TIER=full ./scripts/tui-regression.sh
#   TUI_SCENARIO_FILES="welcome.yaml mock_response.yaml" ./scripts/tui-regression.sh
#   PAGER_BINARY=/path/to/grok-build ./scripts/tui-regression.sh
#   TUI_FAIL_FAST=1 ./scripts/tui-regression.sh
#   SKIP_TUI=1 ./scripts/tui-regression.sh      # no-op pass
#
# Isolation:
#   • synthetic HOME / USER / GROK_HOME / XDG_* under REG_DIR
#   • API keys + SSH agent unset in the cargo/test process
#   • PTY child strips secrets again (apply_child_env)
#   • mock inference server (no live LLM)
set -eu

ROOT=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
cd "$ROOT"

export PATH="/usr/local/bin:${PATH}"
export PROTOC="${PROTOC:-$(command -v protoc 2>/dev/null || echo protoc)}"
export CARGO="${CARGO:-cargo}"
export TUI_TIER="${TUI_TIER:-smoke}"

if [ "${SKIP_TUI:-0}" = "1" ]; then
	echo "SKIP tui-regression (SKIP_TUI=1)"
	exit 0
fi

if ! command -v "$CARGO" >/dev/null 2>&1; then
	echo "SKIP tui-regression (cargo missing)"
	exit 0
fi

REG="${REG_DIR:-/tmp/grok-tui-reg-$$}"
mkdir -p "$REG"
LOG="$REG/tui-regression.log"
: >"$LOG"

# Resolve pager binary: explicit → release → debug → build
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
echo "TUI_TIER=$TUI_TIER" | tee -a "$LOG"
if [ -n "${TUI_SCENARIO_FILES:-}" ]; then
	echo "TUI_SCENARIO_FILES=$TUI_SCENARIO_FILES" | tee -a "$LOG"
fi
file "$PAGER_BINARY" >>"$LOG" 2>&1 || true

# Isolation for the cargo test process.
# shellcheck disable=SC1091
. "$ROOT/scripts/lib/isolated-env.sh"
grok_isolation_init "$REG/sandbox"
grok_isolation_seed_fixtures
eval "$(grok_isolation_env_exports)"
grok_isolation_summary | tee -a "$LOG"

# Parse-only gate (no PTY) — fails fast on missing/broken YAML.
echo "---- parse suite YAMLs ----" | tee -a "$LOG"
set +e
env PROTOC="$PROTOC" \
	"$CARGO" test -p xai-grok-pager --test tui_regression_suite \
	tui_regression_scenario_files_parse -- --nocapture >>"$LOG" 2>&1
parse_ec=$?
set -e
if [ "$parse_ec" -ne 0 ]; then
	echo "FAIL tui:parse-suite"
	tail -40 "$LOG" || true
	exit 1
fi
echo "PASS tui:parse-suite"

# One cargo invocation runs the whole curated tier sequentially inside the test.
echo "---- TUI curated ($TUI_TIER) ----" | tee -a "$LOG"
set +e
env PROTOC="$PROTOC" PAGER_BINARY="$PAGER_BINARY" \
	HOME="$GROK_ISO_HOME" GROK_HOME="$GROK_ISO_GROK_HOME" \
	TUI_TIER="$TUI_TIER" \
	TUI_SCENARIO_FILES="${TUI_SCENARIO_FILES:-}" \
	TUI_FAIL_FAST="${TUI_FAIL_FAST:-0}" \
	"$CARGO" test -p xai-grok-pager --test tui_regression_suite \
	tui_regression_curated -- --ignored --nocapture >>"$LOG" 2>&1
ec=$?
set -e

if [ "$ec" -eq 0 ]; then
	echo "PASS tui:curated tier=$TUI_TIER"
	echo "tui-regression pass=1 fail=0 log=$LOG"
	exit 0
fi

echo "FAIL tui:curated tier=$TUI_TIER (exit $ec) — see $LOG"
# Extract scenario lines for a short summary
rg -n "tui_regression:|tui_regression FAIL|TUI regression failures" "$LOG" 2>/dev/null | tail -40 || true
tail -50 "$LOG" || true
echo "tui-regression pass=0 fail=1 log=$LOG"
exit 1
