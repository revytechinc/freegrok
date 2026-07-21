# Providers (multi-LLM)

Grok Build can use **SpaceXAI/Grok** models and **third-party / local** OpenAI-compatible or Anthropic Messages endpoints.

## Discover what is installed

```bash
grok providers discover
# or in the TUI:
/connect
```

Scans:

- Local servers (Ollama `:11434`, LM Studio `:1234`, llama.cpp `:8080`)
- Common API key environment variables (presence only — values never printed)
- Sibling tool configs (Claude, Cursor, OpenCode)
- Existing `~/.grok/config.toml` `[model.*]` blocks

## Validate a connection

Proves reachability and (by default) a tiny **hello** completion:

```bash
# Local Ollama (no key)
grok providers validate --base-url http://127.0.0.1:11434/v1 --no-hello   # list only
grok providers validate --base-url http://127.0.0.1:11434/v1               # + hello

# OpenAI
grok providers validate \
  --base-url https://api.openai.com/v1 \
  --env-key OPENAI_API_KEY \
  --api-backend chat_completions

# Anthropic / MiniMax (messages + x-api-key)
grok providers validate \
  --base-url https://api.minimax.io/anthropic/v1 \
  --env-key MINIMAX_API_KEY \
  --api-backend messages \
  --auth-scheme x_api_key
```

Levels: **L0** TCP → **L1** `GET …/models` → **L2** minimal `"hello"` completion.

`doctor --ci` loads the offline models.dev catalog and runs discovery (no paid hellos).

## Catalog (models.dev)

```bash
grok providers catalog
```

Offline snapshot ships with the binary and install (`share/grok-build/models_dev.json.gz`).  
Runtime may refresh `~/.grok/cache/models_dev.json.gz` **only if** the downloaded JSON parses and validates.

## Configure a model

See [11-custom-models.md](./11-custom-models.md) for `[model.*]` TOML, `auth_scheme`, Ollama, MiniMax, and OpenAI examples.
