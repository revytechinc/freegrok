# Plan: Reusable Path Browser + SSH Import Identity

**Status:** Proposed  
**Date:** 2026-07-21  
**Scope:** TUI path selection (including hidden dirs such as `~/.ssh`), wired first into SSH import; foundation for a full SSH host import flow.

---

## 1. Problem

Users importing over SSH often need an explicit **identity file** (`ssh -i`). Keys live under **`~/.ssh`**, a hidden directory.

The pager has no reusable file chooser that:

1. Opens as a **modal** (not prompt `@` completion or scrollback dump).
2. Can **list and enter** hidden directories (`.ssh`).
3. Selects a **file path** for a form field (identity, later config, MCP paths, etc.).

Existing surfaces are the wrong fit:

| Existing | Why it fails for identity pick |
|----------|--------------------------------|
| `@` file search | Workspace-scoped; hidden only via `@!`; bound to prompt, not forms |
| Project picker | Skips hidden dirs; project recents only |
| Dashboard location picker | Directory / cwd oriented; not a general file chooser |
| `default_ssh_key_candidates()` | Useful shortlist only; no browse of custom key names |

---

## 2. Goals

1. **Reusable** `PathBrowserModal` for any feature that needs “pick a path.”
2. **Hidden-aware:** show and enter `.ssh` (and other dotdirs) when configured.
3. **SSH identity UX:** form field = shortlist of known keys + **Browse…** → path browser.
4. **Consistent import UX:** multi-line results stay in report modals; short status in toasts only (existing import-menu rules).
5. **Test-first** (red/green) for browser listing, hidden toggle, and selection.

### Non-goals (this plan)

- Full remote apply/elect of findings into `~/.grok` (SSH import phase 2).
- Replacing `@` file search or the project picker.
- Native OS file dialogs (stay in-TUI for SSH/headless/FreeBSD).

---

## 3. Current foundation (do not reinvent)

Already in tree:

| Module | Role |
|--------|------|
| `xai_grok_shell::import::ssh` | `SshSession`, `SshAuth::with_identity`, OpenSSH config/agent, `default_ssh_key_candidates()`, `auth_diagnostics` |
| `import::remote_paths` / `findings` / `dedup` | Remote catalog + `FindReport` for later SSH scan |
| `import_menu_modal` / `import_report_modal` | Multi-source menu + scrollable report (no screen dump) |
| `config_cmd` `ssh-probe` | CLI auth debug |

**Missing:** path browser UI; SSH form; `scan_remote` orchestrator; menu row for SSH.

---

## 4. Design

### 4.1 `PathBrowserModal` (reusable)

**Location:** `crates/codegen/xai-grok-pager/src/views/path_browser_modal.rs`  
**Export:** `views/mod.rs`  
**Not** under `import_*` so non-import callers can depend on it cleanly.

#### Configuration

```rust
pub struct PathBrowserConfig {
    pub title: String,
    /// Initial directory (e.g. ~/.ssh). Expanded if starts with ~.
    pub start_dir: PathBuf,
    pub mode: PathBrowserMode,       // Files | Dirs | Both
    /// Show names starting with '.' in listings.
    pub show_hidden: bool,
    /// Allow '.' key to toggle show_hidden at runtime.
    pub allow_toggle_hidden: bool,
    /// Optional extension allowlist (None = any file).
    pub extensions: Option<Vec<String>>,
    /// Optional extra filter after mode/extensions.
    // kept as simple predicates in code, not dyn Fn in config if that hurts Send
    pub multi_select: bool,          // phase 1: false only
    pub max_entries_per_dir: usize,  // e.g. 500
}

pub enum PathBrowserMode {
    Files,
    Dirs,
    Both,
}
```

#### State and outcomes

```rust
pub struct PathBrowserModalState { /* cwd, entries, focus, scroll, config, filter_query */ }

pub enum PathBrowserOutcome {
    Selected(PathBuf),   // absolute, canonicalized when possible
    Cancelled,
    Changed,
    Unchanged,
}
```

#### Interaction

| Input | Action |
|-------|--------|
| ↑↓ / j k | Move focus |
| Enter | File → `Selected`; directory → chdir into |
| ← / - / Backspace | Parent directory |
| → / l | Enter focused directory |
| `.` | Toggle hidden (if `allow_toggle_hidden`) |
| `~` | Jump to home |
| Printable | Filter current listing (prefix) |
| Esc | `Cancelled` |
| Click row | Same as Enter |
| Scroll wheel | Scroll list |

#### Listing rules

1. `std::fs::read_dir` of current directory.
2. Sort: directories first, then case-insensitive name.
3. **Hidden:** entry name starts with `.` (Unicode-aware enough: first char is `.`).
4. If `show_hidden == false`, omit hidden **entries**, except:
   - Always allow navigation **out** of a hidden cwd (parent still works).
   - When `start_dir` itself is hidden (e.g. `~/.ssh`), open with `show_hidden: true` or treat “cwd is hidden” as force-show children of cwd.
5. Apply `mode` and `extensions` / filters.
6. Cap list length; show a trailing “truncated…” pseudo-row (non-selectable) if needed.
7. Symlinks: show; resolve on select with `dunce::canonicalize` when possible; do not infinite-loop (optional visited set for “enter”).

#### Rendering

- Shared **`ModalWindow`** chrome (title, Esc/close, footer shortcuts).
- Content: one line per entry (`📁 name/` / `📄 name`), focus highlight, optional filter hint.
- Footer: `↑↓ navigate · Enter open/select · . hidden · Esc cancel`.

#### App integration

```text
AppView.import_path_browser: Option<PathBrowserModalState>
  // or generic: path_browser: Option<PathBrowserModalState>

Action::OpenPathBrowser { purpose: PathBrowserPurpose, config: PathBrowserConfig }
Action::PathBrowserCancel
// Selection is handled by purpose-specific dispatch when outcome is Selected:
// e.g. PathBrowserPurpose::SshIdentity → write path into SSH form state
```

```rust
pub enum PathBrowserPurpose {
    SshIdentity,
    SshConfig,
    /// Future: McpConfigFile, PluginPath, ExportDest, …
    Generic,
}
```

Input/render priority: path browser above other import modals when open (same stack discipline as import report vs menu).

---

### 4.2 SSH form + identity field

**Location:** `views/ssh_import_form_modal.rs` (import-specific).

Fields (phase 1) — product model matches OpenSSH:

| Field | Required | Notes |
|-------|----------|--------|
| Host | Yes | Host alias, hostname, or `user@host` (same as `ssh <destination>`) |
| Login as | **No** | Empty = use `user@…` in Host, else `User` in `~/.ssh/config`, else OpenSSH default. **Never** prefilled with local OS user (local ≠ remote). Soft hint only in “Detected” panel. |
| Port | No | Default shown as `22`; blank = config Port |
| Identity | No | Path or empty = config/agent only; **only** browsable field |
| Password | No | Optional; keys-first; never log |

**Identity UI:**

1. **Quick list** from `default_ssh_key_candidates()` (+ optional `ssh -G` resolved `IdentityFile`s for the destination when cheap).
2. **Browse…** → `PathBrowserConfig`:
   - `title`: `"Choose SSH identity"`
   - `start_dir`: `~/.ssh` if it exists, else `~`
   - `mode`: `Files`
   - `show_hidden`: `true`
   - `allow_toggle_hidden`: `true`
   - Prefer private keys: filter can de-prioritize or hide `*.pub` (still allow select if user forces).
3. **Typed path** in the field (power users): expand `~`.

On Browse select: set identity field, close browser, return focus to form.

---

### 4.3 Import menu entry

Extend `ImportSource`:

```text
… existing …
Ssh,  // "SSH host… — scan remote Claude/OpenCode/Cursor configs"
```

Flow:

```
/import-menu → SSH host…
    → SshImportForm
        → [Scan] async remote find (phase 1B+)
        → Import report modal (FindReport::summary_text)
        → Identity Browse… → PathBrowser (this plan’s core UI)
```

Slash: `/import ssh` opens form; `/import ssh user@host` pre-fills destination.

---

### 4.4 Shell: remote scan (phase 1B, separate from browser)

New orchestrator (e.g. `import/remote_scan.rs`):

```text
scan_remote(session: &SshSession, opts) -> Result<FindReport, SshError>
```

1. ping / uname / home  
2. path catalog for OS  
3. path exists + bounded read  
4. product parsers → findings → dedup  

Runs off UI thread via `Effect::ScanRemoteImport` / `TaskResult::RemoteImportScanned`.

CLI optional: `grok config ssh-scan <dest>`.

---

## 5. Implementation phases

### Phase A — PathBrowser only (this plan’s hard dependency)

| Step | Work | Done when |
|------|------|-----------|
| A1 | Red tests: listing, hidden, enter `.ssh`, select file, Esc | Failing tests |
| A2 | Green: `path_browser_modal` + render/input on AppView | Tests pass |
| A3 | `PathBrowserPurpose` + open/cancel/select dispatch stubs | Manual open possible in test |

**Exit criteria:** Can programmatically open browser at `~/.ssh` with hidden on, select a file, get absolute path; no scrollback dump.

### Phase B — SSH form uses PathBrowser

| Step | Work | Done when |
|------|------|-----------|
| B1 | `SshImportFormModal` + menu row `ImportSource::Ssh` | Menu opens form |
| B2 | Identity shortlist + Browse → PathBrowser | Select identity path |
| B3 | Submit with `SshAuth::with_identity` into probe or scan | Keys used on connect |

**Exit criteria:** User can pick a non-default key under `~/.ssh` without typing the path.

### Phase C — Remote scan report

| Step | Work | Done when |
|------|------|-----------|
| C1 | `scan_remote` + unit tests (fake ssh) | Report data |
| C2 | Async effect + report modal | Professional UX |
| C3 | `/import ssh` + docs | Parity |

### Phase D — Apply findings (later)

Checklist elect → write local MCP/env/models; secret opt-in.

---

## 6. Testing strategy

### Path browser (required)

- `lists_non_hidden_by_default` (when `show_hidden: false`)
- `shows_dotdirs_when_show_hidden`
- `can_enter_dot_ssh_and_list_keys` (tempdir fixture with `.ssh/id_test`)
- `select_file_returns_absolute_path`
- `esc_cancels`
- `toggle_hidden_key` when allowed
- `filter_narrows_listing`

Use `tempfile` trees; never depend on the operator’s real `~/.ssh` in CI.

### SSH form (phase B)

- Browse purpose is `SshIdentity`
- Selected path lands in form state
- Cancel browser leaves form identity unchanged

### Regression UX

- No multi-line path lists in `startup_warnings` or scrollback system blocks.

---

## 7. Security and safety

- Browser only reads **local** filesystem (identity file paths); does not exfiltrate key material.
- Never log private key contents; path strings OK in debug at `debug!` only if needed.
- Password fields (SSH form) cleared after scan success/failure; never written to report body.
- Prefer `BatchMode` when no password (existing `SshAuth::keys_and_config`).
- Document: user-chosen identity is passed as OpenSSH `-i` only.

---

## 8. Documentation

| Doc | Update |
|-----|--------|
| `docs/user-guide/04-slash-commands.md` | `/import-menu` SSH row; `/import ssh` |
| `docs/plans/…` (this file) | Status → In progress / Done |
| FreeBSD README note | OpenSSH client required for SSH import |

---

## 9. Success criteria (phase A + B)

1. Reusable `PathBrowserModal` lives outside import-specific modules.  
2. With `show_hidden: true`, user can open `~/.ssh` and select an identity file.  
3. SSH form identity field supports shortlist + Browse + optional typed path.  
4. Selection returns a real filesystem path usable by `SshAuth::with_identity`.  
5. Unit tests cover hidden listing without requiring a real SSH host.  
6. UX stays modal-based (no random full-screen dumps).

---

## 10. Open decisions (lock before Phase B code)

| # | Decision | Default proposal |
|---|----------|------------------|
| 1 | Hide `*.pub` in identity browser? | Soft-hide (filter) with toggle or show all files |
| 2 | Multi identity files (`-i` repeated)? | Phase 1 single path only |
| 3 | Start dir if `~/.ssh` missing? | `~` with `show_hidden: true` |
| 4 | Share AppView slot name | `path_browser: Option<PathBrowserModalState>` |

---

## 11. Suggested implementation order

```
Phase A (PathBrowser reusable)
    → Phase B (SSH form + identity Browse)
        → Phase C (scan_remote + report)
            → Phase D (apply findings)
```

Do **not** block PathBrowser on remote scan: identity browse is useful for `ssh-probe` and form even before remote find ships.

---

## 12. References

- Shell SSH: `crates/codegen/xai-grok-shell/src/import/ssh.rs`  
- Remote paths / findings: `import/remote_paths.rs`, `import/findings.rs`  
- Import menu / report: `xai-grok-pager/src/views/import_*_modal.rs`  
- Prior discussion: import menu always-on, report modal for OpenCode (no banner dump), `grok-build-static` for live sessions during regression  
