#!/bin/sh
# isolated-env.sh — sourceable helpers for hermetic CLI / TUI regression.
#
# Usage (from another script after setting ROOT):
#   . "$ROOT/scripts/lib/isolated-env.sh"
#   grok_isolation_init "$REG/sandbox"
#   grok_isolation_seed_fixtures
#   grok_isolated_run "$GB" providers discover
#
# Does NOT mutate the operator's real $HOME. All state lands under the
# sandbox root. API keys and SSH agent from the parent are cleared for
# child processes.
#
# shellcheck shell=sh

# Shell-global sandbox paths (set by grok_isolation_init).
GROK_ISO_ROOT=
GROK_ISO_HOME=
GROK_ISO_GROK_HOME=
GROK_ISO_WORKSPACE=
GROK_ISO_TMP=

# Generic synthetic user — never the operator's username.
GROK_ISO_USER="${GROK_ISO_USER:-testuser}"

grok_isolation_init() {
	GROK_ISO_ROOT=${1:?sandbox root required}
	GROK_ISO_HOME="$GROK_ISO_ROOT/home"
	GROK_ISO_GROK_HOME="$GROK_ISO_HOME/.grok"
	GROK_ISO_WORKSPACE="$GROK_ISO_ROOT/workspace"
	GROK_ISO_TMP="$GROK_ISO_ROOT/tmp"
	mkdir -p \
		"$GROK_ISO_HOME" \
		"$GROK_ISO_GROK_HOME" \
		"$GROK_ISO_WORKSPACE" \
		"$GROK_ISO_TMP" \
		"$GROK_ISO_ROOT/xdg-config" \
		"$GROK_ISO_ROOT/xdg-data" \
		"$GROK_ISO_ROOT/xdg-cache" \
		"$GROK_ISO_ROOT/xdg-state"

	# Minimal non-interactive shell rc so login shells stay quiet.
	printf '# isolated regression home\n' >"$GROK_ISO_HOME/.profile"
	printf '# isolated regression home\n' >"$GROK_ISO_HOME/.bashrc"
}

# Seed synthetic sibling-tool configs under the sandbox (no real ~/.gemini).
grok_isolation_seed_fixtures() {
	if [ -z "$GROK_ISO_HOME" ]; then
		echo "error: grok_isolation_init first" >&2
		return 1
	fi
	# Antigravity / Gemini layout (synthetic only)
	agy="$GROK_ISO_HOME/.gemini"
	mkdir -p \
		"$agy/antigravity-cli/builtin/skills/demo-skill" \
		"$agy/config/skills/user-skill" \
		"$agy/antigravity-cli/cache"
	cat >"$agy/antigravity-cli/settings.json" <<'JSON'
{
  "toolPermission": "default",
  "permissions": { "allow": ["command(true)"] },
  "trustedWorkspaces": ["/tmp/isolated-workspace"],
  "useG1Credits": false
}
JSON
	cat >"$agy/config/mcp_config.json" <<'JSON'
{
  "mcpServers": {
    "fixture-stdio": {
      "command": "true",
      "args": []
    },
    "fixture-url": {
      "serverUrl": "https://mcp.example.invalid/sse"
    }
  }
}
JSON
	cat >"$agy/config/config.json" <<'JSON'
{ "userSettings": { "remoteControlHostname": "isolated-fixture" } }
JSON
	# OAuth *shape* only — values intentionally do not match live token regexes
	# (check-pii / secret scanners) while remaining non-empty for import scan.
	cat >"$agy/antigravity-cli/antigravity-oauth-token" <<'JSON'
{
  "token": {
    "access_token": "fixture-access-token-not-real",
    "token_type": "Bearer",
    "refresh_token": "fixture-refresh-token-not-real",
    "expiry": "2099-01-01T00:00:00Z"
  },
  "auth_method": "consumer"
}
JSON
	printf '%s\n' '---' 'name: demo-skill' '---' '# Demo' > \
		"$agy/antigravity-cli/builtin/skills/demo-skill/SKILL.md"
	printf '%s\n' '---' 'name: user-skill' '---' '# User' > \
		"$agy/config/skills/user-skill/SKILL.md"

	# Minimal OpenCode project config in workspace
	cat >"$GROK_ISO_WORKSPACE/opencode.json" <<'JSON'
{
  "model": "ollama/llama3.2",
  "provider": {
    "ollama": {
      "npm": "@ai-sdk/openai-compatible",
      "options": { "baseURL": "http://127.0.0.1:11434/v1" },
      "models": { "llama3.2": {}, "codellama": {} }
    }
  }
}
JSON

	# Minimal Grok config under isolated GROK_HOME, including multi-provider
	# catalog shape. Hostnames are RFC 2606 test domains only — no operator
	# infra. Keys are env_key names only (isolation clears real secrets).
	mkdir -p "$GROK_ISO_GROK_HOME"
	cat >"$GROK_ISO_GROK_HOME/config.toml" <<'TOML'
[cli]
installer = "internal"

[ui]
permission_mode = "always-approve"

# --- multi-provider catalog fixture (parse-only; portable on any machine) ---
[model.minimax-direct-m3]
model = "MiniMax-M3"
name = "MiniMax M3 (direct)"
base_url = "https://minimax.example.test/anthropic/v1"
api_backend = "messages"
auth_scheme = "x_api_key"
env_key = "MINIMAX_API_KEY"
context_window = 204800
extra_headers = { "anthropic-version" = "2023-06-01" }

[model.gateway-minimaxm3]
model = "minimaxm3"
name = "minimaxm3 (OpenAI-compat gateway)"
base_url = "https://llm-gateway.example.test/v1"
env_key = "LLM_GATEWAY_API_KEY"

[model.gateway-glm-5-2-cloud]
model = "glm-5.2:cloud"
name = "glm-5.2 cloud (gateway)"
base_url = "https://llm-gateway.example.test/v1"
env_key = "LLM_GATEWAY_API_KEY"

[model.local-openai-compat]
model = "local-coder"
name = "Local OpenAI-compat (no key)"
base_url = "http://127.0.0.1:9/v1"
TOML
}

# Env pairs for `env` prefix — clear secrets, pin isolation paths.
# Usage: eval "$(grok_isolation_env_exports)"
grok_isolation_env_exports() {
	if [ -z "$GROK_ISO_HOME" ]; then
		echo "error: grok_isolation_init first" >&2
		return 1
	fi
	cat <<EOF
export HOME='$GROK_ISO_HOME'
export USER='$GROK_ISO_USER'
export LOGNAME='$GROK_ISO_USER'
export GROK_HOME='$GROK_ISO_GROK_HOME'
export XDG_CONFIG_HOME='$GROK_ISO_ROOT/xdg-config'
export XDG_DATA_HOME='$GROK_ISO_ROOT/xdg-data'
export XDG_CACHE_HOME='$GROK_ISO_ROOT/xdg-cache'
export XDG_STATE_HOME='$GROK_ISO_ROOT/xdg-state'
export TMPDIR='$GROK_ISO_TMP'
export TMP='$GROK_ISO_TMP'
export TEMP='$GROK_ISO_TMP'
unset SSH_AUTH_SOCK SSH_AGENT_PID SSH_CONNECTION SSH_CLIENT SSH_TTY
unset SSH_ASKPASS SSH_ASKPASS_REQUIRE
unset OPENAI_API_KEY ANTHROPIC_API_KEY ANTHROPIC_AUTH_TOKEN
unset GEMINI_API_KEY GOOGLE_API_KEY GOOGLE_GENAI_API_KEY
unset XAI_API_KEY GROK_CODE_XAI_API_KEY OPENROUTER_API_KEY
unset TOGETHER_API_KEY GROQ_API_KEY DEEPSEEK_API_KEY MISTRAL_API_KEY MINIMAX_API_KEY
unset LITELLM_API_KEY LITELLM_MASTER_KEY
unset AWS_SECRET_ACCESS_KEY AWS_ACCESS_KEY_ID AWS_SESSION_TOKEN
unset GH_TOKEN GITHUB_TOKEN
export GROK_TELEMETRY_ENABLED=false
export GROK_FEEDBACK_ENABLED=false
export GROK_TRACE_UPLOAD=false
export GROK_PROMPT_SUGGESTIONS=false
export NO_COLOR=1
export TERM=xterm-256color
export PATH='/usr/local/bin:/usr/bin:/bin'
EOF
}

# Run a command fully isolated (subshell so parent env is unchanged).
# Example: grok_isolated_run /path/to/grok-build providers discover
grok_isolated_run() {
	if [ -z "$GROK_ISO_HOME" ]; then
		echo "error: grok_isolation_init first" >&2
		return 1
	fi
	(
		# shellcheck disable=SC1091
		eval "$(grok_isolation_env_exports)"
		cd "$GROK_ISO_WORKSPACE" || exit 1
		exec "$@"
	)
}

# Print sandbox paths (for logs).
grok_isolation_summary() {
	cat <<EOF
isolation_root=$GROK_ISO_ROOT
isolation_home=$GROK_ISO_HOME
isolation_grok_home=$GROK_ISO_GROK_HOME
isolation_workspace=$GROK_ISO_WORKSPACE
isolation_user=$GROK_ISO_USER
EOF
}
