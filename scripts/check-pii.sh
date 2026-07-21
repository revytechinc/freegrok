#!/bin/sh
# check-pii.sh — best-effort PII / secret gate for *this* tree.
#
# Honest limits (read this before trusting the green light):
#   • This does NOT find all PII "across the board."
#   • It cannot prove absence of personal data in prose, screenshots,
#     third_party blobs, already-pushed history, or cleverly split strings.
#   • Auto-rewrite would be worse than a fail-closed report (wrong edits).
#
# What it *does* do well enough for commit + regression gates:
#   1. Scan only what you are about to ship (staged / changed) by default.
#   2. Flag HIGH-confidence secrets (private keys, OAuth access tokens, etc.).
#   3. Flag the *current operator identity* ($USER / $LOGNAME / $HOME leaf)
#      appearing in source, so your real home/username does not land in tests.
#   4. Allowlist common synthetic fixture identities (user, alice, bob, …).
#   5. Optional extra denylist: .pii-deny (one term per line) or PII_EXTRA.
#
# Usage:
#   ./scripts/check-pii.sh              # staged if any, else unstaged+untracked source
#   ./scripts/check-pii.sh --staged     # git index only (pre-commit)
#   ./scripts/check-pii.sh --changed    # staged + unstaged vs HEAD
#   ./scripts/check-pii.sh --tree       # whole tracked source (slow; noisy)
#   ./scripts/check-pii.sh --fix    # report only, exit 0
#   make check-pii
#
# Exit 1 on hits (unless --fix). Never prints secret values in full.
set -eu

ROOT=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
cd "$ROOT"

MODE=auto
FIX=0
QUIET=0
while [ $# -gt 0 ]; do
	case "$1" in
	--staged) MODE=staged ;;
	--changed) MODE=changed ;;
	--tree) MODE=tree ;;
	--auto) MODE=auto ;;
	--fix) FIX=1 ;;
	-q|--quiet) QUIET=1 ;;
	-h|--help)
		sed -n '2,35p' "$0" | sed 's/^# \{0,1\}//'
		exit 0
		;;
	*)
		echo "unknown arg: $1 (try --help)" >&2
		exit 2
		;;
	esac
	shift
done

log() {
	if [ "$QUIET" -eq 0 ]; then
		printf '%s\n' "$*"
	fi
}

# --- allowlist: synthetic / public fixture names never flagged as operator ---
is_allowlisted_user() {
	case "$1" in
	user|users|alice|bob|carol|dave|eve|mallory|trent|agent|demo|test|tester| \
	example|sample|someone|nobody|anonymous|admin|root|ubuntu|freebsd| \
	runner|ci|github|gitlab|builder|developer|dev|ops|local|me|u|x)
		return 0
		;;
	esac
	return 1
}

# Collect operator terms (username, home leaf).
collect_operator_terms() {
	terms=
	for v in USER LOGNAME; do
		eval "val=\${$v-}"
		if [ -n "${val}" ] && ! is_allowlisted_user "$val"; then
			terms="$terms $val"
		fi
	done
	if [ -n "${HOME-}" ]; then
		leaf=$(basename "$HOME")
		if [ -n "$leaf" ] && [ "$leaf" != "/" ] && ! is_allowlisted_user "$leaf"; then
			terms="$terms $leaf"
		fi
	fi
	# Optional project-local denylist (not committed secrets — just names)
	if [ -f "$ROOT/.pii-deny" ]; then
		while IFS= read -r line || [ -n "$line" ]; do
			case "$line" in
			''|\#*) continue ;;
			esac
			terms="$terms $line"
		done <"$ROOT/.pii-deny"
	fi
	if [ -n "${PII_EXTRA-}" ]; then
		# space or comma separated
		oldifs=$IFS
		IFS=', '
		# shellcheck disable=SC2086
		set -- $PII_EXTRA
		IFS=$oldifs
		for t in "$@"; do
			[ -n "$t" ] && terms="$terms $t"
		done
	fi
	# dedupe
	out=
	for t in $terms; do
		dup=0
		for u in $out; do
			[ "$u" = "$t" ] && dup=1 && break
		done
		[ "$dup" -eq 0 ] && out="$out $t"
	done
	# trim
	echo "$out" | sed 's/^ *//;s/ *$//'
}

# File list for scan mode
list_files() {
	case "$MODE" in
	staged)
		git diff --cached --name-only --diff-filter=ACMR 2>/dev/null || true
		;;
	changed)
		{
			git diff --cached --name-only --diff-filter=ACMR 2>/dev/null || true
			git diff --name-only --diff-filter=ACMR 2>/dev/null || true
			git ls-files --others --exclude-standard 2>/dev/null || true
		} | sort -u
		;;
	tree)
		git ls-files 2>/dev/null || find . -type f \
			! -path './target/*' ! -path './.git/*' ! -path './third_party/*'
		;;
	auto)
		staged=$(git diff --cached --name-only --diff-filter=ACMR 2>/dev/null || true)
		if [ -n "$staged" ]; then
			printf '%s\n' "$staged"
		else
			{
				git diff --name-only --diff-filter=ACMR 2>/dev/null || true
				git ls-files --others --exclude-standard 2>/dev/null || true
			} | sort -u
		fi
		;;
	esac
}

# Only scan text-ish source we own (skip vendored / huge binary catalogs)
should_scan() {
	f=$1
	case "$f" in
	''|*.png|*.jpg|*.jpeg|*.gif|*.webp|*.ico|*.pdf|*.zip|*.gz|*.tgz|*.xz| \
	*.so|*.a|*.o|*.dylib|*.wasm|*.bin|*.exe|*.dll|*.pdb|*.db|*.sqlite| \
	*.lock|*.mp4|*.webm|*.woff|*.woff2|*.ttf|*.otf)
		return 1
		;;
	third_party/*|./third_party/*|target/*|./target/*|.git/*)
		return 1
		;;
	# shipped catalog is public models.dev data, not personal
	*/models_dev.json.gz|*/models_dev.json)
		return 1
		;;
	esac
	# Prefer code + docs + scripts + tests
	case "$f" in
	*.rs|*.ts|*.js|*.go|*.py|*.sh|*.md|*.toml|*.yml|*.yaml|*.json|*.jsonc| \
	*.txt|*.html|*.css|*.svg|*.proto|*.h|*.c|*.cpp|*.cc|*.hpp|Makefile| \
	*.mk|*.in|*.inc|*.skill|SKILL.md|AGENTS.md|*.tmpl)
		return 0
		;;
	scripts/*|crates/*|docs/*|ports/*)
		return 0
		;;
	esac
	return 1
}

hits=0
files_scanned=0
report=$(mktemp "${TMPDIR:-/tmp}/pii-report.XXXXXX")
trap 'rm -f "$report"' EXIT

OPERATOR_TERMS=$(collect_operator_terms)

log "check-pii: mode=$MODE fix=$FIX"
if [ -n "$OPERATOR_TERMS" ]; then
	log "check-pii: operator terms (identity): $OPERATOR_TERMS"
else
	log "check-pii: no non-allowlisted operator username (only generic secret patterns)"
fi

# High-confidence secret patterns (value redacted in output).
# Use rg if available; fall back to grep -E.
RG=
if command -v rg >/dev/null 2>&1; then
	RG=rg
fi

# Build regexes at runtime so this script's source is not a self-hit.
secret_patterns() {
	# PEM / OpenSSH private keys
	printf '%s\n' '-----BEGIN ([A-Z0-9 ]+)?PRIVATE KEY-----'
	printf '%s\n' '-----BEGIN OPENSSH PRIVATE KEY-----'
	# Google OAuth access token prefix + long body
	printf '%s\n' "ya29\\.[A-Za-z0-9_-]{20,}"
	# Google API key
	printf '%s\n' "AIza[0-9A-Za-z_-]{20,}"
	# OpenAI-style secret keys
	printf '%s\n' "sk-[A-Za-z0-9]{20,}"
	# Slack tokens
	printf '%s\n' "xox[baprs]-[A-Za-z0-9-]{10,}"
	# GitHub PATs
	printf '%s\n' "ghp_[A-Za-z0-9]{20,}"
	printf '%s\n' "github_pat_[A-Za-z0-9_]{20,}"
	# AWS access key id
	printf '%s\n' "AKIA[0-9A-Z]{16}"
	# JWT-shaped (three base64url segments)
	printf '%s\n' "eyJ[A-Za-z0-9_-]{20,}\\.[A-Za-z0-9_-]{10,}\\.[A-Za-z0-9_-]{10,}"
}

scan_file_secrets() {
	file=$1
	# Never flag the gate's own source or the example denylist.
	case "$file" in
	scripts/check-pii.sh|./scripts/check-pii.sh|.pii-deny.example)
		return 0
		;;
	esac

	if [ -n "$RG" ]; then
		secret_patterns | while IFS= read -r pat; do
			[ -z "$pat" ] && continue
			if rg -n --pcre2 -e "$pat" -- "$file" >/dev/null 2>&1; then
				rg -n --pcre2 -e "$pat" -- "$file" 2>/dev/null | while IFS= read -r line; do
					ln=$(printf '%s\n' "$line" | cut -d: -f1)
					printf 'SECRET  %s:%s  (pattern match; value redacted)\n' "$file" "$ln" >>"$report"
					echo 1 >>"${report}.n"
				done
			fi
		done
	else
		secret_patterns | while IFS= read -r pat; do
			[ -z "$pat" ] && continue
			if grep -nE -e "$pat" -- "$file" >/dev/null 2>&1; then
				grep -nE -e "$pat" -- "$file" 2>/dev/null | while IFS= read -r line; do
					ln=$(printf '%s\n' "$line" | cut -d: -f1)
					printf 'SECRET  %s:%s  (pattern match; value redacted)\n' "$file" "$ln" >>"$report"
					echo 1 >>"${report}.n"
				done
			fi
		done
	fi
}

scan_file_operator() {
	file=$1
	[ -z "$OPERATOR_TERMS" ] && return 0
	for term in $OPERATOR_TERMS; do
		# Word-ish matches of operator identity in source
		# Flag: /home/$term, \Users\$term, $term@, bare path segments
		if [ -n "$RG" ]; then
			if rg -n -e "/home/${term}([/\"'[:space:]]|\$)" \
				-e "/Users/${term}([/\\\\\"'[:space:]]|\$)" \
				-e "${term}@" \
				-e "\\\\${term}\\\\" \
				-- "$file" >/dev/null 2>&1; then
				rg -n -e "/home/${term}([/\"'[:space:]]|\$)" \
					-e "/Users/${term}([/\\\\\"'[:space:]]|\$)" \
					-e "${term}@" \
					-e "\\\\${term}\\\\" \
					-- "$file" 2>/dev/null | while IFS= read -r line; do
					ln=$(printf '%s\n' "$line" | cut -d: -f1)
					# redact: show term class only
					printf 'IDENTITY %s:%s  (operator term %s — replace with synthetic fixture)\n' \
						"$file" "$ln" "$term" >>"$report"
					echo 1 >>"${report}.n"
				done
			fi
			# Also catch bare username in quoted home-ish strings in tests only
			case "$file" in
			*test*|*Tests*|*_test.*|*/tests/*)
				if rg -n -e "\"/home/${term}" -e "'/home/${term}" -- "$file" >/dev/null 2>&1; then
					rg -n -e "\"/home/${term}" -e "'/home/${term}" -- "$file" 2>/dev/null | while IFS= read -r line; do
						ln=$(printf '%s\n' "$line" | cut -d: -f1)
						printf 'IDENTITY %s:%s  (operator home path in test)\n' "$file" "$ln" >>"$report"
						echo 1 >>"${report}.n"
					done
				fi
				;;
			esac
		else
			if grep -nE -e "/home/${term}([/\"'[:space:]]|$)" \
				-e "${term}@" -- "$file" >/dev/null 2>&1; then
				grep -nE -e "/home/${term}([/\"'[:space:]]|$)" \
					-e "${term}@" -- "$file" 2>/dev/null | while IFS= read -r line; do
					ln=$(printf '%s\n' "$line" | cut -d: -f1)
					printf 'IDENTITY %s:%s  (operator term %s)\n' "$file" "$ln" "$term" >>"$report"
					echo 1 >>"${report}.n"
				done
			fi
		fi
	done
}

: >"${report}.n"

# shellcheck disable=SC2046
set -- $(list_files | tr '\n' ' ')
if [ $# -eq 0 ] || [ -z "${1-}" ]; then
	log "check-pii: no files to scan (nothing staged/changed)"
	log "PASS check-pii (nothing to scan)"
	exit 0
fi

for f in "$@"; do
	[ -f "$f" ] || continue
	should_scan "$f" || continue
	files_scanned=$((files_scanned + 1))
	scan_file_secrets "$f"
	scan_file_operator "$f"
done

if [ -f "${report}.n" ]; then
	hits=$(wc -l <"${report}.n" | tr -d ' ')
else
	hits=0
fi

log "check-pii: scanned $files_scanned file(s), hits=$hits"

if [ "$hits" -gt 0 ]; then
	log "---- findings (values redacted where secrets) ----"
	if [ "$QUIET" -eq 0 ]; then
		sort -u "$report" 2>/dev/null || cat "$report"
	fi
	log "----"
	log "check-pii: FAIL — replace operator identity with synthetic fixtures"
	log "  (alice/user/demo) and remove secrets. Re-run: ./scripts/check-pii.sh --staged"
	log "  Optional denylist file: .pii-deny   Extra terms: PII_EXTRA=name1,name2"
	log "  This gate is best-effort; it does not guarantee zero PII."
	if [ "$FIX" -eq 1 ]; then
		log "check-pii: --fix set; reporting only"
		exit 0
	fi
	exit 1
fi

log "PASS check-pii"
exit 0
