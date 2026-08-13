#!/bin/sh
# Installed-product integration (pkg truth + optional live L6).
#
#   ./scripts/freebsd-product-integration.sh
#   GROK_LIVE=1 ./scripts/freebsd-product-integration.sh   # --single PONG + file canary
#
# Exit 0 only when every executed check PASSes.
set -eu

ROOT=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
pass=0
fail=0
skip=0

ok() { echo "PASS $*"; pass=$((pass + 1)); }
bad() { echo "FAIL $*"; fail=$((fail + 1)); }
skipn() { echo "SKIP $*"; skip=$((skip + 1)); }

echo "HOST $(uname -srm) $(hostname)"
echo "GIT $(git -C "$ROOT" rev-parse --short HEAD 2>/dev/null || echo n/a)"

if [ "$(uname -s)" != "FreeBSD" ]; then
	bad host-not-freebsd
	echo "PRODUCT_INTEGRATION pass=$pass fail=$fail skip=$skip"
	exit 1
fi

if ! "$ROOT/scripts/freebsd-pkg-regression.sh"; then
	bad pkg_regression
else
	ok pkg_regression
fi

# Offline product surfaces beyond pkg-regression.
run() {
	name=$1
	shift
	if "$@" >/dev/null 2>&1; then
		ok "$name"
	else
		bad "$name"
	fi
}

run inspect_json freegrok inspect
run version_json freegrok version --json
run jail_status_json freegrok jail status --json

if [ "${GROK_LIVE:-0}" = "1" ]; then
	CANARY=/tmp/l6-canary-$$
	mkdir -p "$CANARY"
	LIVE_MODEL=${GROK_LIVE_MODEL:-}
	set -- freegrok --single "Reply with exactly the word PONG and nothing else."
	if [ -n "$LIVE_MODEL" ]; then
		set -- freegrok -m "$LIVE_MODEL" --single "Reply with exactly the word PONG and nothing else."
	fi
	set +e
	timeout 180 "$@" >"$CANARY/pong.out" 2>&1
	pong_ec=$?
	set -e
	if [ "$pong_ec" -eq 0 ] && grep -q PONG "$CANARY/pong.out"; then
		ok l6_pong
	else
		bad "l6_pong exit=$pong_ec"
		# Do not dump full error (may include model catalogs); keep last status line.
		tail -3 "$CANARY/pong.out" | cut -c1-200 || true
	fi
	if [ -n "$LIVE_MODEL" ]; then
		set -- freegrok -m "$LIVE_MODEL" --single "Create a file named canary.txt containing exactly CANARY_OK. Use a file write tool if available. When done reply CANARY_DONE." --cwd "$CANARY"
	else
		set -- freegrok --single "Create a file named canary.txt containing exactly CANARY_OK. Use a file write tool if available. When done reply CANARY_DONE." --cwd "$CANARY"
	fi
	set +e
	timeout 180 "$@" >"$CANARY/file.out" 2>&1
	file_ec=$?
	set -e
	if [ "$file_ec" -eq 0 ] && test -f "$CANARY/canary.txt" && grep -q CANARY_OK "$CANARY/canary.txt"; then
		ok l6_file_canary
	else
		bad "l6_file_canary exit=$file_ec"
		tail -20 "$CANARY/file.out" || true
	fi
else
	skipn l6_live "(set GROK_LIVE=1)"
fi

echo "PRODUCT_INTEGRATION pass=$pass fail=$fail skip=$skip"
if [ "$fail" -ne 0 ]; then
	exit 1
fi
exit 0
