#!/bin/sh
# FreeBSD full-ish regression + probe harness for FreeGrok.
# Run on FreeBSD (e.g. freedev007) from the repo root:
#   ./scripts/freebsd-regression.sh
# Optional: PACKAGES="xai-grok-sandbox xai-grok-tools" ./scripts/freebsd-regression.sh
#
# After a successful release build, runs:
#   target/release/freegrok doctor --ci
# (offline product gate; see docs/user-guide/25-doctor.md)
#
# For regression against a *make install* binary (recommended):
#   ./scripts/freebsd-installed-regression.sh
set -eu

ROOT=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
cd "$ROOT"

export PATH="/usr/local/bin:${PATH}"
export PROTOC="${PROTOC:-$(command -v protoc || true)}"
unset CFLAGS || true
export CARGO_TERM_COLOR=never
# ACP auth-retry tokio tests overflow the default thread stack (Linux abort /
# FreeBSD SIGSEGV). 32 MiB is enough for the current_thread mock loop.
export RUST_MIN_STACK="${RUST_MIN_STACK:-33554432}"

LOG_DIR="${LOG_DIR:-/tmp/grok-freebsd-reg}"
mkdir -p "$LOG_DIR"
LOG="$LOG_DIR/regression.log"
SUMMARY="$LOG_DIR/summary.txt"
PROBE="$LOG_DIR/probe.txt"
: >"$LOG"
: >"$SUMMARY"
: >"$PROBE"

ts() { date -u +%Y-%m-%dT%H:%M:%SZ; }

echo "START $(ts)" | tee -a "$SUMMARY"
echo "host: $(uname -srm)" | tee -a "$SUMMARY"
echo "rustc: $(rustc --version 2>/dev/null || echo missing)" | tee -a "$SUMMARY"
echo "protoc: $(protoc --version 2>/dev/null || echo missing)" | tee -a "$SUMMARY"
echo "rg: $(rg --version 2>/dev/null | head -1 || echo missing)" | tee -a "$SUMMARY"
echo "cwd: $ROOT" | tee -a "$SUMMARY"
echo "git: $(git rev-parse --short HEAD 2>/dev/null) $(git branch --show-current 2>/dev/null)" | tee -a "$SUMMARY"

# --- probes ---
{
  echo "=== probe $(ts) ==="
  uname -a
  sysctl security.jail.jailed 2>/dev/null || true
  sysctl security.jail.enable 2>/dev/null || true
  if [ -x target/release/freegrok ]; then
    BIN=target/release/freegrok
  elif [ -x target/debug/freegrok ]; then
    BIN=target/debug/freegrok
  else
    BIN=
  fi
  if [ -n "$BIN" ]; then
    echo "binary: $BIN"
    "$BIN" --version || true
    "$BIN" --help 2>&1 | head -25 || true
    ldd "$BIN" 2>&1 | head -30 || true
  else
    echo "binary: not built yet"
  fi
  echo "JAIL_ENV=__GROK_INSIDE_JAIL=${__GROK_INSIDE_JAIL:-unset}"
} | tee -a "$PROBE"

DEFAULT_PKGS="
xai-grok-sandbox
xai-grok-config
xai-file-utils
xai-grok-telemetry
xai-grok-secrets
xai-grok-env
xai-tty-utils
xai-grok-shared
xai-grok-shell-base
xai-grok-tools
xai-grok-tools-api
xai-grok-memory
xai-grok-hooks
xai-grok-mcp
xai-fast-worktree
xai-fsnotify
xai-codebase-graph
xai-grok-markdown
xai-grok-markdown-core
xai-grok-mermaid
xai-grok-pager-render
xai-grok-pager-minimal
xai-grok-pager
xai-grok-shell
xai-grok-sampler
xai-grok-agent
xai-chat-state
xai-grok-workspace
xai-crash-handler
xai-system-power
xai-sqlite-journal
xai-hunk-tracker
xai-gix-status
xai-token-estimation
xai-circuit-breaker
xai-tool-types
xai-tool-protocol
xai-tool-runtime
xai-interjection-core
xai-grok-compaction
xai-prompt-queue
xai-acp-lib
xai-agent-lifecycle
xai-ratatui-inline
xai-ratatui-textarea
ptyctl
xai-grok-pager-bin
"

PKGS=${PACKAGES:-$DEFAULT_PKGS}

pass=0
fail=0

for p in $PKGS; do
  echo "==== TEST $p $(ts) ====" | tee -a "$LOG"
  if cargo test -p "$p" -- --test-threads="${TEST_THREADS:-4}" >>"$LOG" 2>&1; then
    echo "PASS $p" | tee -a "$SUMMARY"
    pass=$((pass + 1))
  else
    echo "FAIL $p" | tee -a "$SUMMARY"
    fail=$((fail + 1))
    tail -50 "$LOG" >>"$SUMMARY"
  fi
done

# Always attempt release build + smoke if pager-bin in list or always
echo "==== RELEASE pager-bin $(ts) ====" | tee -a "$LOG"
if cargo build -p xai-grok-pager-bin --release >>"$LOG" 2>&1; then
  echo "PASS release xai-grok-pager-bin" | tee -a "$SUMMARY"
  target/release/freegrok --version | tee -a "$SUMMARY" || true
else
  echo "FAIL release xai-grok-pager-bin" | tee -a "$SUMMARY"
  fail=$((fail + 1))
  tail -60 "$LOG" >>"$SUMMARY"
fi

# Product doctor CI gate (offline, critical-only, 15s wall budget — no stall)
echo "==== DOCTOR --ci $(ts) ====" | tee -a "$LOG"
if [ -x target/release/freegrok ]; then
  if target/release/freegrok doctor --ci --json >>"$LOG" 2>&1; then
    echo "PASS doctor --ci" | tee -a "$SUMMARY"
    pass=$((pass + 1))
  else
    echo "FAIL doctor --ci" | tee -a "$SUMMARY"
    fail=$((fail + 1))
    tail -40 "$LOG" >>"$SUMMARY"
  fi
else
  echo "FAIL doctor --ci (no release binary)" | tee -a "$SUMMARY"
  fail=$((fail + 1))
fi

# Sandbox smoke example (no kernel nono on FreeBSD; should not crash)
echo "==== EXAMPLE sandbox_smoke_test $(ts) ====" | tee -a "$LOG"
if cargo run -p xai-grok-sandbox --example sandbox_smoke_test -- workspace >>"$LOG" 2>&1; then
  echo "PASS sandbox_smoke_test" | tee -a "$SUMMARY"
else
  echo "FAIL sandbox_smoke_test" | tee -a "$SUMMARY"
  fail=$((fail + 1))
fi

echo "END $(ts) pass=$pass fail=$fail" | tee -a "$SUMMARY"
echo "logs: $LOG_DIR"
exit "$fail"
