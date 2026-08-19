//! Single source of truth for the grok / FreeGrok home directory.
//!
//! Resolution: `$FREEGROK_HOME` → `$GROK_HOME` → `~/.freegrok` (copying
//! `~/.grok` into it when dest is missing). Shared by `xai-grok-config`
//! and `xai-fast-worktree`.
//!
//! Which function to call:
//! - [`grok_home`]: the usual choice, a cached, created path to build on.
//! - [`user_grok_home`]: `None` instead of a cwd fallback when no home resolves.
//! - [`default_grok_home`]: the `<home>/.grok` legacy default (copy source).
//! - [`default_freegrok_home`]: the `<home>/.freegrok` product default.
//! - [`resolve_grok_home`]: a fresh, uncached resolve.
//!
//! TODO: collapse these getters by threading the path through config as an
//! explicit value.

#[cfg(test)]
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Directory names under a legacy `~/.grok` tree that must not be copied into
/// `~/.freegrok` (regenerable caches, hostile sandbox fixtures, or huge blobs).
const MIGRATE_SKIP_DIR_NAMES: &[&str] = &[
    "downloads",
    "marketplace-cache",
    "memtrace",
    "logs",
    "vendor",
];

/// Marker written into a copied FreeGrok home so we can tell a tree was
/// migrated from grok-build rather than created empty.
pub const MIGRATED_FROM_GROK_MARKER: &str = ".migrated-from-grok";

/// Result of resolving the per-user config home (testable; no process env).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HomeResolution {
    pub path: PathBuf,
    pub copied_from: Option<PathBuf>,
}

fn home_join(home: &Path, leaf: &str) -> PathBuf {
    dunce::canonicalize(home)
        .unwrap_or_else(|_| home.to_path_buf())
        .join(leaf)
}

/// `<home>/.grok`, canonicalized via `dunce` (not `std::fs::canonicalize`,
/// which yields Windows `\\?\` verbatim paths).
fn grok_home_in(home: &Path) -> PathBuf {
    home_join(home, ".grok")
}

fn freegrok_home_in(home: &Path) -> PathBuf {
    home_join(home, ".freegrok")
}

fn should_skip_migrate_name(name: &str) -> bool {
    MIGRATE_SKIP_DIR_NAMES.contains(&name) || name.starts_with("sandbox-blocked")
}

fn staging_path(dest: &Path) -> PathBuf {
    let mut s = dest.as_os_str().to_os_string();
    s.push(".migrating");
    PathBuf::from(s)
}

/// Recursively copy `src` → `dest` for a grok-build → FreeGrok tree migrate.
///
/// Returns `Ok(true)` when a copy was performed, `Ok(false)` when dest already
/// exists or src is missing. Skips cache/hostile directory names.
pub fn copy_grok_tree(src: &Path, dest: &Path) -> std::io::Result<bool> {
    if dest.exists() {
        return Ok(false);
    }
    if !src.exists() {
        return Ok(false);
    }
    let staging = staging_path(dest);
    if staging.exists() {
        let _ = std::fs::remove_dir_all(&staging);
    }
    copy_tree_filtered(src, &staging)?;
    std::fs::write(
        staging.join(MIGRATED_FROM_GROK_MARKER),
        format!("source={}\n", src.display()),
    )?;
    match std::fs::rename(&staging, dest) {
        Ok(()) => Ok(true),
        Err(e) => {
            if dest.exists() {
                let _ = std::fs::remove_dir_all(&staging);
                Ok(false)
            } else {
                let _ = std::fs::remove_dir_all(&staging);
                Err(e)
            }
        }
    }
}

fn copy_tree_filtered(src: &Path, dest: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dest)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if should_skip_migrate_name(&name_str) {
            continue;
        }
        let from = entry.path();
        let to = dest.join(&name);
        let ft = entry.file_type()?;
        if ft.is_dir() {
            copy_tree_filtered(&from, &to)?;
        } else if ft.is_symlink() {
            #[cfg(unix)]
            {
                let target = std::fs::read_link(&from)?;
                let _ = std::os::unix::fs::symlink(target, &to);
            }
            #[cfg(not(unix))]
            {
                if from.is_dir() {
                    copy_tree_filtered(&from, &to)?;
                } else {
                    std::fs::copy(&from, &to)?;
                }
            }
        } else {
            std::fs::copy(&from, &to)?;
            #[cfg(unix)]
            {
                if let Ok(meta) = std::fs::metadata(&from) {
                    let _ = std::fs::set_permissions(&to, meta.permissions());
                }
            }
        }
    }
    Ok(())
}

/// Resolve the user config home from explicit env + default paths.
///
/// Precedence:
/// 1. `freegrok_home` (`FREEGROK_HOME`)
/// 2. `grok_home` (`GROK_HOME`)
/// 3. `default_fg` (`~/.freegrok`) after optionally copying `default_legacy`
///    (`~/.grok`) into it when dest is missing
pub fn resolve_user_home(
    freegrok_home: Option<&str>,
    grok_home: Option<&str>,
    default_fg: &Path,
    default_legacy: &Path,
    migrate: bool,
) -> HomeResolution {
    if let Some(v) = freegrok_home.map(str::trim).filter(|s| !s.is_empty()) {
        return HomeResolution {
            path: PathBuf::from(v),
            copied_from: None,
        };
    }
    if let Some(v) = grok_home.map(str::trim).filter(|s| !s.is_empty()) {
        return HomeResolution {
            path: PathBuf::from(v),
            copied_from: None,
        };
    }
    let mut copied_from = None;
    if migrate && !default_fg.exists() && default_legacy.exists() {
        if copy_grok_tree(default_legacy, default_fg).ok() == Some(true) {
            copied_from = Some(default_legacy.to_path_buf());
        }
    }
    HomeResolution {
        path: default_fg.to_path_buf(),
        copied_from,
    }
}

/// `$FREEGROK_HOME` / `$GROK_HOME` verbatim when non-empty, else `<home>/.grok`
/// (legacy shape used by callers that only pass a single env). Prefer
/// [`resolve_user_home`] for FreeGrok product resolution.
///
/// Production resolution goes through [`resolve_user_home`]; this helper is the
/// characterization surface for the single-env GROK_HOME path.
#[cfg(test)]
fn resolve_grok_home_from(
    grok_home_env: Option<&OsStr>,
    os_home: Option<&Path>,
) -> Option<PathBuf> {
    if let Some(env) = grok_home_env.filter(|env| !env.is_empty()) {
        return Some(PathBuf::from(env));
    }
    os_home.map(grok_home_in)
}

/// Resolve the grok home from the environment (fresh, no cache); `None` if
/// neither `$FREEGROK_HOME`/`$GROK_HOME` nor an OS home resolves.
pub fn resolve_grok_home() -> Option<PathBuf> {
    let fg = std::env::var("FREEGROK_HOME").ok();
    let gk = std::env::var("GROK_HOME").ok();
    if fg.as_deref().map(str::trim).is_some_and(|s| !s.is_empty())
        || gk.as_deref().map(str::trim).is_some_and(|s| !s.is_empty())
    {
        return Some(
            resolve_user_home(
                fg.as_deref(),
                gk.as_deref(),
                Path::new(""),
                Path::new(""),
                false,
            )
            .path,
        );
    }
    dirs::home_dir().map(|h| {
        let migrate = std::env::var_os("FREEGROK_NO_MIGRATE").is_none();
        resolve_user_home(
            None,
            None,
            &freegrok_home_in(&h),
            &grok_home_in(&h),
            migrate,
        )
        .path
    })
}

/// The default `<home>/.grok` legacy tree (copy source).
pub fn default_grok_home() -> PathBuf {
    grok_home_in(&dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")))
}

/// The default `<home>/.freegrok` product tree.
pub fn default_freegrok_home() -> PathBuf {
    freegrok_home_in(&dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")))
}

/// The grok home, created if missing and cached for the process.
///
/// Order: `$FREEGROK_HOME` → `$GROK_HOME` → `~/.freegrok` (copying `~/.grok`
/// when dest is missing).
pub fn grok_home() -> PathBuf {
    static GROK_HOME: OnceLock<PathBuf> = OnceLock::new();
    GROK_HOME
        .get_or_init(|| {
            let home = resolve_grok_home().unwrap_or_else(default_freegrok_home);
            if let Err(err) = std::fs::create_dir_all(&home) {
                tracing::warn!(path = %home.display(), %err, "failed to create grok home");
            }
            home
        })
        .clone()
}

/// Like [`grok_home`], but `None` when no home resolves (no cwd fallback).
pub fn user_grok_home() -> Option<PathBuf> {
    let resolvable = std::env::var_os("FREEGROK_HOME").is_some()
        || std::env::var_os("GROK_HOME").is_some()
        || dirs::home_dir().is_some();
    resolvable.then(grok_home)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use std::ffi::OsString;

    #[test]
    fn env_wins_over_os_home() {
        let resolved =
            resolve_grok_home_from(Some(OsStr::new("/custom/home")), Some(Path::new("/home/u")));
        assert_eq!(resolved, Some(PathBuf::from("/custom/home")));
    }

    #[test]
    fn env_used_verbatim_even_when_it_exists() {
        // A real, existing dir whose canonical form differs (macOS symlinks
        // `/var` -> `/private/var`): the env value must come back unchanged.
        let tmp = tempfile::tempdir().unwrap();
        let resolved = resolve_grok_home_from(Some(tmp.path().as_os_str()), None);
        assert_eq!(resolved, Some(tmp.path().to_path_buf()));
    }

    #[test]
    fn empty_env_falls_through_to_os_home() {
        let tmp = tempfile::tempdir().unwrap();
        let resolved = resolve_grok_home_from(Some(&OsString::new()), Some(tmp.path()));
        assert_eq!(
            resolved,
            Some(dunce::canonicalize(tmp.path()).unwrap().join(".grok"))
        );
    }

    #[test]
    fn default_grok_home_has_no_verbatim_prefix() {
        // The reason we canonicalize via dunce: std::fs::canonicalize yields
        // `\\?\` verbatim paths on Windows that break git and byte-exact
        // comparisons. No-op assertion on Unix.
        let home = default_grok_home();
        assert!(!home.to_string_lossy().starts_with(r"\\?\"));
        assert!(home.ends_with(".grok"));
    }

    #[test]
    fn none_when_nothing_resolves() {
        assert_eq!(
            resolve_grok_home_from(/* grok_home_env */ None, /* os_home */ None),
            None
        );
    }

    fn write_file(path: &Path, body: &str) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, body).unwrap();
    }

    fn seed_legacy_tree(root: &Path) {
        write_file(&root.join("config.toml"), "model = \"legacy\"\n");
        write_file(&root.join("skills/ulw/SKILL.md"), "# ulw\n");
        write_file(&root.join("downloads/big.bin"), "BLOB");
        write_file(&root.join("marketplace-cache/x.idx"), "idx");
        write_file(&root.join("sandbox-blocked.1/x"), "nope");
    }

    #[test]
    fn freegrok_home_wins_over_grok_home() {
        let tmp = tempfile::tempdir().unwrap();
        let fg = tmp.path().join("from-fg");
        let r = resolve_user_home(
            Some(fg.to_str().unwrap()),
            Some("/tmp/legacy-should-not-win"),
            &tmp.path().join("default-fg"),
            &tmp.path().join("default-legacy"),
            true,
        );
        assert_eq!(r.path, fg);
        assert_eq!(r.copied_from, None);
    }

    #[test]
    fn grok_home_used_when_freegrok_home_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let gh = tmp.path().join("from-gh");
        let r = resolve_user_home(
            Some("  "),
            Some(gh.to_str().unwrap()),
            &tmp.path().join("default-fg"),
            &tmp.path().join("default-legacy"),
            true,
        );
        assert_eq!(r.path, gh);
        assert_eq!(r.copied_from, None);
    }

    #[test]
    fn copies_legacy_tree_into_default_freegrok() {
        let tmp = tempfile::tempdir().unwrap();
        let dest = tmp.path().join("home").join(".freegrok");
        let src = tmp.path().join("home").join(".grok");
        seed_legacy_tree(&src);
        let r = resolve_user_home(None, None, &dest, &src, true);
        assert_eq!(r.path, dest);
        assert_eq!(r.copied_from.as_deref(), Some(src.as_path()));
        assert_eq!(
            std::fs::read_to_string(dest.join("config.toml")).unwrap(),
            "model = \"legacy\"\n"
        );
        assert!(!dest.join("downloads").exists());
        assert!(!dest.join("marketplace-cache").exists());
        assert!(!dest.join("sandbox-blocked.1").exists());
        assert!(dest.join(MIGRATED_FROM_GROK_MARKER).is_file());
    }

    #[test]
    fn does_not_overwrite_existing_freegrok() {
        let tmp = tempfile::tempdir().unwrap();
        let dest = tmp.path().join(".freegrok");
        let src = tmp.path().join(".grok");
        seed_legacy_tree(&src);
        std::fs::create_dir_all(&dest).unwrap();
        std::fs::write(dest.join("config.toml"), "already\n").unwrap();
        let r = resolve_user_home(None, None, &dest, &src, true);
        assert_eq!(r.copied_from, None);
        assert_eq!(
            std::fs::read_to_string(dest.join("config.toml")).unwrap(),
            "already\n"
        );
    }

    #[test]
    fn migrate_false_skips_copy() {
        let tmp = tempfile::tempdir().unwrap();
        let dest = tmp.path().join(".freegrok");
        let src = tmp.path().join(".grok");
        seed_legacy_tree(&src);
        let r = resolve_user_home(None, None, &dest, &src, false);
        assert_eq!(r.path, dest);
        assert_eq!(r.copied_from, None);
        assert!(!dest.exists());
    }

    #[test]
    fn default_freegrok_home_ends_with_freegrok() {
        let home = default_freegrok_home();
        assert!(!home.to_string_lossy().starts_with(r"\\?\"));
        assert!(home.ends_with(".freegrok"));
    }
}
