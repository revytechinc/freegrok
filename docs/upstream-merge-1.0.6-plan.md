# Upstream merge 1.0.6 — conflict plan

**Date:** 2026-08-19  
**Ours:** `eff2682` FreeGrok 1.0.1 (`revytechinc/freegrok`)  
**Theirs:** `upstream/main` `19d42e3` xAI Grok Build 1.0.6  
**Base:** `be71313` (2026-08-11)  
**Method:** merge (not rebase) — 55 local commits. Never push xAI.

Honcho: session `lessons-freegrok-upstream-2026-08-19`.

## Goal after merge

- Version **1.0.6**, binary **`freegrok`**, home **`~/.freegrok`** with copy from `~/.grok`
- Dual-read `FREEGROK_*` then `GROK_*`
- Jail helper + FreeBSD factory unchanged (upstream did not touch them)
- Ship: FreeBSD 15+16 `.pkg`, Ubuntu amd64 `.deb`, Ubuntu arm64 on `dgx.cloudbsd.org`

## TDD

1. **RED** — after merge, add `xai-grok-home` tests that fail on stock upstream:
   - `FREEGROK_HOME` wins over `GROK_HOME`
   - empty `FREEGROK_HOME` falls back to `GROK_HOME`
   - default dest `~/.freegrok`; copy `~/.grok` when dest missing
   - skip `downloads/`, `marketplace-cache/`, `memtrace/`, `logs/`, `vendor/`, `sandbox-blocked*`
   - no overwrite if dest exists
   - `FREEGROK_NO_MIGRATE` skips copy
2. **GREEN** — implement in `xai-grok-home`; re-export from `paths.rs`
3. **SURFACE** — `freegrok --version`, `doctor --ci` home=`~/.freegrok`, pkg/deb install

## Conflict table

| File | Upstream change | Ours | Resolution |
|------|-----------------|------|------------|
| `xai-grok-config/src/paths.rs` | Home logic moved to `xai-grok-home`; file is re-exports + sessions helpers | +497 lines copy/dual-read/tests | **Takeirs structure.** Keep session helpers. Re-export new home APIs. Move copy tests into `xai-grok-home`. |
| `xai-grok-home` (new) | `$GROK_HOME` / `~/.grok` only | n/a | **Port our resolver here.** `resolve_grok_home_from` must take FREEGROK + GROK + migrate flag. |
| `pager-bin/Cargo.toml` | version 1.0.6, bin still `xai-grok-pager` | bin `freegrok` | Auto-merge then **restore `[[bin]] name = "freegrok"`** and `default-run`. Version **1.0.6**. |
| `pager-bin/src/main.rs` | functional edits | brand strings, managed-install test uses `freegrok` | Keep theirs hunks; keep `version_text` = `freegrok {}`, `~/.freegrok` user strings, managed-install `bin/freegrok`. |
| `pager/src/app/cli.rs` | `full_version()`, memory override helpers | `name=freegrok` `about=FreeGrok` `Jail` cmd | Combine: **freegrok + Jail + full_version + memory helpers**. |
| `fsnotify/src/watcher.rs` | large rewrite (−4010 / +378) | 4× `#[ignore]` kqueue flakes | **Takeirs.** Re-apply ignore on surviving equivalent tests. |
| `pager/src/app/app_view.rs` | theirs | ours brand? | Takeirs; grep FreeGrok / Grok Build after. |
| `pager/docs/user-guide/README.md` | theirs | ours | Takeirs index; add FreeGrok clone note if missing. |

## Post-merge checklist (do not skip)

- [ ] `SOURCE_REV` = upstream `7d67dea…`
- [ ] `freegrok --version` starts with `freegrok 1.0.6`
- [ ] `cargo test -p xai-grok-home --lib`
- [ ] `cargo test -p xai-grok-config --lib paths`
- [ ] `cargo test -p xai-grok-sandbox --lib`
- [ ] Makefile `CARGO_BIN_NAME=freegrok` still
- [ ] scripts still look for `target/release/freegrok`
- [ ] doctor `fs.grok_home` uses `grok_home()` not hardcoded `~/.grok`
- [ ] Ubuntu `scripts/linux-deb.sh`
- [ ] pkg 007+008 + fleet 001–009 ABI-correct

## Fleet (2026-08-19)

| Host | ABI | Notes |
|------|-----|-------|
| 001, 002 | ? | SSH host-key **changed** — confirm before `known_hosts` update |
| 003 | FreeBSD:16 | has `grok-build-0.2.114` |
| 004 | ? | SSH timeout |
| 005, 006 | FreeBSD:16 | old `grok-build-0.2.106_2` |
| 007 | FreeBSD:15 | `freegrok-1.0.1` (build host) |
| 008 | FreeBSD:16 | `freegrok-1.0.1` (build host) |
| 009 | — | NXDOMAIN |

Ubuntu: this host 26.04 amd64; `dgx.cloudbsd.org` 24.04 arm64 (no rustc yet).
