# FreeBSD Port + Jail Sandbox Plan — Grok Build (`grok`)

## Context

Upstream Grok Build targets **macOS / Linux / Windows**. This plan covers:

1. **What breaks** when packaging for FreeBSD (build, ports policy, feature gaps).
2. **Sandbox strategy:** FreeBSD isolation via the **jail system**, mapped onto Linux **bubblewrap re-exec** (not Landlock).
3. **Delivery:** plan lives on branch **`freebsd-port-plan`** for execution elsewhere.

**Bottom line:** Core TUI/agent can port with modest build fixes. Sandbox on FreeBSD uses **jails**, parallel to Linux `bwrap`.

---

## Host constraint (current machine)

**We are on macOS right now** (`uname` = Darwin). This limits what can be verified here.

| Can do on this macOS host | Cannot do here (needs FreeBSD) |
|---------------------------|--------------------------------|
| Write/review patches behind `cfg(target_os = "freebsd")` | Real FreeBSD compile (`x86_64-unknown-freebsd` / native) |
| Patch `build.rs` FreeBSD skip for ripgrep (safe, pure cfg) | Jail create / nullfs / `jail_set` e2e |
| Keep macOS Seatbelt path working (`cargo check` on Darwin) | Ports tree / `USES=cargo` offline build |
| Update docs on `freebsd-port-plan` branch | `security.jail.jailed`, privileged helper smoke |
| Optional: add rustup target + cross-check **if** FreeBSD target + linker available (often incomplete without SDK) | Full release binary + TUI smoke on FreeBSD |

**Implication:** Implementation on macOS is **authoring + macOS regression check**. FreeBSD correctness is **handoff** (VM, remote FreeBSD box, or CI). Do not claim FreeBSD works until verified there.

### Phase 0 status (done on this Mac)

| Item | Status |
|------|--------|
| Branch `freebsd-port-plan` | Done (`cac2fac`) |
| `docs/freebsd-port-and-jail-sandbox.md` | Done |
| Push | Not pushed (unless requested) |

---

## Recommended approach (sandbox)

### Why jails (not Landlock, not “do nothing”)

| Platform | Mechanism | Shape |
|----------|-----------|--------|
| macOS | Seatbelt via `nono` | In-process |
| Linux | Landlock via `nono` + **`bwrap` re-exec** | Hybrid |
| FreeBSD (today) | Unsupported → warn | No enforcement |
| FreeBSD (this plan) | **Jail re-exec** + optional child jail net | bwrap analog |

Mirror:

```text
crates/codegen/xai-grok-sandbox/src/lib.rs
  bwrap_reexec_command(deny_write, deny_read) -> Option<Command>
  is_inside_bwrap()  // __GROK_INSIDE_BWRAP
  child_net::install_child_network_filter()  // Linux seccomp → FreeBSD: jail net params
```

### Design: `jail_reexec_command` (bwrap twin)

FreeBSD-only in `xai-grok-sandbox`:

1. Already jailed (`__GROK_INSIDE_JAIL` / `security.jail.jailed`) → `None`.
2. Ephemeral jail: host root view + RO nullfs for `deny_write` + mode-000 placeholder for `deny_read`.
3. Main process **keeps network** (LLM API); children may get `ip4=disable` when `restrict_network`.
4. Re-exec via helper or privileged path; degrade on `EPERM` (never crash startup).

**Privilege:** preferred `grok-jail-helper` (setuid/doas); else root/service; else log and continue unsandboxed.

**nono on FreeBSD:** do not rely on Landlock; gate `nono` if needed; `sandbox-enforce` dispatches:

- linux → nono + bwrap  
- macos → nono Seatbelt  
- freebsd → jail re-exec + child jail net  

---

## Build / ports blockers

| Severity | Issue | Fix |
|----------|--------|-----|
| **P0** | `build.rs` rg download rejects FreeBSD | Treat `freebsd` like `windows` |
| **P0** | Ports: no network-at-build | No GitHub fetch |
| **P0** | No FreeBSD in `bin/protoc` | System protobuf + `$PROTOC` |
| **P1** | Git dep `nucleo` | Vendor / crates.io for ports |
| **P1** | Rust 1.92 + edition 2024 | Modern `lang/rust` |
| **P2** | Auto-update platform | Fail soft on FreeBSD |
| **P2** | cgroups / overlay / power / crash PC | Soft degrade |

---

## Implementation (where each phase runs)

### Phase 0 — Branch + plan — **DONE (macOS)**

Handoff doc: `docs/freebsd-port-and-jail-sandbox.md` on `freebsd-port-plan`.

### Phase 1a — Prep patches — **macOS (this host)**

1. Patch `crates/codegen/xai-grok-shell/build.rs`: skip rg auto-bundle for `freebsd` (same as Windows).
2. Scaffold FreeBSD jail module behind `cfg(target_os = "freebsd")` (stubs OK): env marker, `is_inside_jail`, `jail_reexec_command` returning `None` / helper-missing path.
3. Ensure macOS still builds: `cargo check -p xai-grok-pager-bin`.
4. Expand plan doc with FreeBSD port dep notes (`PROTOC`, `ripgrep`).
5. Commit on `freebsd-port-plan` (still no push unless asked).

### Phase 1b — Compile for real — **FreeBSD host only**

1. `PROTOC=… cargo check/build -p xai-grok-pager-bin --release` offline.
2. Fix any FreeBSD-only compile breaks (`nono`/landlock, `pprof`, etc.).

### Phase 2 — Jail backend — **author on macOS (`cfg`), verify on FreeBSD**

| Path | Change |
|------|--------|
| `xai-grok-sandbox/src/lib.rs` | jail re-exec, markers, dispatch |
| `xai-grok-sandbox/src/jail.rs` (new) | FreeBSD jail create/exec/cleanup |
| `deny/*`, `profiles.rs` | Share deny plan with FreeBSD |
| `child_net.rs` + tool spawn | Jail net for restricted children |
| Optional `grok-jail-helper` | Privileged nullfs + jail_set |

### Phase 3 — Ports packaging — **FreeBSD**

`USES=cargo`, deps, install as `grok`, no official auto-update channel until binaries exist.

### Phase 4 — Hardening — later

Capsicum, crash ucontext, network-FS statfs, binary channel.

---

## Verification

### On macOS (this machine)

```sh
cargo check -p xai-grok-pager-bin
# optional: cargo test -p xai-grok-sandbox --lib
# Confirm build.rs still bundles rg for macos release (no regression)
```

### On FreeBSD (elsewhere)

```sh
export PROTOC=/usr/local/bin/protoc
cargo check -p xai-grok-pager-bin
cargo build -p xai-grok-pager-bin --release   # no GitHub rg download
# Jail e2e: helper missing → start OK; with helper → denies; nest → no double jail
```

---

## Summary

| Question | Answer |
|----------|--------|
| Issues on FreeBSD? | Yes — P0 build (rg, protoc, offline); soft feature gaps. |
| Sandbox? | **Jails**, bwrap-shaped; not Landlock. |
| Working host now? | **macOS** — author patches + keep Darwin green; FreeBSD verify elsewhere. |
| Phase 0? | **Done** on `freebsd-port-plan`. |
| Next after approval | Phase 1a on this Mac: `build.rs` FreeBSD skip + jail stubs + macOS check; hand off 1b/2 e2e. |
