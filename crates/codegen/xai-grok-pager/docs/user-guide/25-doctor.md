# Diagnostics (`grok doctor`)

Product self-checks for the **installed** binary. This is not a full test suite (see `cargo test` / `scripts/freebsd-regression.sh` for developers).

Stay offline by default. Do not require admin or network unless you opt in.

---

## Quick start

```bash
grok doctor              # human report
grok doctor --json       # machine-readable
grok doctor --quick      # critical subset
grok doctor --ci         # build gate: offline, fail only on critical, ≤15s
```

On FreeBSD ports packages that ship as **`grok-build`**, use that command name.

Exit codes:

| Code | Meaning |
|------|---------|
| `0` | OK (or only warnings when using `--ci`) |
| `1` | Warnings (default mode) |
| `2` | Critical failure (or wall-budget stall under `--ci`) |

---

## Build / CI gate (`--ci`)

Use this in packaging and FreeBSD regression. It:

- Runs offline (no network, auth, MCP, live inference, or sandbox-deep)
- Uses the quick check set
- **Ignores warnings** (e.g. optional jail helper missing)
- Fails only on **critical** checks
- Enforces a **15s** whole-run wall budget and short per-command timeouts (must not stall)

```bash
# From a source tree (FreeBSD Makefile):
make build && make doctor

# Or cargo:
cargo build -p xai-grok-pager-bin --release
./target/release/xai-grok-pager doctor --ci

# Unit gate (library):
cargo test -p xai-grok-shell --lib doctor::tests::doctor_ci_gate_no_critical_failures
```

After `make install` (global or `~/.local` fallback — same command), run `grok-build doctor --ci`.  
FreeBSD ports WIP: `make doctor-ci` / `do-test` with `GROK_REUSE_TARGET=yes`.

---

## Default checks (offline)

Typical critical checks:

- Binary identity (e.g. FreeBSD ELF vs Linux brand under linuxulator)
- FreeBSD version floor (**14, 15, 16**; 16 = CURRENT/dev)
- System `rg` on `PATH` (FreeBSD does not bundle BurntSushi rg assets)
- Writable `~/.grok` and temp dir
- Config load
- Shell spawn
- Sandbox backend status (info / warn if jail helper absent)

Optional / opt-in flags (partial or stub until fully wired):

| Flag | Intent |
|------|--------|
| `--online` | Network probes (update check, etc.) |
| `--auth` | Credential / models probes |
| `--mcp` | MCP connectivity (see also `grok mcp doctor`) |
| `--sandbox-deep` | Deeper jail helper probes; never prompts for sudo |
| `--live` | One inference turn (quota-sensitive) |
| `--strict` | Treat warnings as failures (non-`--ci`) |

---

## Related commands

```bash
grok inspect                 # config / skills / MCP inventory for this directory
grok mcp doctor              # MCP server connectivity
grok models                  # auth + model list (needs credentials)
grok update --check          # release channel
grok jail status             # FreeBSD isolation status (offline)
grok jail setup              # FreeBSD optional isolation dry-run
```

---

## FreeBSD notes

- Missing jail helper → **warning**, not a critical failure under `--ci`
- Unprivileged processes cannot create jails; isolation is optional
- Prefer native FreeBSD package binary `grok-build` over a Linux brand install via linuxulator when packaging for FreeBSD

Engineering plan and ports notes: source tree `docs/freebsd-port-and-jail-sandbox.md`.
