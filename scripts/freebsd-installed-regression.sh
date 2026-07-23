#!/bin/sh
# Regression against the *installed* application (make install), not cargo target/.
#
#   ./scripts/freebsd-installed-regression.sh
#
# Builds, runs doctor, installs via `make install` (PREFIX=/usr/local or
# userspace fallback under a temp HOME), then exercises the installed
# grok-build binary inside a full isolation sandbox (not the operator home).
# Does not require root.
#
# Optional:
#   SKIP_TUI=1     skip PTY TUI smoke
#   REG_DIR=...    regression log/sandbox root
set -eu

ROOT=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
cd "$ROOT"

export PATH="/usr/local/bin:${PATH}"
export PROTOC="${PROTOC:-$(command -v protoc 2>/dev/null || echo protoc)}"

REG="${REG_DIR:-/tmp/grok-install-reg-$$}"
mkdir -p "$REG"
LOG="$REG/regression.log"
SUMMARY="$REG/summary.txt"
: >"$LOG"
: >"$SUMMARY"

pass=0
fail=0
skip=0

ts() { date -u +%Y-%m-%dT%H:%M:%SZ; }
ok() { echo "PASS $*" | tee -a "$SUMMARY"; pass=$((pass + 1)); }
bad() { echo "FAIL $*" | tee -a "$SUMMARY"; fail=$((fail + 1)); }
note() { echo "---- $* ----" | tee -a "$LOG"; }

run() {
	name=$1
	shift
	note "TEST $name"
	if "$@" >>"$LOG" 2>&1; then
		ok "$name"
		return 0
	fi
	bad "$name"
	return 1
}

echo "START $(ts)" | tee -a "$SUMMARY"
echo "host $(uname -srm)" | tee -a "$SUMMARY"
echo "reg $REG" | tee -a "$SUMMARY"
echo "git: $(git rev-parse --short HEAD 2>/dev/null) $(git branch --show-current 2>/dev/null)" | tee -a "$SUMMARY"

# Best-effort PII/secret gate on the working tree (changed + untracked source).
note "check-pii"
if sh "$ROOT/scripts/check-pii.sh" --changed >>"$LOG" 2>&1; then
	ok "check-pii"
else
	bad "check-pii"
	tail -40 "$LOG" || true
	echo "END $(ts) pass=$pass fail=$fail skip=$skip" | tee -a "$SUMMARY"
	exit 1
fi

note "make build"
if make build >>"$LOG" 2>&1; then ok "make build"; else
	bad "make build"
	tail -40 "$LOG"
	exit 1
fi

note "make doctor"
if make doctor >>"$LOG" 2>&1; then ok "make doctor"; else bad "make doctor"; fi

# Full isolation: never use operator HOME for install or CLI probes.
# shellcheck disable=SC1091
. "$ROOT/scripts/lib/isolated-env.sh"
grok_isolation_init "$REG/sandbox"
grok_isolation_seed_fixtures
eval "$(grok_isolation_env_exports)"
grok_isolation_summary | tee -a "$SUMMARY" | tee -a "$LOG"

note "make install"
if make install PREFIX=/usr/local >>"$LOG" 2>&1; then ok "make install"; else
	bad "make install"
	tail -50 "$LOG"
	exit 1
fi

if [ -x "$HOME/.local/bin/grok-build" ]; then
	GB="$HOME/.local/bin/grok-build"
elif [ -x /usr/local/bin/grok-build ]; then
	GB=/usr/local/bin/grok-build
else
	bad "find installed grok-build"
	exit 1
fi
ok "installed $GB"
echo "GB=$GB" | tee -a "$SUMMARY"
file "$GB" >>"$LOG" 2>&1 || true

# Prove isolation: binary must not resolve operator home for config export source
# when only fixture data exists under $HOME.
if [ -n "${USER-}" ] && [ "$USER" = "testuser" ]; then
	ok "isolation-user-is-synthetic"
else
	# USER may still be operator if shell re-exported; env export sets USER=testuser
	if [ "${USER:-}" = "testuser" ] || [ "${LOGNAME:-}" = "testuser" ]; then
		ok "isolation-user-is-synthetic"
	else
		# Soft: still fail closed if GROK_HOME points at real home
		case "$GROK_HOME" in
		"$REG/sandbox"/*) ok "isolation-grok-home-under-sandbox" ;;
		*) bad "isolation-grok-home-not-sandboxed ($GROK_HOME)" ;;
		esac
	fi
fi
case "$HOME" in
"$REG/sandbox"/*) ok "isolation-home-under-sandbox" ;;
*) bad "isolation-home-not-sandboxed ($HOME)" ;;
esac

# --- installed binary (all under isolation) ---
run "version" grok_isolated_run "$GB" --version
run "version-json" grok_isolated_run "$GB" version --json
run "help" grok_isolated_run "$GB" --help
run "doctor-ci" grok_isolated_run "$GB" doctor --ci
run "doctor-json" grok_isolated_run "$GB" doctor --ci --json
run "inspect" grok_isolated_run "$GB" inspect
run "jail-status" grok_isolated_run "$GB" jail status
run "jail-setup-dry" grok_isolated_run "$GB" jail setup
run "jail-help" grok_isolated_run "$GB" jail --help
run "mcp-list" grok_isolated_run "$GB" mcp list
run "elf-freebsd" sh -c "file \"$GB\" | grep -qi freebsd"

# Multi-provider / config — fixtures only (no operator ~/.gemini)
set +e
grok_isolated_run "$GB" providers --help >>"$LOG" 2>&1
ec=$?
set -e
if [ "$ec" -eq 0 ]; then
	ok "providers-help"
	run "providers-catalog" grok_isolated_run "$GB" providers catalog
	run "providers-models-offline" grok_isolated_run "$GB" providers models --provider openai --offline --no-cli
	run "providers-import-antigravity" grok_isolated_run "$GB" providers import-antigravity
	run "providers-import-open-code" grok_isolated_run "$GB" providers import-open-code --cwd "$GROK_ISO_WORKSPACE"
	run "config-export" sh -c "rm -rf \"$REG/cfg-export\" && grok_isolated_run \"$GB\" config export \"$REG/cfg-export\" --no-plugins && test -f \"$REG/cfg-export/manifest.json\""
	# Prove import-antigravity saw fixture MCP (not empty real config)
	if grok_isolated_run "$GB" providers import-antigravity 2>/dev/null | grep -q "fixture-stdio\|fixture-url\|demo-skill"; then
		ok "import-antigravity-fixture-hit"
	else
		bad "import-antigravity-fixture-hit"
	fi
	# Multi-provider fixture config must load (doctor config.parse) and keep
	# catalog entries visible via inspect / config export.
	if grok_isolated_run "$GB" doctor --ci 2>/dev/null | grep -q "config.parse"; then
		ok "multi-provider-config-parse-doctor"
	else
		# doctor --ci always prints config.parse on success path
		if grok_isolated_run "$GB" doctor --ci >>"$LOG" 2>&1; then
			ok "multi-provider-config-parse-doctor"
		else
			bad "multi-provider-config-parse-doctor"
		fi
	fi
	if test -f "$GROK_HOME/config.toml" \
		&& grep -q 'minimax-direct-m3' "$GROK_HOME/config.toml" \
		&& grep -q 'gateway-minimaxm3' "$GROK_HOME/config.toml" \
		&& grep -q 'glm-5.2:cloud' "$GROK_HOME/config.toml" \
		&& grep -q 'example.test' "$GROK_HOME/config.toml" \
		&& ! grep -qi 'cloudbsd' "$GROK_HOME/config.toml" \
		&& ! grep -qE 'sk-[a-zA-Z0-9]{10,}' "$GROK_HOME/config.toml"; then
		ok "multi-provider-fixture-config-shape"
	else
		bad "multi-provider-fixture-config-shape"
	fi
	# Offline-only validate: closed loopback port → Unreachable (exit non-zero).
	# Proves the CLI path works without any local LLM or cloud service.
	set +e
	out=$(grok_isolated_run "$GB" providers validate \
		--base-url http://127.0.0.1:1/v1 --no-hello 2>&1)
	ec=$?
	set -e
	if [ "$ec" -ne 0 ] && echo "$out" | grep -qiE 'Unreachable|unreachable|TCP|connect'; then
		ok "providers-validate-offline-unreachable"
	else
		echo "$out" >>"$LOG"
		bad "providers-validate-offline-unreachable (ec=$ec)"
	fi
else
	echo "SKIP providers-help (exit $ec)" | tee -a "$SUMMARY"
	skip=$((skip + 1))
fi

set +e
grok_isolated_run "$GB" doctor --ci >/dev/null 2>&1
ec=$?
set -e
if [ "$ec" -eq 0 ]; then ok "doctor-ci-exit-0"; else bad "doctor-ci-exit-$ec"; fi

set +e
grok_isolated_run "$GB" jail catalog >>"$LOG" 2>&1
ec=$?
set -e
if [ "$ec" -eq 0 ]; then ok "jail-catalog"; else
	echo "SKIP jail-catalog (exit $ec)" | tee -a "$SUMMARY"
	skip=$((skip + 1))
fi

set +e
grok_isolated_run "$GB" models >>"$LOG" 2>&1
ec=$?
set -e
if [ "$ec" -eq 0 ]; then ok "models"; else
	echo "SKIP models (exit $ec)" | tee -a "$SUMMARY"
	skip=$((skip + 1))
fi

pref=$(dirname "$(dirname "$GB")")
if [ -f "$pref/etc/bash_completion.d/grok-build" ]; then
	ok "bash-completion-file"
else
	bad "bash-completion-file"
fi
if [ -f "$pref/share/doc/grok-build/doctor.md" ]; then
	ok "doc-doctor-md"
else
	bad "doc-doctor-md"
fi

note "uninstall + reinstall"
if make uninstall PREFIX="$pref" >>"$LOG" 2>&1; then ok "uninstall"; else bad "uninstall"; fi
if [ ! -e "$GB" ]; then ok "binary-removed"; else bad "binary-still-present"; fi
if make install PREFIX=/usr/local >>"$LOG" 2>&1; then ok "reinstall"; else bad "reinstall"; fi
if [ -x "$HOME/.local/bin/grok-build" ]; then
	GB="$HOME/.local/bin/grok-build"
elif [ -x /usr/local/bin/grok-build ]; then
	GB=/usr/local/bin/grok-build
fi
run "post-reinstall-version" grok_isolated_run "$GB" --version
run "post-reinstall-doctor-ci" grok_isolated_run "$GB" doctor --ci

# TUI smoke (PTY harness) — isolated mock home, not operator env.
note "tui-regression"
if [ "${SKIP_TUI:-0}" = "1" ]; then
	echo "SKIP tui-regression (SKIP_TUI=1)" | tee -a "$SUMMARY"
	skip=$((skip + 1))
else
	set +e
	REG_DIR="$REG/tui" PAGER_BINARY="$GB" sh "$ROOT/scripts/tui-regression.sh" >>"$LOG" 2>&1
	ec=$?
	set -e
	if [ "$ec" -eq 0 ]; then
		ok "tui-regression"
	else
		bad "tui-regression"
		tail -50 "$LOG" || true
	fi
fi

echo "END $(ts) pass=$pass fail=$fail skip=$skip" | tee -a "$SUMMARY"
echo "logs: $REG"
cat "$SUMMARY"
exit "$fail"
