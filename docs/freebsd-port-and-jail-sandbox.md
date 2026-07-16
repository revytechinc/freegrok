# FreeBSD Port + Jail Sandbox Plan — Grok Build (`grok`)

## Context

Upstream Grok Build targets **macOS / Linux / Windows**. This plan covers:

1. **What breaks** when packaging for FreeBSD (build, ports policy, feature gaps).
2. **Sandbox strategy:** implement FreeBSD isolation via the **jail system**, mapped onto the existing Linux **bubblewrap re-exec** model (not Landlock).
3. **Delivery:** store this plan on a **git branch** so it can be executed elsewhere.

**Bottom line:** Core TUI/agent can port with modest build fixes. Sandbox on FreeBSD should **not** wait for Landlock/Seatbelt — use **jails** as the enforcement backend, parallel to Linux `bwrap`.

---

## Recommended approach (sandbox)

### Why jails (not Landlock, not “do nothing”)

| Platform | Current mechanism | Shape |
|----------|-------------------|--------|
| macOS | Seatbelt via `nono` | In-process capability restriction |
| Linux | Landlock via `nono` + **`bwrap` re-exec** for deny bind-overs | Hybrid: kernel FS rules + namespace re-exec |
| FreeBSD (today) | Unsupported → warn and continue | No enforcement |
| FreeBSD (this plan) | **Jail re-exec** (bwrap analog) + optional child jail for tools | Process re-exec / child confinement |

Key existing API to mirror:

```text
crates/codegen/xai-grok-sandbox/src/lib.rs
  bwrap_reexec_command(deny_write, deny_read) -> Option<Command>
  is_inside_bwrap()  // env marker __GROK_INSIDE_BWRAP
  child_net::install_child_network_filter()  // Linux seccomp; FreeBSD: jail net params
```

Linux deny paths are **not** fully Landlock-enforced for all cases — they rely on **bwrap bind-over at re-exec**. FreeBSD jails fit that same “wrap the process at launch” shape better than trying to force `nono` onto BSD.

### Design: `jail_reexec_command` (bwrap twin)

Add FreeBSD-only path in `xai-grok-sandbox`:

```text
pub fn jail_reexec_command(deny_write: &[&str], deny_read: &[&str])
  -> Option<std::process::Command>
```

**Behavior (mirror bwrap):**

1. If already inside a grok jail (`__GROK_INSIDE_JAIL=1` or `security.jail.jailed`), return `None` (already confined).
2. Build a **ephemeral jail** that:
   - Starts from host root view (or a minimal nullfs tree — prefer thin jail / host-path jail pattern).
   - **RO-nullfs** (or equivalent) over each `deny_write` path.
   - Bind/nullfs a **mode-000 placeholder** over each `deny_read` path (same PID-suffixed placeholders under `~/.grok/` as bwrap).
   - Keeps host network **on** for the main process by default (agent needs LLM API — same as Landlock leaving net open at process level).
3. Re-exec current binary + argv inside the jail (`jail -c … exec` / `jail_set` + `exec`, or a small setuid helper — see privilege below).
4. Set env `__GROK_INSIDE_JAIL=1` (and treat `is_inside_bwrap()`-style helpers generically as `is_inside_os_sandbox()`).

**Profile mapping** (reuse existing `SandboxProfile`):

| Profile field | Jail enforcement |
|---------------|------------------|
| `read_write` / workspace | Default allow via full host bind (same as bwrap `--bind / /`) |
| `deny` write | nullfs RO or RO mount over path |
| `deny` read | placeholder bind-over (EPERM) |
| `restrict_network` | **Children only:** jail/VNET disable or `ip4=disable` for tool jails; main process keeps net for API |
| `ProfileName::Off` | No jail re-exec |

**Child network** (replaces seccomp on FreeBSD):

- Today: `child_net.rs` installs seccomp in `pre_exec` on Linux only.
- FreeBSD: spawn tool/shell children with `jail` params `ip4=disable` / `ip6=disable` (or inherited restricted jail) when `should_restrict_child_network()`.

### Privilege model (critical)

Unlike Linux user-namespace bwrap, **creating jails / nullfs mounts typically requires privilege**.

Pick one (recommended order):

1. **Privileged helper binary** (preferred for desktop CLI)  
   - e.g. `grok-jail-helper` installed setuid-root or with `capsicum`/mac privilege, or invoked via `doas`/`sudo` once.  
   - Helper only: create ephemeral jail + nullfs overlays + `exec` target; no general shell.  
   - Mirrors the “privileged overlay mount helper” pattern already used in `xai-fast-worktree` for CAP_SYS_ADMIN.

2. **Root-only / service mode**  
   - Document that sandbox profiles require root or `security.jail.*` delegated admin.  
   - Acceptable for servers; poor for laptop users.

3. **Degrade if unprivileged**  
   - If helper missing / `EPERM`, log `apply_failed` and continue **without** sandbox (same as unsupported nono today). Never crash startup.

**Do not claim** jail sandbox works unprivileged without the helper.

### Capsicum (optional later, not this plan’s primary)

Capsicum (`cap_enter`) is closer to Landlock for **in-process** restriction, but does not map cleanly to existing bwrap deny bind-overs and needs Casper for many ops. **Phase 1 = jails.** Capsicum can be a Phase 2 hardening layer inside the jail.

### nono / landlock on FreeBSD

- Keep `nono` behind `cfg` so FreeBSD **does not** pull Landlock as a hard dependency if it fails to compile.
- FreeBSD enforce path: **jail only**, not `Sandbox::apply` from nono.
- Feature flag: `sandbox-enforce` remains the user-facing switch; implementation dispatches:
  - linux → nono + bwrap  
  - macos → nono Seatbelt  
  - freebsd → jail re-exec + child jail net  

---

## Build / ports blockers (still required)

| Severity | Issue | Fix |
|----------|--------|-----|
| **P0** | `xai-grok-shell/build.rs` release ripgrep download rejects FreeBSD | Treat `freebsd` like `windows`: skip auto-bundle; depend on system `rg` |
| **P0** | Ports forbid network-at-build | No GitHub fetch in build scripts |
| **P0** | `bin/protoc` dotslash has no FreeBSD | `BUILD_DEPENDS=protobuf`; set `PROTOC` |
| **P1** | Git dep `nucleo` | Vendor or pin crates.io for ports |
| **P1** | Rust **1.92** + edition **2024** | Require modern `lang/rust` |
| **P2** | Auto-update `detect_platform` | Fail soft / disable on FreeBSD |
| **P2** | cgroups, overlay worktrees, power, crash PC/FP | Soft degradation (separate from jail work) |

### Soft degradation (unchanged)

- Fast worktrees: no `/proc` overlay/btrfs → copy/reflink fallback  
- System power: no-op  
- Crash handler: Unix signals OK; PC/FP extraction 0 on FreeBSD until implemented  
- Clipboard: `arboard`  
- SQLite network-FS probe: treat as local (NFS risk)  

---

## Implementation plan (ordered)

### Phase 0 — Branch + plan artifact (this session’s deliverable)

1. Create branch **`freebsd-port-plan`** (or `docs/freebsd-migration`) from current `main`.
2. Commit this document into the repo as e.g.  
   **`docs/freebsd-port-and-jail-sandbox.md`**  
   so it can be executed in another worktree/machine without relying on session storage.
3. Push only if the operator requests (do not push by default).

### Phase 1 — Compile on FreeBSD (no sandbox yet)

1. Patch `crates/codegen/xai-grok-shell/build.rs`:  
   `target_os == "freebsd"` → skip auto-download (same early-return pattern as Windows).
2. Document / port Makefile: `PROTOC`, `textproc/ripgrep` runtime dep.
3. Gate or fix `nono` so FreeBSD builds with `sandbox-enforce` (dispatch to jail stub that logs “not applied yet” until Phase 2).
4. `cargo check -p xai-grok-pager-bin` and release build offline.

### Phase 2 — Jail sandbox backend

**Critical files to touch:**

| Path | Change |
|------|--------|
| `crates/codegen/xai-grok-sandbox/src/lib.rs` | `jail_reexec_command`, env marker, dispatch from apply/startup |
| `crates/codegen/xai-grok-sandbox/src/profiles.rs` | FreeBSD path: resolve deny lists for jail (reuse `effective_deny_paths`, `partition_deny_entries`) |
| `crates/codegen/xai-grok-sandbox/src/deny/*` | Share bind-over / placeholder helpers with FreeBSD (today Linux-only in places) |
| `crates/codegen/xai-grok-sandbox/src/child_net.rs` | FreeBSD: no seccomp; document jail-based child net |
| Tool spawn sites (shell/terminal) | When `restrict_network`, spawn via jail helper |
| New: `crates/codegen/xai-grok-sandbox/src/jail.rs` (or `freebsd_jail.rs`) | Jail create/attach/exec, cleanup |
| Optional helper: `grok-jail-helper` binary crate | Privileged nullfs + jail_set |

**Reuse:**

- `bwrap_reexec_command` control flow and placeholder design  
- `SandboxProfile` / `ProfileName` / `load_sandbox_config`  
- `effective_deny_paths`, glob partition helpers  
- `SandboxEvent` logging / `apply_failed` metrics  

**API sketch:**

```rust
// lib.rs
pub fn is_inside_os_sandbox() -> bool {
    is_inside_bwrap() || is_inside_jail()
}

#[cfg(target_os = "freebsd")]
pub fn jail_reexec_command(deny_write: &[&str], deny_read: &[&str]) -> Option<Command>;

#[cfg(target_os = "freebsd")]
pub fn is_inside_jail() -> bool {
    std::env::var_os("__GROK_INSIDE_JAIL").is_some()
        || /* sysctl security.jail.jailed == 1 */
}
```

Startup (pager/shell entry): if FreeBSD + enforce + profile ≠ off →  
`jail_reexec_command(...).exec()` once, same call site pattern as bwrap.

### Phase 3 — FreeBSD port packaging

1. `USES=cargo`, vendor lockfile, patch `nucleo` if needed.  
2. `BUILD_DEPENDS`: rust ≥ pin, protobuf.  
3. `RUN_DEPENDS`: ripgrep; optional jail helper with careful setuid packaging notes.  
4. Install binary as `grok`.  
5. Disable or no-op auto-update for FreeBSD channels.  

### Phase 4 — Hardening (later)

- Capsicum inside jail  
- Crash-handler FreeBSD `ucontext` PC/FP  
- `statfs` network-FS detection for SQLite  
- Official FreeBSD binary channel for auto-update  

---

## Verification

### Build

```sh
export PROTOC=/usr/local/bin/protoc
cargo check -p xai-grok-pager-bin
cargo build -p xai-grok-pager-bin --release   # must not hit GitHub rg download
CARGO_NET_OFFLINE=true cargo build -p xai-grok-pager-bin --release  # after vendor
```

### Jail sandbox

1. Unprivileged without helper → starts, logs sandbox not applied / helper missing.  
2. With helper + profile `strict` / deny paths → re-exec; `__GROK_INSIDE_JAIL=1`; read/write denies observed (`EPERM` / RO).  
3. Profile `off` → no jail.  
4. Nested start → no double-jail.  
5. `restrict_network` tools → child cannot open TCP; parent still reaches API.  
6. Compare deny semantics against Linux bwrap e2e tests (`deny_paths_e2e` patterns).  

### Smoke

TUI launch, shell tool, file edit, fs watch, clipboard (X11 if present).

---

## Branch delivery (explicit)

| Step | Action |
|------|--------|
| 1 | `git checkout -b freebsd-port-plan` |
| 2 | Write `docs/freebsd-port-and-jail-sandbox.md` (this plan) |
| 3 | Commit with message describing FreeBSD port + jail sandbox plan |
| 4 | Leave unpushed unless asked |

This branch is the handoff artifact for execution on a FreeBSD host or another agent.

---

## Summary

| Question | Answer |
|----------|--------|
| Issues making a FreeBSD port? | **Yes** — P0 build (rg, protoc, offline), rust pin, git dep; many features soft-degrade. |
| Sandbox on FreeBSD? | **Use jails**, modeled on Linux **bwrap re-exec**, not Landlock/`nono`. |
| Privilege? | Need helper or root; degrade gracefully if unavailable. |
| Next concrete action after approval | Create branch `freebsd-port-plan`, commit plan under `docs/`, then implement Phase 1–2 on FreeBSD. |
