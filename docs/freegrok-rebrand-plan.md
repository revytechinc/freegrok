# FreeGrok rebrand plan

**Status:** planning (not started)  
**Audience:** CloudBSD / RevyTech maintainers  
**Out of scope for this plan:** pushing anything to xAI upstream  

---

## 1. Goals

| Goal | Meaning |
|------|---------|
| **Product brand** | Ship as **FreeGrok** (user-facing: “FreeGrok”, short “freegrok”) |
| **Binary names fit** | All installed CLI names use the freegrok family (no `grok` / `grok-build` as *primary* names) |
| **Repo rename later** | GitHub/GitLab repo will eventually become `freegrok` (or `freegrok-cli`); plan for it without requiring it on day one |
| **Safe migration** | Existing `~/.grok` configs, scripts, and ports users keep working through a deprecation window |
| **Dual org** | RevyTech + CloudBSD stay interchangeable; no xAI push |

Non-goals (v1 rebrand):

- Renaming every internal Rust crate (`xai-grok-*`) in the first PR (optional later phase; high churn).
- Changing LLM API product names / third-party “Grok” model IDs (those are model catalog strings, not this CLI brand).
- Claiming FreeGrok is an official xAI product.

---

## 2. Naming table (decision record)

Lock these before coding. Adjust only with an explicit plan revision.

### 2.1 User-facing

| Role | Current | FreeGrok (proposed) |
|------|---------|---------------------|
| Product name | Grok Build / grok | **FreeGrok** |
| CLI help / clap name | `grok` / “Grok Build” | **`freegrok`** / “FreeGrok” |
| Version line | `grok 1.0.1 (rev) [channel]` | **`freegrok 1.0.1 (rev) [channel]`** |
| Docs title | Grok Build | FreeGrok |
| Doctor binary brand | freebsd-elf + path | same checks; path must end with freegrok* |

### 2.2 Installed binaries (FreeBSD / make install / Linux packages)

| Role | Current | FreeGrok (proposed) | Notes |
|------|---------|---------------------|--------|
| Primary CLI | `grok-build` | **`freegrok`** | One primary name; drop “-build” suffix for product clarity |
| Session-safe twin | `grok-build-static` | **`freegrok-static`** | Full second copy (not symlink); same reason as today (pkill by name) |
| Short alias | `grok` → grok-build | **`fg`** → freegrok **or** keep **`grok`** as deprecated symlink for one major | Prefer **`fg`** as the short FreeGrok alias; keep **`grok`** as *compat symlink* for one release then drop |
| Cargo release artifact | `xai-grok-pager` | **`freegrok`** (bin name) | Package crate can stay `xai-grok-pager-bin` short-term; bin name must change |

**Recommendation:** primary = `freegrok`, twin = `freegrok-static`, short = `fg`, compat = `grok` + `grok-build` → freegrok for **one** `PORTREVISION`/semver minor, then remove.

### 2.3 Paths and env

| Role | Current | FreeGrok (proposed) |
|------|---------|---------------------|
| User home | `~/.grok` / `$GROK_HOME` | **`~/.freegrok`** / **`$FREEGROK_HOME`** |
| System config | `/etc/grok` | **`/etc/freegrok`** |
| Project dir | `.grok/` in repo | **`.freegrok/`** (also still honor `.grok/` for one major) |
| Docs install | `share/doc/grok-build` | **`share/doc/freegrok`** |
| Completions | bash/zsh/fish for `grok` | for **`freegrok`** (and `fg` if shipped) |

**Env migration rule:**

1. Prefer `FREEGROK_*` when set.
2. Else fall back to `GROK_*` (and `GROK_HOME`) for the deprecation window.
3. Document only `FREEGROK_*` in new docs; keep a “compat” appendix listing `GROK_*`.

There are **100+** `GROK_*` env vars in-tree. Do **not** rename every identifier in one PR. Introduce a single env helper:

```text
fn env_var(name_without_prefix: &str) -> Option<String>
// looks up FREEGROK_{name} then GROK_{name}
```

and migrate call sites gradually (or wrap once in `xai-grok-env` / paths).

### 2.4 Packages / ports

| Role | Current | FreeGrok (proposed) |
|------|---------|---------------------|
| FreeBSD PORTNAME | `grok-build` | **`freegrok`** |
| FreeBSD origin | `devel/grok-build` | **`devel/freegrok`** (new port; old port CONFLICTS + DEPRECATED) |
| pkg name | `grok-build-0.2.106_N` | **`freegrok-<ver>`** |
| PLIST bins | grok-build, grok-build-static, grok | freegrok, freegrok-static, fg (+ optional grok compat) |
| GROK_SRC make var | `GROK_SRC` | **`FREEGROK_SRC`** (accept `GROK_SRC` as alias in port Makefile) |
| Prebuilt path | `target/release/xai-grok-pager` | `target/release/freegrok` |

### 2.5 Git remotes / repo rename (later)

| Current | Eventually |
|---------|------------|
| `revytechinc/grok-build` | `revytechinc/freegrok` (or keep grok-build as archive redirect) |
| Branches `freebsd-port-plan`, `main` | unchanged semantics |
| CloudBSD ports only | stays on `cloudbsdorg/cloudbsd-ports` |

**Day one:** no GitHub rename required.  
**When renaming:** GitHub “Rename repository” + update remotes on freedev007/008, CI, ports `WWW=`, docs clone URLs. Keep old URL as GitHub redirect.

### 2.6 Internal crates (optional / late)

| Current | FreeGrok options |
|---------|------------------|
| `xai-grok-pager-bin` etc. (41 `xai-grok-*` crates) | **Phase A (default):** leave crate directory names; only change **bin name** + **user strings**. |
| | **Phase B (optional):** rename to `freegrok-*` / drop `xai-` prefix; huge PR, breaks external path deps. |

**Recommendation:** Phase A only until FreeGrok is stable on FreeBSD + Linux; Phase B only if packaging or legal needs demand it.

---

## 3. Inventory (what must change)

### 3.1 High impact (must for “binary names fit”)

| Area | Paths / notes |
|------|----------------|
| Cargo bin | `crates/codegen/xai-grok-pager-bin/Cargo.toml` (`[[bin]] name`, `default-run`) |
| Clap / UI brand | `xai-grok-pager-bin/src/main.rs`, shell CLI banner, doctor labels |
| Version display | `xai-grok-version`, update channel strings |
| Root Makefile | `BINNAME`, `DOCDIR`, completion sed, help text |
| FreeBSD port | `cloudbsd-ports` `devel/grok-build` → `devel/freegrok` |
| Install scripts | `scripts/freebsd-*.sh`, ports README |
| Completions | generated from binary name |

### 3.2 Config / identity (must for product rebrand)

| Area | Paths / notes |
|------|----------------|
| Home resolution | `xai-grok-config` / `xai-grok-paths` (`default_grok_home`, `system_config_dir`, `grok_application`) |
| Env dual-read | all `GROK_*` via helper (or central env crate) |
| Project markers | `.grok/` discovery, AGENTS.md loaders if path-hardcoded |
| Doctor | binary brand / path identity checks for `freegrok` |
| Sandbox markers | `__GROK_INSIDE_JAIL` / bwrap env — either rename to `__FREEGROK_*` with dual-check or leave internal |

### 3.3 Docs & packaging copy

| Area | Notes |
|------|--------|
| User guide | `crates/codegen/xai-grok-pager/docs/user-guide/*` |
| FreeBSD plan | `docs/freebsd-port-and-jail-sandbox.md` |
| This plan | `docs/freegrok-rebrand-plan.md` |
| Port pkg-descr / pkg-message | CloudBSD ports |
| README / CONTRIBUTING | root |

### 3.4 Tests & regression

| Area | Notes |
|------|--------|
| Doctor unit tests | expect “freegrok” / jail backend labels |
| PTY / e2e harness | binary path assumptions |
| `scripts/freebsd-pkg-regression.sh` | `pkg info freegrok`, `pkg which /usr/local/bin/freegrok` |
| Installed regression | make install BINNAME |

### 3.5 Do not rename (v1)

- Model IDs containing “grok” in `default_models.json` (API models ≠ CLI brand).
- Upstream-compatible wire formats that embed historical names (document if any).
- Third-party crate names under `third_party/`.

---

## 4. Compatibility strategy

### 4.1 One-release deprecation window

| Old | Behavior during window |
|-----|------------------------|
| `~/.grok` | If `~/.freegrok` missing, **use** `~/.grok` and print one-time notice; optional auto-migrate (copy or rename) behind flag |
| `$GROK_HOME` | Honored if `$FREEGROK_HOME` unset |
| `GROK_*` env | Dual-read after `FREEGROK_*` |
| Binary `grok` / `grok-build` | Symlink or second install name → freegrok |
| Port `devel/grok-build` | `DEPRECATED` + `EXPIRATION` pointing to `devel/freegrok` |

### 4.2 Migration helper (optional but recommended)

```text
freegrok migrate-home
  # copies ~/.grok → ~/.freegrok if dest empty
  # does not delete old dir unless --delete-old
```

Doctor check: `config.home_brand` info if still on legacy path.

### 4.3 Sandbox / jail env markers

Prefer **dual accept** for one release:

- Write new: `__FREEGROK_INSIDE_JAIL`
- Read: either `__FREEGROK_INSIDE_JAIL` or `__GROK_INSIDE_JAIL`

Same for helper env `GROK_JAIL_HELPER` → `FREEGROK_JAIL_HELPER`.

---

## 5. Phased execution

### Phase 0 — Lock decisions (this doc)

- [x] Draft naming table  
- [ ] Maintainer ACK on primary binary (`freegrok` vs `freegrok-build`)  
- [ ] ACK on short alias (`fg` vs keep `grok`)  
- [ ] ACK on home dir (`~/.freegrok` + migrate)  
- [ ] ACK on crate renames deferred  

**Exit:** decisions frozen in §2.

### Phase 1 — Binary + user strings (smallest ship unit)

1. Change `[[bin]] name` / `default-run` to `freegrok`.
2. Clap command name, about, version banner → FreeGrok.
3. Makefile `BINNAME=freegrok`, docs dir, completions sed.
4. Update doctor binary identity for freegrok path.
5. Linux: `cargo build -p xai-grok-pager-bin --release` produces `target/release/freegrok`.
6. Unit tests for version/doctor brand.

**Exit:** local `./target/release/freegrok --version` and `doctor --ci` green (Linux authoring host).

### Phase 2 — Config home dual-stack

1. `default_freegrok_home()` → `~/.freegrok`.
2. Resolution order: `FREEGROK_HOME` → `GROK_HOME` → `~/.freegrok` if exists → else `~/.grok` if exists → else create `~/.freegrok`.
3. System dir: `/etc/freegrok` then `/etc/grok`.
4. Project `.freegrok/` then `.grok/`.
5. Tests for each precedence.

**Exit:** existing `~/.grok` users keep working; new installs use `~/.freegrok`.

### Phase 3 — Env dual-read

1. Central helper in paths/env crate.
2. Migrate high-traffic call sites (HOME, API keys, binary paths) first.
3. Grep gate in CI: new code must not introduce bare `std::env::var("GROK_` without helper (optional lint later).

**Exit:** documented env matrix; smoke with both prefixes.

### Phase 4 — FreeBSD ports + pkg (product truth)

1. Add **`devel/freegrok`** port (new directory):  
   - `PORTNAME=freegrok`  
   - install `freegrok`, `freegrok-static`, `fg`  
   - optional `CONFLICTS_INSTALL=grok-build`  
2. Mark **`devel/grok-build`** `DEPRECATED` + move users.  
3. `FORCE_CARGO_BUILD=yes package` on freedev007 from revytech tip.  
4. `pkg install -U` freegrok package.  
5. Update `scripts/freebsd-pkg-regression.sh` for freegrok.  
6. Re-run doctor --ci + pkg which + L5/L6 as required.

**Exit:** installed product is `/usr/local/bin/freegrok` owned by package `freegrok-*`.

### Phase 5 — Docs, scripts, CloudBSD/RevyTech hygiene

1. User guide global replace of product strings (careful with model names).  
2. FreeBSD port plan + this plan status → “in progress / done”.  
3. Completions, man pages, pkg-message.  
4. Push to **revytechinc** + **cloudbsdorg** only.

### Phase 6 — Repo rename (when ready)

1. Rename GitHub `revytechinc/grok-build` → `revytechinc/freegrok`.  
2. Update remotes on all build hosts; keep GitHub redirect.  
3. Update port `WWW=`, clone URLs in docs.  
4. Optional: mirror or archive notes for CloudBSD.

### Phase 7 — Remove compat (later major)

1. Drop `GROK_*` dual-read (or hide behind `FREEGROK_LEGACY=1`).  
2. Drop `grok` / `grok-build` install names.  
3. Drop `~/.grok` fallback.  
4. Expire old FreeBSD port.

---

## 6. Suggested first PR slice (after ACK)

Single PR titled roughly:

`feat(brand): FreeGrok binary name and user-facing strings (compat installs later)`

Scope:

- Bin name `freegrok`  
- Clap / version strings  
- Makefile `BINNAME`  
- Doctor path check  
- Minimal docs note “formerly Grok Build”  

Not in first PR: full `~/.freegrok` migration, ports rename, crate directory renames.

---

## 7. Risk register

| Risk | Mitigation |
|------|------------|
| Users’ scripts call `grok` | Compat symlink for one release; announce in pkg-message |
| Config “loss” if only `~/.freegrok` created empty | Prefer existing `~/.grok` until migrate |
| Ports name conflict with future official “grok” | Use `freegrok` PORTNAME; distinct from Linux misc/grok-build |
| Mass env rename breaks CI | Dual-read helper; no big-bang |
| Internal crate rename breaks local paths / patches | Defer Phase B |
| Accidental xAI push | Keep push URL disabled; only revytech/cloudbsd remotes |
| Binary twin / pkill story | Keep `freegrok-static` full copy |

---

## 8. Success criteria (done when)

1. **Primary installed name** is `freegrok` (pkg which on FreeBSD).  
2. **`freegrok doctor --ci`** green; sandbox backend still `jail` on FreeBSD.  
3. **Version string** says freegrok / FreeGrok, not “Grok Build” as product title.  
4. **New install** defaults to `~/.freegrok` (or documented migrate).  
5. **Legacy** `~/.grok` + `GROK_HOME` still work during deprecation.  
6. **Ports** `devel/freegrok` builds with `FORCE_CARGO_BUILD` and installs via `pkg`.  
7. **Remotes** only CloudBSD/RevyTech updated; xAI untouched.  
8. **Repo rename** either done or explicitly scheduled with redirect.

---

## 9. Open questions for maintainer ACK

1. Primary binary: **`freegrok`** (recommended) vs `freegrok-build`?  
2. Short alias: **`fg`** only, or also keep **`grok`** for a full major?  
3. Home dir: **auto-migrate** `~/.grok` → `~/.freegrok` on first run, or manual `freegrok migrate-home` only?  
4. Should **`grok-build-static`** pattern stay (`freegrok-static`) or is a single binary enough on FreeBSD?  
5. Crate renames (`xai-grok-*` → `freegrok-*`): **defer** (recommended) or required for legal/branding?  
6. Target GitHub name: **`freegrok`** vs **`freegrok-cli`** vs keep **`grok-build`** with product-only rename?

---

## 10. Work log

| Date | Note |
|------|------|
| 2026-08-11 | Plan authored after FreeBSD ports `grok-build-0.2.106_5` + revytech `88f8d5d` baseline. Implementation not started. |
