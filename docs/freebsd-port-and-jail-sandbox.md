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

---

## Context

Grok Build FreeBSD work lives on branch **`freebsd-port-plan`**. Phase 0–1a are done (authored on macOS). FreeBSD is not verified until **Phase V — Verify the platform** passes on a FreeBSD host.

**Policy:** Do not claim FreeBSD support, green FreeBSD CI, or jail sandbox readiness until Phase V succeeds **on FreeBSD**. Authoring on Linux/macOS is allowed; FreeBSD verification is not optional and is not portable.

---

## Status

| Item | Status |
|------|--------|
| Branch | `freebsd-port-plan` → `origin/freebsd-port-plan` |
| Phase 0 | Plan doc on branch |
| Phase 1a | FreeBSD rg skip, jail stubs, nono = linux\|macos only; macOS `cargo check` was green in a prior session |
| FreeBSD platform verify | **Not done** — waiting for an agent on FreeBSD |
| Ports collection (fork of freebsd/freebsd-ports) | Branch `wip/grok-build-port` on cloudbsd-ports; port skeleton TBD |

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

Phase 2 still needed: real `jail_reexec_command` / helper, nullfs denies, child net jails.

---

## Remaining phases

| Phase | Where to run | Work |
|-------|--------------|------|
| **V** | FreeBSD for pass; any host for V.1 | Platform verify gate |
| **1b** | FreeBSD | Fix compile breaks from V.3 |
| **2** | Author on any OS; **verify on FreeBSD** | Jail helper, denies, child net |
| **3** | Ports tree (any host can author Makefile); **build on FreeBSD** | See **Phase 3 — Ports collection** |
| **4** | Later | Capsicum, crash ucontext, update channel |

---

## Phase 3 — Ports collection (CloudBSD fork of FreeBSD ports)

### What the ports repo is

| Item | Value |
|------|--------|
| **Upstream (canonical FreeBSD ports)** | https://github.com/freebsd/freebsd-ports |
| **Working fork (push target)** | `git@github.com:cloudbsdorg/cloudbsd-ports.git` |
| Relationship | **cloudbsd-ports is a fork of freebsd/freebsd-ports** (layout, categories, `Mk/`, `USES=cargo`, etc. match FreeBSD ports) |
| Typical local path | `~/git/cloudbsd-ports` (operator preference; **not guaranteed on the next agent**) |
| Isolation branch | **`wip/grok-build-port`** (create if missing; never commit port work on `main` directly) |

Treat the tree as a normal FreeBSD ports tree. Prefer FreeBSD Handbook / Porter’s Handbook conventions. Sync/rebase from upstream FreeBSD when needed; land CloudBSD-specific work on the fork branch.

### Separate repo — do not assume checkout

Ports work is **not** inside the `grok-build` git tree.

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
# Track FreeBSD upstream for merges/rebases (read-only; push goes to origin/fork)
git remote add freebsd https://github.com/freebsd/freebsd-ports.git 2>/dev/null \
  || git remote set-url freebsd https://github.com/freebsd/freebsd-ports.git
git fetch origin
git fetch freebsd   # optional unless syncing with FreeBSD
git checkout main
git pull --ff-only origin main
git checkout -b wip/grok-build-port   # or: git checkout wip/grok-build-port if it exists
```

**If the tree already exists** but may be mid-clone (incomplete HEAD): wait until `git log -1` works, then create/checkout `wip/grok-build-port`.

**Remotes and branches to keep straight:**

| Repo | Remote name | Branch | Purpose |
|------|-------------|--------|---------|
| `grok-build` | (its origin) | `freebsd-port-plan` | Source fixes, jail stubs, this plan |
| `cloudbsdorg/cloudbsd-ports` | `origin` | `wip/grok-build-port` | Port Makefile / distinfo / pkg-descr (push here) |
| `freebsd/freebsd-ports` | `freebsd` (add locally) | `main` | Upstream FreeBSD ports — fetch/merge only, do not push |

Push ports work to **origin** (`cloudbsdorg/cloudbsd-ports`) on **`wip/grok-build-port`**. Do not mix into grok-build PRs. Do not push to `freebsd/freebsd-ports` unless you have explicit upstream commit rights and intent.

### Port design (Grok Build)

Intended category (adjust to match local ports layout / category policy):

- Likely: `devel/grok` or `devel/grok-build` (binary name ships as `grok`; artifact is `xai-grok-pager`)
- Follow existing **Rust / `USES=cargo`** ports in the tree as templates (search for `USES=cargo` under the ports root once cloned)

| Port piece | Guidance |
|------------|----------|
| **BUILD_DEPENDS** | Recent `lang/rust` (≥ pin / edition 2024), `devel/protobuf` (`protoc`) |
| **RUN_DEPENDS** | `textproc/ripgrep` — **do not** bundle/download `rg` in the port (see ripgrep section below) |
| **CARGO_BUILD** | Offline vendor / crates.io distfiles per CloudBSD/FreeBSD cargo port conventions |
| **Network at build** | Forbidden for ports — no GitHub ripgrep fetch, no live git deps if avoidable |
| **PROTOC** | `PROTOC=${LOCALBASE}/bin/protoc` in the Makefile |
| **rg bundle** | FreeBSD already skips auto-bundle in grok-build `build.rs`; rely on system `rg` |
| **git dep `nucleo`** | May need crate pin / vendor / ports cargo-crates list — fix in grok-build or port patches |
| **Sandbox** | Jail backend is stub; port can ship without privileged helper initially |
| **Install** | Install binary as `grok` (from `xai-grok-pager`) |

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

# 3) Ports tree = CloudBSD fork of https://github.com/freebsd/freebsd-ports
#    (may be absent — clone fork if needed)
if [ ! -d ~/git/cloudbsd-ports/.git ]; then
  mkdir -p ~/git && git clone git@github.com:cloudbsdorg/cloudbsd-ports.git ~/git/cloudbsd-ports
fi
cd ~/git/cloudbsd-ports
git rev-parse HEAD >/dev/null 2>&1 || { echo "ports clone incomplete"; exit 1; }
git remote add freebsd https://github.com/freebsd/freebsd-ports.git 2>/dev/null || true
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
| (prior) | Darwin arm64 | aarch64-apple-darwin | Authoring only | Phase 1a + docs; FreeBSD V **pending** |
| | | | | *Next agent: add a row* |
