# FreeBSD Port + Jail Sandbox Plan (with platform verification)

## Context

Grok Build FreeBSD work lives on branch **`freebsd-port-plan`** (pushed). Phase 0–1a are done on **macOS**. FreeBSD is not verified until a dedicated **Verify the platform** gate passes on a FreeBSD host (or a true FreeBSD target triple).

**Policy:** Do not claim FreeBSD support, green CI, or jail sandbox readiness until **Verify the platform** succeeds on FreeBSD. Authoring on macOS is allowed; verification is not.

---

## Status (as of last push)

| Item | Status |
|------|--------|
| Branch | `freebsd-port-plan` → `origin/freebsd-port-plan` |
| Phase 0 | Plan doc committed |
| Phase 1a | Done: FreeBSD rg skip, jail stubs, nono = linux\|macos only, macOS `cargo check` green |
| Commits | `cac2fac` (docs) · `0ea1408` (scaffolding) |
| FreeBSD platform verify | **Not done** |

**This host (macOS) snapshot:** Darwin arm64 · `rustc` host `aarch64-apple-darwin` · `protoc` + `rg` present via Homebrew. This satisfies **authoring host** checks only.

---

## Phase V — Verify the platform (required gate)

Run this **first** on any machine that will execute FreeBSD work, and **always** on a FreeBSD system before marking the port ready.

### V.1 Identify the host

```sh
uname -srm                    # expect FreeBSD <version> <arch> for real verify
rustc -vV                     # host: must be *-unknown-freebsd for native
echo "CARGO_CFG: $(rustc --print cfg | grep target_os)"
```

| Result | Meaning | Allowed work |
|--------|---------|--------------|
| `Darwin` / `apple-darwin` | Authoring host | Patch + macOS regression only |
| `Linux` | Authoring / CI | Same as macOS for FreeBSD claims |
| `FreeBSD` / `*-unknown-freebsd` | **Target platform** | Full verify + jail e2e |

Record: OS, version, arch (`amd64`/`arm64`), rustc version (≥ pin in `rust-toolchain.toml`, currently **1.92** channel intent).

### V.2 Toolchain and build deps (FreeBSD)

```sh
rustc --version               # >= workspace pin
protoc --version              # system protobuf; $PROTOC if non-default path
rg --version                  # textproc/ripgrep (no bundled rg on FreeBSD)
pkg info protobuf ripgrep rust 2>/dev/null || true
```

| Check | Pass criteria |
|-------|----------------|
| Rust | Builds edition 2024 / matches pin |
| protoc | On PATH or `PROTOC` set; `bin/protoc` dotslash **not** required |
| ripgrep | On PATH; release build does **not** hit GitHub rg download |
| Network at build | Offline/vendor path works for ports (when packaging) |

### V.3 Cargo platform compile

On FreeBSD (native preferred):

```sh
export PROTOC="${PROTOC:-$(command -v protoc)}"
cargo check -p xai-grok-sandbox -p xai-grok-pager-bin
cargo build -p xai-grok-pager-bin --release
# must not error: "Unsupported target for ripgrep bundling: freebsd-..."
```

| Check | Pass criteria |
|-------|----------------|
| `xai-grok-sandbox` | Compiles **without** `nono`/`landlock` |
| `jail` module | Linked on FreeBSD (`cfg(target_os = "freebsd")`) |
| Release build | Completes; uses system `rg` |
| macOS still green | On Darwin: `cargo check -p xai-grok-pager-bin` after patches |

Optional (from macOS only if FreeBSD sysroot/linker exists — usually **skip**):

```sh
rustup target add x86_64-unknown-freebsd   # incomplete without FreeBSD SDK
```

### V.4 Runtime platform behavior

```sh
# OS markers
sysctl security.jail.jailed                 # 0 outside jail, 1 inside
# After starting grok with a non-off sandbox profile (Phase 2):
# env should eventually set __GROK_INSIDE_JAIL when re-exec works
```

| Check | Pass criteria (Phase 1a) | Pass criteria (Phase 2) |
|-------|--------------------------|-------------------------|
| Sandbox apply without helper | Starts; logs jail not active; **no crash** | N/A |
| Sandbox apply with helper | N/A | Re-exec; `__GROK_INSIDE_JAIL=1`; denies hold |
| Child network restrict | N/A | Tool children cannot open TCP when profile restricts net |
| Auto-update | Soft-fail / unsupported OS (acceptable) | Same until FreeBSD artifacts exist |
| TUI smoke | Launch + shell tool + file edit | Same under jail |

### V.5 Platform matrix (record results)

| Capability | macOS (author) | FreeBSD (target) |
|------------|----------------|------------------|
| `cargo check` pager-bin | Required green | Required green |
| Landlock/Seatbelt (`nono`) | Yes | **No** (by design) |
| Jail re-exec | N/A | Required for sandbox parity |
| Bundled rg | Yes (download) | **No** — system `rg` |
| Bundled protoc dotslash | Often yes | **No** — system `protoc` |
| Official auto-update binary | Yes | No until channel exists |

Fill this table in `docs/freebsd-port-and-jail-sandbox.md` after each FreeBSD run (date, host, rustc, pass/fail).

### V.6 Fail the gate if

- Host is not FreeBSD but someone claims “FreeBSD verified”
- Release build tries to download rg or fails unsupported triple
- Sandbox panics on FreeBSD instead of degrade
- `nono`/`landlock` pulled in as FreeBSD deps (link/compile failure)

---

## Remaining phases (after V passes on FreeBSD)

| Phase | Where | Work |
|-------|--------|------|
| **1b** | FreeBSD | Fix any compile breaks found in V.3 |
| **2** | Author anywhere; verify FreeBSD | Jail helper, nullfs denies, child net jails |
| **3** | FreeBSD | Ports packaging (`USES=cargo`, deps) |
| **4** | Later | Capsicum, crash ucontext, update channel |

---

## Immediate next actions (this repo, after approval)

1. **Update** `docs/freebsd-port-and-jail-sandbox.md` with the full **Phase V — Verify the platform** section (and status table reflecting push already done).
2. **Commit and push** on `freebsd-port-plan` (user wants commit+push when plan work is done).
3. Do **not** run FreeBSD e2e on this Darwin host — document V results as “pending FreeBSD host”.

---

## Summary

| Question | Answer |
|----------|--------|
| Missing from plan before? | Explicit **Verify the platform** gate |
| What is it? | Host identity, deps, compile, runtime, matrix — FreeBSD required for pass |
| Phase 1a? | Done + pushed; does **not** replace platform verify |
| Next code change | Doc update + commit/push only (unless FreeBSD host available) |
