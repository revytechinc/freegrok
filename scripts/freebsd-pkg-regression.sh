#!/bin/sh
# Product-truth regression: installed *package* path, not cargo target/.
#
#   ./scripts/freebsd-pkg-regression.sh [path/to/grok-build-*.pkg]
#
# Requires FreeBSD with pkg and an installed grok-build package (or a .pkg
# argument that can be installed with doas/sudo/pkg -U).
#
# Exit 0 only when every check PASSes. Real assertions only — no smoke fluff.
set -eu

pass=0
fail=0

ok() {
	echo "PASS $*"
	pass=$((pass + 1))
}

bad() {
	echo "FAIL $*"
	fail=$((fail + 1))
}

run() {
	name=$1
	shift
	if "$@"; then
		ok "$name"
	else
		bad "$name"
	fi
}

if [ "$(uname -s)" != "FreeBSD" ]; then
	echo "FAIL host-not-freebsd ($(uname -s))"
	exit 1
fi

PKG_ARG=${1:-}
if [ -n "$PKG_ARG" ] && [ -f "$PKG_ARG" ]; then
	echo "---- pkg install -U $PKG_ARG ----"
	if command -v doas >/dev/null 2>&1; then
		doas pkg install -U -y "$PKG_ARG"
	elif command -v sudo >/dev/null 2>&1; then
		sudo pkg install -U -y "$PKG_ARG"
	else
		pkg install -U -y "$PKG_ARG"
	fi
fi

export PATH="/usr/local/bin:${PATH}"
DOC=$(mktemp -t grok-pkg-doc.XXXXXX)
trap 'rm -f "$DOC"' EXIT

run pkg_info pkg info -e grok-build
run pkg_which sh -c 'pkg which /usr/local/bin/grok-build | grep -q grok-build'
run bin_exists test -x /usr/local/bin/grok-build
run static_exists test -x /usr/local/bin/grok-build-static
run alias_exists test -e /usr/local/bin/grok
run helper_libexec test -x /usr/local/libexec/grok-jail-helper
run elf sh -c 'file /usr/local/bin/grok-build | grep -qi FreeBSD'
run version sh -c 'grok-build --version >/dev/null'
# Capture stdout only after a successful doctor --ci (exit 0).
if grok-build doctor --ci >"$DOC"; then
	ok doctor_ci
else
	bad doctor_ci
fi
run backend_jail grep -q "Sandbox backend: jail" "$DOC"
if grep -E "Sandbox backend: nono-" "$DOC" >/dev/null 2>&1; then
	bad no_nono_backend
else
	ok no_nono_backend
fi
run jail_status sh -c 'grok-build jail status >/dev/null'
run jail_help sh -c 'grok-build jail --help >/dev/null'
run mcp_list sh -c 'grok-build mcp list >/dev/null'
run doctor_ci_2 sh -c 'grok-build doctor --ci >/dev/null'

# Packaged helper: dry-run prints a plan; --apply without GROK_JAIL_ROOT
# is fail-closed (exit 1 unprivileged, or 3 if somehow root without a root dir).
HELPER=/usr/local/libexec/grok-jail-helper
if "$HELPER" --dry-run --deny-write /tmp -- -- /usr/bin/true >/tmp/grok-helper-dry.out 2>&1; then
	if grep -q "dry-run: not creating a jail" /tmp/grok-helper-dry.out; then
		ok helper_dry_run
	else
		bad helper_dry_run_missing_banner
	fi
else
	bad helper_dry_run
fi
set +e
env -u GROK_JAIL_ROOT "$HELPER" --apply -- -- /usr/bin/true >/tmp/grok-helper-apply.out 2>&1
apply_ec=$?
set -e
case $apply_ec in
	1|3) ok helper_apply_fail_closed ;;
	*) bad "helper_apply_fail_closed exit=$apply_ec" ;;
esac
if grep -Eq "refusing host path=/|--apply requires root|--apply refused" /tmp/grok-helper-apply.out; then
	ok helper_apply_message
else
	bad helper_apply_message
fi
rm -f /tmp/grok-helper-dry.out /tmp/grok-helper-apply.out

echo "PKG_REGRESSION pass=$pass fail=$fail host=$(uname -srm)"
if [ "$fail" -ne 0 ]; then
	exit 1
fi
exit 0
