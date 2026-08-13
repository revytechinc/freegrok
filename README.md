<div align="center">

<h1>FreeGrok (<code>freegrok</code>)</h1>

**FreeGrok** is the CloudBSD / RevyTech fork of the open-source Grok Build
agent TUI. It runs as a full-screen terminal agent that understands your
codebase, edits files, executes shell commands, searches the web, and
manages long-running tasks — interactively, headlessly for scripting/CI, or
embedded in editors via the Agent Client Protocol (ACP).

[Building from source](#building-from-source) ·
[Configuration](#configuration) ·
[Documentation](#documentation) ·
[Repository layout](#repository-layout) ·
[Development](#development) ·
[License](#license)

This repository is **[revytechinc/freegrok](https://github.com/revytechinc/freegrok)**.
It is periodically synced from upstream `xai-org/grok-build`. Do **not** push
this fork back to xAI.

A small `SOURCE_REV` file at the root records the upstream monorepo commit SHA
for the version of the code present in this tree.

</div>

---

## Clone

```sh
git clone git@github.com:revytechinc/freegrok.git
cd freegrok
```

HTTPS: `https://github.com/revytechinc/freegrok.git`

## Installing the released binary

Upstream xAI publishes official `grok` binaries at [x.ai/cli](https://x.ai/cli).
This fork ships **`freegrok`** via source/`make install` and FreeBSD ports/pkg
(see below). Compat names `grok`, `grok-build`, and short alias `fg` are
installed as symlinks for one release.

## Building from source

Requirements:

- **Rust** — the toolchain is pinned by [`rust-toolchain.toml`](rust-toolchain.toml);
  `rustup` installs it automatically on first build (or use FreeBSD `lang/rust`).
- **protoc** — system `protoc` on `PATH`, or set `PROTOC=…`.  
  FreeBSD: `pkg install protobuf` (the repo `bin/protoc` DotSlash helper has no FreeBSD asset).
- **ripgrep** (runtime): FreeBSD builds do not bundle `rg` — `pkg install ripgrep`.
- **[DotSlash](https://dotslash-cli.com)** — optional on FreeBSD if you use system
  protoc; on macOS/Linux it may still be needed for hermetic `bin/protoc`:

  ```sh
  cargo install dotslash
  /usr/bin/env dotslash --help   # sanity check when using bin/protoc
  ```

- macOS, Linux, and **FreeBSD 14+** are supported build hosts; Windows is best-effort.

### FreeBSD: Makefile install (no ports required)

From a clone on FreeBSD 14, 15, or 16 (dev):

```sh
pkg install rust protobuf ripgrep         # once
cd /path/to/freegrok
make build                                # always runs cargo (incremental)
make doctor                               # offline self-check (doctor --ci)
make install                              # → /usr/local if writable, else ~/.local
freegrok --version && freegrok doctor --ci
```

**One install interface:** `make install` always installs **`freegrok`** (plus
`freegrok-static`, `fg`, and compat `grok` / `grok-build` symlinks). It prefers
`PREFIX` (default `/usr/local`). If that tree is not writable, it installs to
`$HOME/.local` and prints a note — no separate target.

| Variable | Default | Meaning |
|----------|---------|---------|
| `PREFIX` | `/usr/local` | Preferred install root |
| `BINNAME` | `freegrok` | Primary command name |
| `DESTDIR` | empty | Staging root (no userspace fallback when set) |
| `PROTOC` | `protoc` | Protobuf compiler |

```sh
make uninstall                            # from PREFIX
make uninstall PREFIX=$$HOME/.local       # if install fell back
```

### Cargo (all platforms)

```sh
export PROTOC=$(command -v protoc)        # FreeBSD / system protoc
cargo run -p xai-grok-pager-bin           # build + launch the TUI
cargo build -p xai-grok-pager-bin --release  # → target/release/freegrok
cargo check -p xai-grok-pager-bin
```

The cargo artifact is **`freegrok`**. The crate package is still
`xai-grok-pager-bin` (internal crate directories are not renamed in Phase A).
On first launch FreeGrok copies a missing `~/.freegrok` from `~/.grok` (grok-build
config tree) so existing skills, `config.toml`, auth, and hooks keep working.

See the [authentication guide](crates/codegen/xai-grok-pager/docs/user-guide/02-authentication.md).

FreeBSD ports packaging (optional, separate tree): see
[`docs/freebsd-port-and-jail-sandbox.md`](docs/freebsd-port-and-jail-sandbox.md)
and the rebrand plan [`docs/freegrok-rebrand-plan.md`](docs/freegrok-rebrand-plan.md).

## Configuration

| Role | FreeGrok | grok-build compat |
|------|----------|-------------------|
| User home | `~/.freegrok` / `$FREEGROK_HOME` | copied from `~/.grok` / `$GROK_HOME` on first run |
| Env vars | `FREEGROK_*` preferred | `GROK_*` still read |
| Project dir | `.freegrok/` | copied from `.grok/` when dest is missing |
| System dir | `/etc/freegrok` | `/etc/grok` if freegrok dir is absent |

Skipped on migrate (regenerable / hostile): `downloads/`, `marketplace-cache/`,
`memtrace/`, `logs/`, `vendor/`, `sandbox-blocked-dir.*`.

Set `FREEGROK_NO_MIGRATE=1` to create an empty `~/.freegrok` instead of copying.

## Documentation

Full online documentation is available at
[docs.x.ai/build/overview](https://docs.x.ai/build/overview).

The user guide ships with the pager crate:
[`crates/codegen/xai-grok-pager/docs/user-guide/`](crates/codegen/xai-grok-pager/docs/user-guide/)
— getting started, keyboard shortcuts, slash commands, configuration, theming,
MCP servers, skills, plugins, hooks, headless mode, sandboxing, and more.

## Repository layout

| Path | Contents |
|------|----------|
| `crates/codegen/xai-grok-pager-bin` | Composition-root package; builds the `freegrok` binary |
| `crates/codegen/xai-grok-pager` | The TUI: scrollback, prompt, modals, rendering |
| `crates/codegen/xai-grok-shell` | Agent runtime + leader/stdio/headless entry points |
| `crates/codegen/xai-grok-tools` | Tool implementations (terminal, file edit, search, ...) |
| `crates/codegen/xai-grok-workspace` | Host filesystem, VCS, execution, checkpoints |
| `crates/codegen/...` | The rest of the CLI crate closure (config, MCP, markdown, sandbox, ...) |
| `crates/common/`, `crates/build/`, `prod/mc/` | Small shared leaf crates pulled in by the closure |
| `third_party/` | Vendored upstream source (Mermaid diagram stack) — see below |

> [!IMPORTANT]
> The root `Cargo.toml` (workspace members, dependency versions, lints,
> profiles) is **generated** — treat it as read-only. Prefer editing per-crate
> `Cargo.toml` files.

## Development

```sh
cargo check -p <crate>        # always target specific crates; full-workspace builds are slow
cargo test -p xai-grok-config # per-crate tests
cargo clippy -p <crate>       # lint config: clippy.toml at the repo root
cargo fmt --all               # rustfmt.toml at the repo root
```

## Contributing

> [!NOTE]
> External contributions are not accepted. See [`CONTRIBUTING.md`](CONTRIBUTING.md).

## License

First-party code in this repository is licensed under the **Apache License,
Version 2.0** — see [`LICENSE`](LICENSE).

Third-party and vendored code remains under its original licenses. See:

- [`THIRD-PARTY-NOTICES`](THIRD-PARTY-NOTICES) — crates.io / git dependencies,
  bundled UI themes, and **in-tree source ports** (including openai/codex and
  sst/opencode tool implementations)
- [`crates/codegen/xai-grok-tools/THIRD_PARTY_NOTICES.md`](crates/codegen/xai-grok-tools/THIRD_PARTY_NOTICES.md)
  — crate-local notice for the codex and opencode ports (license texts +
  Apache §4(b) change notice)
- [`third_party/NOTICE`](third_party/NOTICE) — vendored Mermaid-stack index
