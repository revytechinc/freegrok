//! Filesystem locations for grok / FreeGrok config files and binaries.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

static GROK_HOME: OnceLock<PathBuf> = OnceLock::new();

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

/// Look up `FREEGROK_{suffix}` then `GROK_{suffix}`. Empty values are ignored.
pub fn env_var(suffix: &str) -> Option<String> {
    nonempty_env(&format!("FREEGROK_{suffix}")).or_else(|| nonempty_env(&format!("GROK_{suffix}")))
}

/// Look up `FREEGROK_{suffix}` then `GROK_{suffix}` as `OsString`.
pub fn env_var_os(suffix: &str) -> Option<OsString> {
    std::env::var_os(format!("FREEGROK_{suffix}"))
        .or_else(|| std::env::var_os(format!("GROK_{suffix}")))
}

fn nonempty_env(name: &str) -> Option<String> {
    std::env::var(name).ok().and_then(|v| {
        let t = v.trim();
        if t.is_empty() {
            None
        } else {
            Some(t.to_string())
        }
    })
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

/// Result of resolving the per-user config home (testable; no process env).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HomeResolution {
    pub path: PathBuf,
    pub copied_from: Option<PathBuf>,
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

/// Project config directory: copy `.grok` → `.freegrok` when dest is missing,
/// then prefer `.freegrok`.
pub fn project_config_dir(root: &Path) -> PathBuf {
    let fg = root.join(".freegrok");
    let legacy = root.join(".grok");
    if !fg.exists() && legacy.exists() {
        let _ = copy_grok_tree(&legacy, &fg);
    }
    if fg.exists() { fg } else { legacy }
}

/// System config dir under an injectable `/etc` root.
/// Prefer `freegrok` when it exists, else `grok` when it exists, else `freegrok`.
pub fn system_config_dir_in(etc: &Path) -> PathBuf {
    let fg = etc.join("freegrok");
    let legacy = etc.join("grok");
    if fg.exists() {
        fg
    } else if legacy.exists() {
        legacy
    } else {
        fg
    }
}

#[cfg(target_os = "macos")]
const CLAUDE_MANAGED_SETTINGS_PATH: &str =
    "/Library/Application Support/ClaudeCode/managed-settings.json";
#[cfg(target_os = "linux")]
const CLAUDE_MANAGED_SETTINGS_PATH: &str = "/etc/claude-code/managed-settings.json";

fn user_home_dir() -> PathBuf {
    #[allow(deprecated)]
    let home = std::env::home_dir().unwrap_or_else(|| PathBuf::from("."));
    dunce::canonicalize(&home).unwrap_or(home)
}

/// The default *legacy* grok-build directory (`~/.grok`). Used as the copy
/// source when migrating into [`default_freegrok_home`].
///
/// Uses [`dunce::canonicalize`] instead of [`std::fs::canonicalize`]: on
/// Windows, std returns a verbatim path (`\\?\C:\Users\...`) which external
/// tools choke on — e.g. `git clone` rejects `\\?\` destinations with
/// "Invalid argument", breaking marketplace cache clones under
/// `~/.grok/marketplace-cache`. `dunce` strips the prefix whenever the path
/// is safely representable in legacy form; on non-Windows it is identical to
/// `std::fs::canonicalize`.
///
/// Keep the dunce canonicalization in sync with the hand-rolled duplicate in
/// `xai_fast_worktree::db::resolve_grok_home` (deliberately standalone crate).
pub fn default_grok_home() -> PathBuf {
    user_home_dir().join(".grok")
}

/// The default FreeGrok user directory (`~/.freegrok`, canonicalized).
pub fn default_freegrok_home() -> PathBuf {
    user_home_dir().join(".freegrok")
}

/// Per-user config directory.
///
/// Order: `$FREEGROK_HOME` → `$GROK_HOME` → `~/.freegrok` (copying `~/.grok`
/// into it when dest is missing). Created if needed.
pub fn grok_home() -> PathBuf {
    GROK_HOME
        .get_or_init(|| {
            let migrate = std::env::var_os("FREEGROK_NO_MIGRATE").is_none();
            let resolved = resolve_user_home(
                nonempty_env("FREEGROK_HOME").as_deref(),
                nonempty_env("GROK_HOME").as_deref(),
                &default_freegrok_home(),
                &default_grok_home(),
                migrate,
            );
            if let Some(src) = &resolved.copied_from {
                eprintln!(
                    "freegrok: copied grok-build config from {} → {}",
                    src.display(),
                    resolved.path.display()
                );
            }
            let _ = std::fs::create_dir_all(&resolved.path);
            resolved.path
        })
        .clone()
}

/// The user-global grok home, but only when one genuinely resolves: `Some` when
/// `$FREEGROK_HOME`/`$GROK_HOME` is set or a home directory is found, `None`
/// otherwise. Unlike [`grok_home()`], this never falls back to a cwd-relative
/// `.grok`, so callers that *scan* user-global grok resources (hooks,
/// marketplace sources, ...) don't mistake a project's `.grok` tree for the
/// user-global one when no home resolves.
pub fn user_grok_home() -> Option<PathBuf> {
    #[allow(deprecated)]
    let resolvable = env_var_os("HOME").is_some() || std::env::home_dir().is_some();
    resolvable.then(grok_home)
}

/// Canonical application path: `$FREEGROK_HOME/bin/freegrok` (Unix) or
/// `freegrok.exe` (Windows).
pub fn grok_application() -> PathBuf {
    grok_application_in(&grok_home())
}

/// [`grok_application`] under an explicit home instead of `$GROK_HOME`.
pub fn grok_application_in(home: &std::path::Path) -> PathBuf {
    let name = if cfg!(windows) {
        "freegrok.exe"
    } else {
        "freegrok"
    };
    home.join("bin").join(name)
}

/// System-wide config directory: `/etc/freegrok` if present, else `/etc/grok`
/// if present, else `/etc/freegrok`. `None` on Windows.
pub fn system_config_dir() -> Option<PathBuf> {
    if cfg!(unix) {
        Some(system_config_dir_in(Path::new("/etc")))
    } else {
        None
    }
}

/// System path for the managed-settings.json used for settings compat, if it exists.
#[cfg(any(target_os = "macos", target_os = "linux"))]
pub fn claude_managed_settings_path() -> Option<PathBuf> {
    let path = PathBuf::from(CLAUDE_MANAGED_SETTINGS_PATH);
    path.exists().then_some(path)
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub fn claude_managed_settings_path() -> Option<PathBuf> {
    None
}

/// The platform path where managed-settings.json would live for settings
/// compat, whether or not it exists. `None` on unsupported platforms.
#[cfg(any(target_os = "macos", target_os = "linux"))]
pub fn claude_managed_settings_probe_path() -> Option<PathBuf> {
    Some(PathBuf::from(CLAUDE_MANAGED_SETTINGS_PATH))
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub fn claude_managed_settings_probe_path() -> Option<PathBuf> {
    None
}

/// Max bytes for a single directory name component (macOS APFS, Linux ext4,
/// NTFS all enforce 255 bytes).
const MAX_DIRNAME_BYTES: usize = 255;

/// Encode a CWD string into a filesystem-safe directory name component.
///
/// Short CWDs (URL-encoded form <= 255 bytes) use URL-encoding for backward
/// compatibility and human readability on disk.
///
/// Long CWDs (> 255 bytes encoded) use a compact `{slug}-{blake3_hex16}`
/// form that is always <= 57 bytes. Callers must write a `.cwd` metadata
/// file via [`ensure_sessions_cwd_dir`] so the original CWD can be
/// recovered by [`decode_cwd_from_dirname`].
pub fn encode_cwd_dirname(cwd: &str) -> String {
    let url_encoded = urlencoding::encode(cwd);
    if url_encoded.len() <= MAX_DIRNAME_BYTES {
        return url_encoded.into_owned();
    }
    let hash = blake3::hash(cwd.as_bytes());
    let hash16 = &hash.to_hex()[..16];
    let leaf = std::path::Path::new(cwd)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("workspace");
    let slug = slugify(leaf, 40);
    let slug = if slug.is_empty() { "workspace" } else { &slug };
    format!("{slug}-{hash16}")
}

/// Recover the original CWD from a sessions CWD directory.
///
/// Tries URL-decoding the directory name first (works for short/legacy dirs).
/// Falls back to reading a `.cwd` metadata file inside the directory (written
/// by [`ensure_sessions_cwd_dir`] for hash-based dirs).
pub fn decode_cwd_from_dirname(dir: &std::path::Path) -> Option<String> {
    let name = dir.file_name()?.to_str()?;
    if let Ok(decoded) = urlencoding::decode(name) {
        let s = decoded.into_owned();
        // URL-decoded absolute CWDs always start with `/` (Unix) or a drive
        // letter (Windows).  The slug-hash form never does, so this
        // distinguishes the two encodings unambiguously.
        if s.starts_with('/') || (cfg!(windows) && s.chars().nth(1) == Some(':')) {
            return Some(s);
        }
    }
    std::fs::read_to_string(dir.join(".cwd"))
        .ok()
        .map(|s| s.trim().to_string())
}

/// Best-effort chmod 0700 on Unix, no-op elsewhere: session dirs hold chat
/// history, and creators re-run on every touch so the mode self-heals.
/// Failures are logged (not returned): on chmod-hostile filesystems (FAT,
/// some network mounts) healing pre-existing loose dirs can never succeed,
/// and that must be visible.
pub fn set_dir_owner_only(dir: &std::path::Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Err(e) = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700)) {
            tracing::debug!(?e, dir = %dir.display(), "failed to chmod session dir owner-only");
        }
    }
    #[cfg(not(unix))]
    {
        let _ = dir;
    }
}

/// `create_dir_all` with directories born 0700 on Unix (no umask window),
/// plus a self-heal chmod for a pre-existing `dir`. Prefer this over bare
/// `create_dir_all` for anything under `sessions/`.
pub fn create_dir_all_owner_only(dir: &std::path::Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)?;
    }
    #[cfg(not(unix))]
    std::fs::create_dir_all(dir)?;
    set_dir_owner_only(dir);
    Ok(())
}

/// Build the CWD-level session directory path:
/// `grok_home()/sessions/{encode_cwd_dirname(cwd)}`.
///
/// Does **not** create the directory on disk — use [`ensure_sessions_cwd_dir`]
/// when the directory must exist.
pub fn sessions_cwd_dir(cwd: &str) -> PathBuf {
    sessions_cwd_dir_in(&grok_home(), cwd)
}

/// [`sessions_cwd_dir`] with an injectable grok home — the single source of
/// truth for the `sessions/<encoded-cwd>` path shape.
pub fn sessions_cwd_dir_in(grok_home: &std::path::Path, cwd: &str) -> PathBuf {
    grok_home.join("sessions").join(encode_cwd_dirname(cwd))
}

/// Create the CWD-level session directory and write a `.cwd` metadata file
/// when hash-based encoding is used (long paths).
///
/// For short paths the `.cwd` file is not written because the directory name
/// itself is reversible via URL-decoding.
pub fn ensure_sessions_cwd_dir(cwd: &str) -> std::io::Result<PathBuf> {
    ensure_sessions_cwd_dir_in(&grok_home(), cwd)
}

/// [`ensure_sessions_cwd_dir`] with an injectable grok home.
pub fn ensure_sessions_cwd_dir_in(
    grok_home: &std::path::Path,
    cwd: &str,
) -> std::io::Result<PathBuf> {
    let encoded_name = encode_cwd_dirname(cwd);
    let dir = sessions_cwd_dir_in(grok_home, cwd);
    // 0700 dir + root shield everything beneath (children with looser modes,
    // cwd-path dirnames, the session search index).
    create_dir_all_owner_only(&dir)?;
    set_dir_owner_only(&grok_home.join("sessions"));
    // Hash-based encoding is in use when the dirname differs from the
    // plain URL-encoded form.  Write a `.cwd` file so decode can recover
    // the original path.  O_CREAT|O_EXCL via create_new avoids TOCTOU
    // races with parallel session starts.
    if encoded_name != urlencoding::encode(cwd).as_ref() {
        let cwd_file = dir.join(".cwd");
        match std::fs::File::create_new(&cwd_file) {
            Ok(mut f) => {
                std::io::Write::write_all(&mut f, cwd.as_bytes())?;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
        }
    }
    Ok(dir)
}

/// Generate a URL-safe slug from a string.
///
/// Lowercases, replaces non-alphanumeric chars with `-`, collapses
/// consecutive dashes, and truncates to `max_len` characters.
fn slugify(input: &str, max_len: usize) -> String {
    let mut result = String::with_capacity(input.len());
    let mut prev_dash = false;
    for c in input.to_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            result.push(c);
            prev_dash = false;
        } else if !prev_dash {
            result.push('-');
            prev_dash = true;
        }
    }
    let trimmed = result.trim_matches('-');
    trimmed.chars().take(max_len).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use tempfile::TempDir;

    /// Realistic CWDs that trigger the bug (URL-encoded > 255 bytes).
    const LONG_CWDS: &[&str] = &[
        "/Users/dev/Documents/開発プロジェクト/機能追加/テスト環境/ソースコード/main-branch",
        "/Users/user/Library/Mobile Documents/com~apple~CloudDocs/项目文件/深层嵌套目录/更深层次的/工作区域/project",
        "/Users/user/Library/CloudStorage/OneDrive-대한민국회사/프로젝트/개발환경/소스코드/백엔드/서비스/my-app",
        "/Users/user/Documents/工作文件夹/二零二六年项目/子目录一/子目录二/子目录三/源代码/code",
    ];

    #[test]
    fn long_cwd_uses_hash_fallback_within_name_max() {
        let long_cwd = format!("/Users/test/{}", "中".repeat(30));
        let encoded = encode_cwd_dirname(&long_cwd);
        assert!(encoded.len() <= MAX_DIRNAME_BYTES);
        assert!(!encoded.starts_with("%2F"));
    }

    #[test]
    fn different_long_paths_produce_different_hashes() {
        let a = format!("/Users/test/{}", "中".repeat(30));
        let b = format!("/Users/test/{}", "日".repeat(30));
        assert_ne!(encode_cwd_dirname(&a), encode_cwd_dirname(&b));
    }

    #[test]
    fn decode_reads_cwd_file_for_hash_dirs() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("some-slug-abcdef0123456789");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(".cwd"), "/original/long/path").unwrap();
        assert_eq!(
            decode_cwd_from_dirname(&dir),
            Some("/original/long/path".to_string())
        );
    }

    #[test]
    fn decode_returns_none_without_cwd_file() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("some-slug-abcdef0123456789");
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(decode_cwd_from_dirname(&dir), None);
    }

    #[test]
    fn cwd_file_write_is_idempotent_via_excl() {
        let tmp = TempDir::new().unwrap();
        let long_cwd = format!("/Users/test/{}", "中".repeat(30));
        let dir = tmp.path().join(encode_cwd_dirname(&long_cwd));
        std::fs::create_dir_all(&dir).unwrap();
        let cwd_file = dir.join(".cwd");
        std::fs::write(&cwd_file, &long_cwd).unwrap();
        match std::fs::File::create_new(&cwd_file) {
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            other => panic!("expected AlreadyExists, got: {other:?}"),
        }
        assert_eq!(std::fs::read_to_string(&cwd_file).unwrap(), long_cwd);
    }

    #[test]
    fn url_encoded_long_cwd_fails_on_real_filesystem() {
        let tmp = TempDir::new().unwrap();
        let url_encoded = urlencoding::encode(LONG_CWDS[0]).into_owned();
        let result = std::fs::create_dir_all(tmp.path().join(&url_encoded));
        assert!(result.is_err());
    }

    #[test]
    fn full_roundtrip_on_real_filesystem_for_long_cwds() {
        let tmp = TempDir::new().unwrap();
        for cwd in LONG_CWDS {
            let encoded = encode_cwd_dirname(cwd);
            let dir = tmp.path().join(&encoded);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join(".cwd"), cwd).unwrap();
            assert_eq!(decode_cwd_from_dirname(&dir).as_deref(), Some(*cwd));
        }
    }

    #[test]
    fn short_cwds_use_url_encoding_and_roundtrip_on_real_filesystem() {
        let tmp = TempDir::new().unwrap();
        for cwd in [
            "/Users/foo/project",
            "/tmp",
            "/Users/user/Documents/project-名前",
        ] {
            let encoded = encode_cwd_dirname(cwd);
            assert_eq!(encoded, urlencoding::encode(cwd).into_owned());
            let dir = tmp.path().join(&encoded);
            std::fs::create_dir_all(&dir).unwrap();
            assert_eq!(decode_cwd_from_dirname(&dir).as_deref(), Some(cwd));
        }
    }

    #[test]
    fn default_grok_home_has_no_verbatim_prefix() {
        // On Windows, std::fs::canonicalize returns `\\?\C:\...` verbatim
        // paths that external tools (notably `git clone`) reject. The dunce
        // canonicalization must yield a plain path. No-op assertion on Unix.
        let home = default_grok_home();
        assert!(!home.to_string_lossy().starts_with(r"\\?\"));
        assert!(home.ends_with(".grok"));
    }

    #[test]
    fn default_freegrok_home_has_no_verbatim_prefix() {
        let home = default_freegrok_home();
        assert!(!home.to_string_lossy().starts_with(r"\\?\"));
        assert!(home.ends_with(".freegrok"));
    }

    fn write_file(path: &Path, body: &str) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, body).unwrap();
    }

    fn seed_legacy_tree(root: &Path) {
        write_file(&root.join("config.toml"), "model = \"legacy\"\n");
        write_file(&root.join("auth.json"), "{\"token\":\"abc\"}\n");
        write_file(&root.join("skills/ultrawork/SKILL.md"), "# ulw\n");
        write_file(&root.join("rules/keep.md"), "keep going\n");
        write_file(&root.join("downloads/big.bin"), "BLOB");
        write_file(&root.join("marketplace-cache/x.idx"), "idx");
        write_file(&root.join("sandbox-blocked-dir.1/x"), "nope");
    }

    #[test]
    fn copy_grok_tree_copies_config_and_skills_skips_caches() {
        let tmp = TempDir::new().unwrap();
        let src = tmp.path().join("legacy");
        let dest = tmp.path().join("freegrok");
        seed_legacy_tree(&src);

        assert!(copy_grok_tree(&src, &dest).unwrap());
        assert_eq!(
            std::fs::read_to_string(dest.join("config.toml")).unwrap(),
            "model = \"legacy\"\n"
        );
        assert_eq!(
            std::fs::read_to_string(dest.join("skills/ultrawork/SKILL.md")).unwrap(),
            "# ulw\n"
        );
        assert_eq!(
            std::fs::read_to_string(dest.join("auth.json")).unwrap(),
            "{\"token\":\"abc\"}\n"
        );
        assert!(dest.join(MIGRATED_FROM_GROK_MARKER).is_file());
        assert!(!dest.join("downloads").exists(), "must not copy downloads");
        assert!(
            !dest.join("marketplace-cache").exists(),
            "must not copy marketplace-cache"
        );
        assert!(
            !dest.join("sandbox-blocked-dir.1").exists(),
            "must not copy sandbox-blocked-dir.*"
        );
    }

    #[test]
    fn copy_grok_tree_is_noop_when_dest_exists() {
        let tmp = TempDir::new().unwrap();
        let src = tmp.path().join("legacy");
        let dest = tmp.path().join("freegrok");
        seed_legacy_tree(&src);
        std::fs::create_dir_all(&dest).unwrap();
        std::fs::write(dest.join("config.toml"), "keep\n").unwrap();
        assert!(!copy_grok_tree(&src, &dest).unwrap());
        assert_eq!(
            std::fs::read_to_string(dest.join("config.toml")).unwrap(),
            "keep\n"
        );
    }

    #[test]
    fn copy_grok_tree_is_noop_when_src_missing() {
        let tmp = TempDir::new().unwrap();
        let src = tmp.path().join("nope");
        let dest = tmp.path().join("freegrok");
        assert!(!copy_grok_tree(&src, &dest).unwrap());
        assert!(!dest.exists());
    }

    #[test]
    fn resolve_prefers_freegrok_home_env() {
        let tmp = TempDir::new().unwrap();
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
    fn resolve_falls_back_to_grok_home_env() {
        let tmp = TempDir::new().unwrap();
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
    fn resolve_copies_legacy_tree_into_default_freegrok() {
        let tmp = TempDir::new().unwrap();
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
    }

    #[test]
    fn resolve_does_not_overwrite_existing_freegrok() {
        let tmp = TempDir::new().unwrap();
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
    fn resolve_skips_copy_when_migrate_false() {
        let tmp = TempDir::new().unwrap();
        let dest = tmp.path().join(".freegrok");
        let src = tmp.path().join(".grok");
        seed_legacy_tree(&src);
        let r = resolve_user_home(None, None, &dest, &src, false);
        assert_eq!(r.path, dest);
        assert_eq!(r.copied_from, None);
        assert!(!dest.exists());
    }

    #[test]
    fn project_config_dir_copies_dot_grok_to_dot_freegrok() {
        let tmp = TempDir::new().unwrap();
        write_file(&tmp.path().join(".grok/config.toml"), "from-project\n");
        let dir = project_config_dir(tmp.path());
        assert_eq!(dir, tmp.path().join(".freegrok"));
        assert_eq!(
            std::fs::read_to_string(dir.join("config.toml")).unwrap(),
            "from-project\n"
        );
        // legacy tree is kept
        assert!(tmp.path().join(".grok/config.toml").is_file());
    }

    #[test]
    fn project_config_dir_prefers_existing_freegrok() {
        let tmp = TempDir::new().unwrap();
        write_file(&tmp.path().join(".grok/config.toml"), "old\n");
        write_file(&tmp.path().join(".freegrok/config.toml"), "new\n");
        let dir = project_config_dir(tmp.path());
        assert_eq!(dir, tmp.path().join(".freegrok"));
        assert_eq!(
            std::fs::read_to_string(dir.join("config.toml")).unwrap(),
            "new\n"
        );
    }

    #[test]
    fn system_config_dir_in_prefers_freegrok_when_present() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("freegrok")).unwrap();
        std::fs::create_dir_all(tmp.path().join("grok")).unwrap();
        assert_eq!(
            system_config_dir_in(tmp.path()),
            tmp.path().join("freegrok")
        );
    }

    #[test]
    fn system_config_dir_in_falls_back_to_grok() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("grok")).unwrap();
        assert_eq!(system_config_dir_in(tmp.path()), tmp.path().join("grok"));
    }

    #[test]
    fn system_config_dir_in_defaults_to_freegrok_when_neither_exists() {
        let tmp = TempDir::new().unwrap();
        assert_eq!(
            system_config_dir_in(tmp.path()),
            tmp.path().join("freegrok")
        );
    }

    #[test]
    fn env_var_prefers_freegrok_prefix() {
        const SUFFIX: &str = "CFGCOPY_DUALREAD_TEST";
        let fg = format!("FREEGROK_{SUFFIX}");
        let gk = format!("GROK_{SUFFIX}");
        // SAFETY: test-only env keys, unique suffix, restored before return.
        unsafe {
            std::env::set_var(&fg, "from-freegrok");
            std::env::set_var(&gk, "from-grok");
        }
        let got = env_var(SUFFIX);
        unsafe {
            std::env::remove_var(&fg);
            std::env::remove_var(&gk);
        }
        assert_eq!(got.as_deref(), Some("from-freegrok"));
    }

    #[test]
    fn env_var_falls_back_to_grok_prefix() {
        const SUFFIX: &str = "CFGCOPY_FALLBACK_TEST";
        let gk = format!("GROK_{SUFFIX}");
        unsafe {
            std::env::remove_var(format!("FREEGROK_{SUFFIX}"));
            std::env::set_var(&gk, "legacy-only");
        }
        let got = env_var(SUFFIX);
        unsafe {
            std::env::remove_var(&gk);
        }
        assert_eq!(got.as_deref(), Some("legacy-only"));
    }

    #[test]
    fn grok_application_in_uses_freegrok_bin_name() {
        let p = grok_application_in(Path::new("/tmp/h"));
        assert!(p.ends_with(if cfg!(windows) {
            "bin/freegrok.exe"
        } else {
            "bin/freegrok"
        }));
    }

    #[cfg(unix)]
    fn unix_mode(path: &std::path::Path) -> u32 {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    #[test]
    #[cfg(unix)]
    fn set_dir_owner_only_restricts_mode_to_0700() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("child");
        std::fs::create_dir(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();

        set_dir_owner_only(&dir);

        assert_eq!(unix_mode(&dir), 0o700);
    }

    #[test]
    fn set_dir_owner_only_is_best_effort_on_missing_path() {
        // Must not panic or error — chmod failures are intentionally ignored.
        set_dir_owner_only(std::path::Path::new("/nonexistent/definitely/not/here"));
    }

    #[test]
    #[cfg(unix)]
    fn create_dir_all_owner_only_creates_chain_born_0700() {
        let tmp = TempDir::new().unwrap();
        let leaf = tmp.path().join("a").join("b");
        create_dir_all_owner_only(&leaf).unwrap();
        assert_eq!(unix_mode(&leaf), 0o700, "leaf must be 0700");
        assert_eq!(
            unix_mode(leaf.parent().unwrap()),
            0o700,
            "created intermediate must be born 0700"
        );
    }

    #[test]
    #[cfg(unix)]
    fn create_dir_all_owner_only_retightens_existing_dir() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("existing");
        std::fs::create_dir(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();

        create_dir_all_owner_only(&dir).unwrap();

        assert_eq!(unix_mode(&dir), 0o700);
    }

    #[test]
    #[cfg(unix)]
    fn ensure_sessions_cwd_dir_creates_owner_only_dir_and_root() {
        let home = TempDir::new().unwrap();
        let dir = ensure_sessions_cwd_dir_in(home.path(), "/some/project").unwrap();
        assert!(dir.is_dir());
        assert_eq!(unix_mode(&dir), 0o700);
        assert_eq!(
            unix_mode(&home.path().join("sessions")),
            0o700,
            "sessions root must be 0700 (shields stale children and the search index)"
        );
    }

    #[test]
    #[cfg(unix)]
    fn ensure_sessions_cwd_dir_retightens_existing_loose_dirs() {
        use std::os::unix::fs::PermissionsExt;
        let home = TempDir::new().unwrap();
        let root = home.path().join("sessions");
        let dir = ensure_sessions_cwd_dir_in(home.path(), "/some/project").unwrap();
        // Simulate dirs created by an older grok with umask-default perms.
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o755)).unwrap();

        let again = ensure_sessions_cwd_dir_in(home.path(), "/some/project").unwrap();

        assert_eq!(again, dir);
        assert_eq!(unix_mode(&dir), 0o700, "mode must self-heal on next touch");
        assert_eq!(unix_mode(&root), 0o700, "root must self-heal on next touch");
    }

    #[test]
    #[cfg(unix)]
    fn ensure_sessions_cwd_dir_hash_encoded_writes_cwd_file_and_owner_only() {
        let home = TempDir::new().unwrap();
        let long_cwd = format!("/Users/test/{}", "中".repeat(30));
        let dir = ensure_sessions_cwd_dir_in(home.path(), &long_cwd).unwrap();
        assert_eq!(unix_mode(&dir), 0o700);
        assert_eq!(std::fs::read_to_string(dir.join(".cwd")).unwrap(), long_cwd);
    }

    #[test]
    fn slugify_basic() {
        assert_eq!(slugify("Hello World!", 40), "hello-world");
    }

    #[test]
    fn slugify_cjk_produces_empty() {
        assert_eq!(slugify("深层目录", 40), "");
    }

    #[test]
    fn slugify_truncates() {
        assert_eq!(slugify(&"a".repeat(100), 10).len(), 10);
    }
}
