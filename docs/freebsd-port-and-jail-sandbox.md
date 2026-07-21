# FreeBSD Port + Jail Sandbox Plan (with platform verification)

## Handoff for the next agent

**Do not assume you are on macOS.** The previous session authored on Darwin; the next agent may be on **Linux, macOS, FreeBSD, CI, or anything else**.

1. **Start with Phase V.1 — identify your host** (`uname`, `rustc -vV`). That decides what you are allowed to claim and which steps to run.
2. Checkout **grok-build** branch **`freebsd-port-plan`** (tracks `origin/freebsd-port-plan`).
3. For the **ports collection** work, use a **separate** repo (see **Phase 3 — Ports collection**). Do **not** assume `~/git/cloudbsd-ports` exists on this machine — clone it if missing, always on an isolated git branch.
4. Read **Status** below, then run only the work your host allows (table in V.1).
5. When you change plan/code: **commit and push** the relevant branch when done (unless told not to).
6. **Never** mark FreeBSD “verified” unless V.1 says you are actually on FreeBSD (or a true `*-unknown-freebsd` native build with full V.2–V.4).

| Your host | You may | You must not |
|-----------|---------|--------------|
| **FreeBSD** | Full Phase V, 1b compile fixes, jail e2e, ports work | Skip V and claim green |
| **Linux** | Author patches, Linux regression (`cargo check`), optional CI | Claim FreeBSD verified; run FreeBSD-only sysctls as pass |
| **macOS** | Author patches, macOS regression | Claim FreeBSD verified |
| **Other / unknown** | Re-run V.1; treat as authoring unless FreeBSD | Invent platform support |

**Target product platform** for this workstream is **FreeBSD**. Authoring hosts only keep the tree buildable and implement `cfg(target_os = "freebsd")` safely.

### FreeBSD version baseline

**14, 15, and 16** (16 = CURRENT/dev). Ports: `OSVERSION >= 1400000`.

### Doctor CI gate (build)

```sh
# Must pass in regression + ports do-test (offline, critical-only, ≤15s):
target/release/xai-grok-pager doctor --ci
# cargo: doctor::tests::doctor_ci_gate_no_critical_failures
```

`--ci` ignores warnings (e.g. jail helper), fails only on critical. No network/auth.

---

## Context

Grok Build FreeBSD work lives on branch **`freebsd-port-plan`**. Phase 0–1a are done. **Phase V.1–V.3 compile/release gates passed** on FreeBSD 15.1 (`freedev007`, 2026-07-20). Jail helper (Phase 2) and full TUI interactive smoke under jail remain open.

**Policy:** Do not claim full FreeBSD product readiness or jail sandbox parity until Phase 2+ succeeds **on FreeBSD**. Compile-green FreeBSD is now recorded in the verification log.

---

## Status

| Item | Status |
|------|--------|
| Branch | `freebsd-port-plan` → `origin/freebsd-port-plan` |
| Phase 0 | Plan doc on branch |
| Phase 1a | FreeBSD rg skip (shell + tools), jail stubs, nono = linux\|macos only |
| Phase 1b | FreeBSD compile fixes landed (mid, sqlite-vec, workspace_classifier, apply_failed, support_info gates) |
| FreeBSD platform verify | **V.1–V.3 green** + full package test matrix re-green on FreeBSD 15.1 (2026-07-20). Jail helper still Phase 2 (discovery ready). |
| Ports collection | Native port design: `devel/grok-build` → package `grok-build-native`, binary **`grok-build`**. Leave **misc/grok-build** (Linux) alone. FreeBSD ports tree is manual submission only — do not auto-push ports. **`nucleo` path-vendored** (`third_party/nucleo`). Offline full `CARGO_CRATES` still open for poudriere. Local package install verified on freedev007. |

Update this table when you complete work (date, host OS, rustc, pass/fail).

---

## Phase V — Verify the platform (required gate)

**Every agent session that touches FreeBSD port work should start here**, regardless of OS.

### V.1 Identify the host (always first)

```sh
uname -srm
rustc -vV                     # look at "host:"
rustc --print cfg | grep target_os || true
hostname
date -u
```

| `uname -s` / rustc host | Role | Allowed work this session |
|-------------------------|------|---------------------------|
| `FreeBSD` / `*-unknown-freebsd` | **Target platform** | Full V.2–V.4, 1b, jail e2e, ports |
| `Linux` / `*-unknown-linux-*` | Authoring / CI | Patches + Linux `cargo check`; FreeBSD claims stay pending |
| `Darwin` / `*-apple-darwin` | Authoring | Patches + macOS `cargo check`; FreeBSD claims stay pending |
| Anything else | Unknown | Document host; author only until classified |

Record in your notes or a short commit: OS, version, arch, rustc version (workspace pin intent is **1.92+** / edition **2024** per `rust-toolchain.toml`).

### V.2 Toolchain and build deps

**On FreeBSD (target verify):**

```sh
rustc --version
protoc --version              # or: export PROTOC=/usr/local/bin/protoc
rg --version                  # pkg: textproc/ripgrep — no bundled rg on FreeBSD
pkg info protobuf ripgrep 2>/dev/null || true
```

**On Linux / macOS (authoring only):**

```sh
rustc --version
protoc --version || true      # needed for proto crates; install if missing
rg --version || true
# Linux: may auto-bundle rg in release builds (supported triple)
# macOS: same
# FreeBSD-specific: system protoc/rg required; bin/protoc dotslash has no FreeBSD asset
```

| Check | FreeBSD pass | Linux/macOS pass (authoring) |
|-------|--------------|------------------------------|
| Rust | Builds pin / edition 2024 | Same for local check |
| protoc | System or `$PROTOC` | System / brew / apt as available |
| ripgrep | On PATH; **no** GitHub bundle in release | Bundle OK on supported triples |
| Offline ports build | Required later for ports | N/A |

### V.3 Cargo compile

**FreeBSD (required for platform verify):**

```sh
export PROTOC="${PROTOC:-$(command -v protoc)}"
cargo check -p xai-grok-sandbox -p xai-grok-pager-bin
cargo build -p xai-grok-pager-bin --release
# must NOT error: Unsupported target for ripgrep bundling: freebsd-...
```

| Check | Pass criteria |
|-------|----------------|
| `xai-grok-sandbox` | Compiles **without** `nono`/`landlock` |
| `jail` module | Built under `cfg(target_os = "freebsd")` |
| Release | Completes using system `rg` |

**Linux or macOS (regression only — does not complete Phase V):**

```sh
cargo check -p xai-grok-sandbox -p xai-grok-pager-bin
# After FreeBSD-targeted patches, keep your host green
```

Cross-compile to FreeBSD from Linux/macOS usually needs a FreeBSD sysroot/linker — **do not treat partial cross as Phase V pass** unless fully linked and smoke-tested.

### V.4 Runtime (FreeBSD only for pass)

```sh
sysctl security.jail.jailed                 # 0 outside jail, 1 inside
# Phase 2: after jail re-exec, expect __GROK_INSIDE_JAIL=1
```

| Check | Phase 1a (current code) | Phase 2 (helper) |
|-------|-------------------------|------------------|
| Sandbox without helper | Starts; logs not active; **no crash** | — |
| Sandbox with helper | — | Re-exec; denies; marker set |
| Child network restrict | — | Jail net params when profile restricts |
| Auto-update | Soft-fail unsupported OS OK | Until FreeBSD artifacts exist |
| TUI smoke | Launch + shell + edit | Same under jail |

Skip V.4 on Linux/macOS (or only note “N/A — not FreeBSD”).

### V.5 Platform matrix (fill after runs)

| Capability | Linux (author) | macOS (author) | FreeBSD (target) |
|------------|----------------|----------------|------------------|
| `cargo check` pager-bin | Should stay green | Should stay green | **Required green** |
| Landlock / Seatbelt (`nono`) | Landlock | Seatbelt | **No** (jail instead) |
| Jail re-exec | N/A | N/A | Required for sandbox parity |
| Bundled rg in release | Often yes | Often yes | **No** — system `rg` |
| `bin/protoc` dotslash | Maybe | Maybe | **No** — system `protoc` |
| Official auto-update binary | Yes (linux) | Yes | Not until channel exists |
| **Phase V complete?** | No | No | Yes only if V.1–V.4 pass |

Append a short log under **Verification log** at the bottom of this file when you run checks.

### V.6 Fail the gate if

- Host is not FreeBSD but FreeBSD is claimed “verified”
- FreeBSD release build downloads rg or hits unsupported triple
- Sandbox panics on FreeBSD instead of degrading
- `nono`/`landlock` become FreeBSD compile dependencies

---

## What is already implemented (Phase 1a)

| Change | Path |
|--------|------|
| Skip rg auto-bundle on FreeBSD | `crates/codegen/xai-grok-shell/build.rs` |
| Jail stubs + markers + degrade `apply` | `crates/codegen/xai-grok-sandbox/src/jail.rs`, `lib.rs` |
| `nono` only on linux \| macos | `xai-grok-sandbox/Cargo.toml` + cfgs |
| `is_inside_os_sandbox()` | bwrap **or** jail |

Phase 2 still needed: real `jail_reexec_command` / helper, nullfs denies, optional local-only net.

### Jail provisioning (Phase 2) — TUI product scope

**Stay a coding TUI.** Do not grow into a FreeBSD control panel / mini OS installer.

| Path | When | Network | Privilege |
|------|------|---------|-----------|
| **Default: helper-only** | Want OS isolation for agent tools (bwrap analog) | **None** (console/`jexec`) | One-shot doas/sudo after short modal; refuse = keep working |
| **Advanced: `--full` base** | Optional persistent root | Local only | Same modal; userland ≤ host from download.freebsd.org |

**Do not** auto-download multi‑hundred‑MB bases on first launch. **Do not** require SSH, second user, or networking for the default path. **Do not** re-prompt after dismiss (config/env later).

**Host rule:** jail userland ≤ host (15.1 cannot run 16 CURRENT). Source: `download.freebsd.org` (ftp.freebsd.org redirects).

```sh
grok jail status              # offline
grok jail setup               # dry-run helper-only + privilege copy
grok jail catalog             # optional online list
grok jail setup --full        # advanced rootfs plan only
```

---

## Remaining phases

| Phase | Where to run | Work |
|-------|--------------|------|
| **V** | FreeBSD for pass; any host for V.1 | Platform verify gate |
| **1b** | FreeBSD | Fix compile breaks from V.3 |
| **2** | FreeBSD | Jail helper, base catalog, privilege modal, local-only provision, denies |
| **3** | Ports tree (any host can author Makefile); **build on FreeBSD** | See **Phase 3 — Ports collection** |
| **4** | Later | Capsicum, crash ucontext, update channel |

---

## Phase 3 — Ports collection

### Separate repo — do not assume checkout

Ports work is **not** inside the `grok-build` git tree. It lives in:

| Item | Value |
|------|--------|
| Remote | `git@github.com:cloudbsdorg/cloudbsd-ports.git` |
| Typical local path | `~/git/cloudbsd-ports` (operator preference; **not guaranteed**) |
| Isolation branch | **`wip/grok-build-port`** (create if missing; never commit port work on `main` directly) |

**If the ports tree is missing** (common for the next agent):

```sh
mkdir -p ~/git
cd ~/git
# Prefer not clobbering a partial clone:
if [ ! -d cloudbsd-ports/.git ]; then
  git clone git@github.com:cloudbsdorg/cloudbsd-ports.git cloudbsd-ports
elif ! git -C cloudbsd-ports rev-parse HEAD >/dev/null 2>&1; then
  echo "Partial clone at ~/git/cloudbsd-ports — wait for clone to finish or remove and re-clone"
  exit 1
fi
cd cloudbsd-ports
git fetch origin
git checkout main 2>/dev/null || git checkout master
git pull --ff-only
git checkout -b wip/grok-build-port   # or: git checkout wip/grok-build-port if it exists
```

**If the tree already exists** but may be mid-clone (incomplete HEAD): wait until `git log -1` works, then create/checkout `wip/grok-build-port`.

**Two remotes / two branches to keep straight:**

| Repo | Branch | Purpose |
|------|--------|---------|
| `grok-build` (this project) | `freebsd-port-plan` | Source fixes, jail stubs, this plan |
| `cloudbsdorg/cloudbsd-ports` | `wip/grok-build-port` | Port Makefile / distinfo / pkg-descr only |

Push ports work to **cloudbsd-ports** origin on `wip/grok-build-port` when ready (do not mix into grok-build PRs).

### Port design (Grok Build)

**Binary / package naming (decided 2026-07-20):**

| Item | Value | Why |
|------|--------|-----|
| Port path | `devel/grok-build` | Native FreeBSD source port |
| Package base | `grok-build-native` (`PKGNAMESUFFIX=-native`) | Coexists with **misc/grok-build** (Linux brand) |
| Installed binary | **`bin/grok-build`** | Cargo artifact is still `xai-grok-pager`; install renames. Do **not** install as `grok` |
| Leave alone | **misc/grok-build** | Official Linux brand binary (`bin/grok` + `bin/agent` via Linuxulator). Do not edit or conflict |
| Ports tree | FreeBSD ports is **read-only / manual submission** — author port files locally; do not auto-push ports; submit by hand |

Follow existing **Rust / `USES=cargo`** ports as templates (`textproc/ripgrep`, `devel/gitoxide`, …).

| Port piece | Guidance |
|------------|----------|
| **BUILD_DEPENDS** | Recent `lang/rust` (≥ pin / edition 2024), `devel/protobuf` (`protoc`) |
| **LIB_DEPENDS** | `audio/alsa-lib` (`libasound.so`) — linked by the release binary on FreeBSD |
| **RUN_DEPENDS** | `textproc/ripgrep` — **do not** bundle/download `rg` in the port (see ripgrep section below) |
| **CARGO_BUILD** | Offline vendor / crates.io distfiles per CloudBSD/FreeBSD cargo port conventions |
| **Network at build** | Forbidden for ports — no GitHub ripgrep fetch, no live git deps if avoidable |
| **PROTOC** | `PROTOC=${LOCALBASE}/bin/protoc` in the Makefile |
| **rg bundle** | FreeBSD already skips auto-bundle in grok-build `build.rs`; rely on system `rg` |
| **git dep `nucleo`** | May need crate pin / vendor / ports cargo-crates list — fix in grok-build or port patches (`CARGO_FREEBSD_PORTS_SKIP_GIT_UPDATE` blocks bare git deps) |
| **Sandbox** | Jail backend is stub; port can ship without privileged helper initially |
| **Install** | Install as **`grok-build`** (from `xai-grok-pager`). Completions rewrite `grok` → `grok-build` |
| **Local WIP** | `make GROK_SRC=/path/to/grok-build GROK_REUSE_TARGET=yes` until offline crates land |

### Ripgrep in the port

**Do not** try to download BurntSushi FreeBSD binaries (upstream does not ship them). Prefer:

1. `RUN_DEPENDS+= textproc/ripgrep` (or equivalent category path in this tree)
2. Runtime uses `rg` on `PATH` (`rg_path()` when not `bundle_rg`)
3. Optional: `GROK_SHELL_BUNDLE_RG_PATH=${LOCALBASE}/bin/rg` only if embedding a **local** package binary is desired (usually unnecessary)

### Scaffolding checklist (once branch exists)

1. Create port directory + `Makefile`, `pkg-descr`, `distinfo` / cargo crates list per tree conventions  
2. Point at a grok-build release tag or git commit that includes FreeBSD `build.rs` skip + sandbox gates  
3. Build **on FreeBSD** (`make` / poudriere) — authoring Makefile on Linux/macOS is fine; package build verify is FreeBSD  
4. Commit only under `wip/grok-build-port`  

---

## Suggested first commands for the next agent

```sh
# 1) Who am I?
uname -srm && rustc -vV

# 2) Grok-build plan branch
cd /path/to/grok-build   # or this worktree
git fetch origin && git checkout freebsd-port-plan && git pull --ff-only

# 3) Ports tree (may be absent — clone if needed)
if [ ! -d ~/git/cloudbsd-ports/.git ]; then
  mkdir -p ~/git && git clone git@github.com:cloudbsdorg/cloudbsd-ports.git ~/git/cloudbsd-ports
fi
cd ~/git/cloudbsd-ports
git rev-parse HEAD >/dev/null 2>&1 || { echo "ports clone incomplete"; exit 1; }
git checkout wip/grok-build-port 2>/dev/null || git checkout -b wip/grok-build-port

# 4a) If FreeBSD → full grok-build verify + later port build
# export PROTOC=...; cargo check -p xai-grok-sandbox -p xai-grok-pager-bin

# 4b) If Linux or macOS → author Makefile / patches only; stay green on host
# cargo check -p xai-grok-sandbox -p xai-grok-pager-bin
```

---

## Verification log

| Date (UTC) | Host | rustc host | Result | Notes |
|------------|------|------------|--------|-------|
| (prior) | Darwin arm64 | aarch64-apple-darwin | Authoring only | Phase 1a + docs |
| 2026-07-20 | FreeBSD 15.1-STABLE amd64 (`freedev007`) | x86_64-unknown-freebsd rustc 1.96.1 | **V.1–V.3 pass** | `cargo check -p xai-grok-sandbox -p xai-grok-pager-bin` green; `cargo test -p xai-grok-sandbox` 27+3+doctest ok; `cargo build -p xai-grok-pager-bin --release` → `grok 0.2.106`; system `rg`/`protoc`; no rg auto-bundle; `security.jail.jailed=0`; jail helper still Phase 2 |
| 2026-07-20 | FreeBSD 15.1 freedev007 | x86_64-unknown-freebsd 1.96.1 | **matrix 39/46 first pass; retests all green** | First pass fail/fix: telemetry, shell-base, fsnotify, pager, shell, workspace, gix-status. After fixes: telemetry 159, shell-base 67, fsnotify 113, gix 9, workspace 1475, sandbox 31+jail, pager 7364, shell 5726 — all pass. Ports skeleton pushed. Release `grok 0.2.106`. |
| 2026-07-21 | FreeBSD 15.1 freedev007 | x86_64-unknown-freebsd 1.96.1 | **port design locked** | Binary name **`grok-build`** (not `grok` / not `xai-grok-pager`). Package `grok-build-native` coexists with misc/grok-build Linux. FreeBSD ports = manual submission; do not push ports from agents. Stage blocked on git `nucleo` under ports cargo SKIP_GIT until crates vendored. |
| 2026-07-21 | FreeBSD 15.1 freedev007 | x86_64-unknown-freebsd 1.96.1 | **nucleo vendored + pkg install** | Path-vendored `third_party/nucleo` @ 5b74652 (no git dep). `make package` + install → `/usr/local/bin/grok-build` FreeBSD ELF; Linux `~/bin/grok` untouched. Completions + docs staged. Full offline `CARGO_CRATES` still TODO. |
| 2026-07-21 | FreeBSD 15.1 freedev007 | x86_64-unknown-freebsd 1.96.1 | **`grok doctor` D1** | Unprivileged offline self-test: binary brand, rg, fs, config, sandbox status. Jail missing helper → **warn** (EPERM without privilege; not critical). Never sudo. Opt-in `--sandbox-deep` skips without helper. |
