#!/bin/sh
# Regression against the *installed* application (make install), not cargo target/.
#
#   ./scripts/freebsd-installed-regression.sh
#
# Builds, runs doctor, installs via `make install` (PREFIX=/usr/local or
# userspace fallback under a temp HOME), then exercises the installed
# grok-build binary. Does not require root.
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
# Does not guarantee zero PII — see scripts/check-pii.sh header.
note "check-pii"
if sh "$ROOT/scripts/check-pii.sh" --changed >>"$LOG" 2>&1; then
	ok "check-pii"
else
	bad "check-pii"
	tail -40 "$LOG" || true
	# Hard fail: do not install a tree that failed the identity/secret gate
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

# Isolate install into temp HOME so fallback lands at $REG/home/.local
# without needing root, and without polluting the operator's real HOME.
REAL_HOME="${REAL_HOME:-${HOME:-}}"
export HOME="$REG/home"
mkdir -p "$HOME"

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

# --- installed binary ---
run "version" "$GB" --version
run "version-json" "$GB" version --json
run "help" "$GB" --help
run "doctor-ci" "$GB" doctor --ci
run "doctor-json" "$GB" doctor --ci --json
run "inspect" "$GB" inspect
run "jail-status" "$GB" jail status
run "jail-setup-dry" "$GB" jail setup
run "jail-help" "$GB" jail --help
run "mcp-list" "$GB" mcp list
run "elf-freebsd" sh -c "file \"$GB\" | grep -qi freebsd"

# Branch feature probes (multi-provider / config portability) — skip if absent.
set +e
"$GB" providers --help >>"$LOG" 2>&1
ec=$?
set -e
if [ "$ec" -eq 0 ]; then
	ok "providers-help"
	run "providers-catalog" "$GB" providers catalog
	run "providers-models-offline" "$GB" providers models --provider openai --offline
	run "providers-import-antigravity" env HOME="${REAL_HOME:-$HOME}" "$GB" providers import-antigravity
	run "config-export" sh -c "rm -rf \"$REG/cfg-export\" && \"$GB\" config export \"$REG/cfg-export\" --no-plugins && test -f \"$REG/cfg-export/manifest.json\""
else
	echo "SKIP providers-help (exit $ec)" | tee -a "$SUMMARY"
	skip=$((skip + 1))
fi

set +e
"$GB" doctor --ci >/dev/null 2>&1
ec=$?
set -e
if [ "$ec" -eq 0 ]; then ok "doctor-ci-exit-0"; else bad "doctor-ci-exit-$ec"; fi

set +e
"$GB" jail catalog >>"$LOG" 2>&1
ec=$?
set -e
if [ "$ec" -eq 0 ]; then ok "jail-catalog"; else
	echo "SKIP jail-catalog (exit $ec)" | tee -a "$SUMMARY"
	skip=$((skip + 1))
fi

set +e
"$GB" models >>"$LOG" 2>&1
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
run "post-reinstall-version" "$GB" --version
run "post-reinstall-doctor-ci" "$GB" doctor --ci

echo "END $(ts) pass=$pass fail=$fail skip=$skip" | tee -a "$SUMMARY"
echo "logs: $REG"
cat "$SUMMARY"
exit "$fail"
