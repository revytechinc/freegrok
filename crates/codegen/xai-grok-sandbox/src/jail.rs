//! FreeBSD jail-based sandbox (bubblewrap analog).
//!
//! Linux enforces many deny paths by re-execing under `bwrap` with bind mounts.
//! FreeBSD has no Landlock/Seatbelt in `nono`; isolation is planned via an
//! ephemeral jail + nullfs overlays, optionally through a privileged helper
//! (`grok-jail-helper`). See `docs/freebsd-port-and-jail-sandbox.md`.
//!
//! **Phase 1a–2 scaffolding:** detection, helper discovery, and re-exec
//! command construction. Until a real helper lands on `PATH` (or
//! `GROK_JAIL_HELPER`), `jail_reexec_command` returns `None` so startup
//! degrades gracefully (no crash).

use std::path::{Path, PathBuf};
use std::process::Command;

/// Env marker set when the process is running inside a grok-managed jail.
pub const JAIL_ENV_VAR: &str = "__GROK_INSIDE_JAIL";

/// Override path to the privileged jail helper binary.
pub const JAIL_HELPER_ENV: &str = "GROK_JAIL_HELPER";

/// Default helper basenames searched on `PATH`.
const DEFAULT_HELPER_NAMES: &[&str] = &["grok-jail-helper", "xai-grok-jail-helper"];
/// Packaged location (not setuid).
const LIBEXEC_HELPERS: &[&str] = &[
    "/usr/local/libexec/grok-jail-helper",
    "/usr/local/libexec/xai-grok-jail-helper",
];

/// Whether this process is already inside a grok jail (or any FreeBSD jail).
///
/// Prefer the grok env marker so nested detection matches `is_inside_bwrap`.
/// Also consults `security.jail.jailed` when available so a process started
/// under an external jail is not double-wrapped.
pub fn is_inside_jail() -> bool {
    if std::env::var_os(JAIL_ENV_VAR).is_some() {
        return true;
    }
    sysctl_jail_jailed().unwrap_or(false)
}

/// Read `security.jail.jailed` (1 = currently jailed). Best-effort; `None` on error.
pub fn sysctl_jail_jailed() -> Option<bool> {
    let mut val: libc::c_int = 0;
    let mut len = std::mem::size_of_val(&val);
    let name = std::ffi::CString::new("security.jail.jailed").ok()?;
    // SAFETY: name is a valid C string; val/len point at a stack c_int.
    let rc = unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            &mut val as *mut _ as *mut libc::c_void,
            &mut len,
            std::ptr::null_mut(),
            0,
        )
    };
    if rc != 0 {
        return None;
    }
    Some(val != 0)
}

/// Resolve the jail helper executable, if any.
///
/// Order: `$GROK_JAIL_HELPER` if it points at an existing file, else the first
/// of [`DEFAULT_HELPER_NAMES`] found on `PATH`.
pub fn resolve_jail_helper() -> Option<PathBuf> {
    if let Ok(path) = std::env::var(JAIL_HELPER_ENV) {
        let pb = PathBuf::from(&path);
        if pb.is_file() {
            return Some(pb);
        }
        tracing::debug!(
            path = %path,
            "{JAIL_HELPER_ENV} set but not a file; falling back to PATH search"
        );
    }
    for name in DEFAULT_HELPER_NAMES {
        if let Ok(path) = which(name) {
            return Some(path);
        }
    }
    for path in LIBEXEC_HELPERS {
        let pb = PathBuf::from(path);
        if pb.is_file() {
            return Some(pb);
        }
    }
    None
}

fn which(bin: &str) -> Result<PathBuf, ()> {
    let path = std::env::var_os("PATH").ok_or(())?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(bin);
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    Err(())
}

/// Build a command that re-execs the current process inside a FreeBSD jail
/// with deny paths applied (nullfs RO / placeholder bind-over), mirroring
/// [`crate::bwrap_reexec_command`].
///
/// Returns `None` when:
/// - already inside a jail, or
/// - no helper is available yet (caller must degrade: log and continue).
///
/// When a helper is present, the command is:
/// `helper --deny-write PATH ... --deny-read PATH ... -- -- exe args...`
/// The helper is responsible for jail setup, setting [`JAIL_ENV_VAR`], and
/// exec'ing the payload. Real helper semantics land with Phase 2 completion.
pub fn jail_reexec_command(
    deny_write: &[&str],
    deny_read: &[&str],
) -> Option<Command> {
    if is_inside_jail() {
        return None;
    }
    let helper = match resolve_jail_helper() {
        Some(h) => h,
        None => {
            tracing::debug!(
                "FreeBSD jail helper not found ({JAIL_HELPER_ENV} or {}); degrade",
                DEFAULT_HELPER_NAMES.join("|")
            );
            return None;
        }
    };

    let self_exe = std::env::current_exe().ok()?;
    let args: Vec<String> = std::env::args().skip(1).collect();

    let mut cmd = Command::new(helper);
    for path in deny_write {
        cmd.arg("--deny-write").arg(path);
    }
    for path in deny_read {
        cmd.arg("--deny-read").arg(path);
    }
    cmd.arg("--");
    cmd.arg(self_exe);
    cmd.args(args);
    // Marker for the child after helper re-exec (helper should also set this).
    cmd.env(JAIL_ENV_VAR, "1");
    Some(cmd)
}

/// Profile-aware jail re-exec (twin of `bwrap_reexec_for_profile` on Linux).
///
/// Resolves deny lists from the profile when possible; Phase 2 helper still
/// owns mount/nullfs planning. Returns `None` if already jailed or no helper.
pub fn jail_reexec_for_profile(
    profile: &crate::ProfileName,
    workspace: &Path,
) -> Option<Command> {
    if is_inside_jail() {
        return None;
    }
    if resolve_jail_helper().is_none() {
        return None;
    }
    let (deny_write, deny_read) = jail_deny_paths(profile, workspace);
    let write_refs: Vec<&str> = deny_write.iter().map(String::as_str).collect();
    let read_refs: Vec<&str> = deny_read.iter().map(String::as_str).collect();
    jail_reexec_command(&write_refs, &read_refs)
}

/// Exact deny paths for the helper. Globs are not expanded on FreeBSD
/// (Linux-only walk); they are logged and skipped so we never claim
/// coverage we do not have. Implemented here because `deny::exact_*`
/// helpers are cfg(linux).
fn jail_deny_paths(profile: &crate::ProfileName, workspace: &Path) -> (Vec<String>, Vec<String>) {
    let config = crate::profiles::load_sandbox_config(workspace);
    let deny_write = if crate::is_devbox_based(profile, &config) {
        vec!["/data".to_string()]
    } else {
        Vec::new()
    };
    if *profile == crate::ProfileName::Off {
        return (deny_write, Vec::new());
    }
    let resolved = match profile.resolve_profile(workspace, &config) {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(error = %e, "jail profile resolve failed; exact denies empty");
            return (deny_write, Vec::new());
        }
    };
    let mut deny_read = Vec::new();
    let mut glob_count = 0usize;
    for entry in &resolved.deny {
        let s = match entry.to_str() {
            Some(s) => s,
            None => continue,
        };
        if crate::deny::is_glob(s) {
            glob_count += 1;
            continue;
        }
        let path = if entry.is_absolute() {
            entry.clone()
        } else {
            workspace.join(entry)
        };
        deny_read.push(path.display().to_string());
    }
    if glob_count > 0 {
        tracing::warn!(
            count = glob_count,
            "FreeBSD jail helper does not expand deny globs; only exact paths are passed"
        );
    }
    deny_read.sort();
    deny_read.dedup();
    (deny_write, deny_read)
}

/// Human-readable status for probes / diagnostics (not a public API surface
/// for enforcement).
pub fn jail_backend_status() -> JailBackendStatus {
    JailBackendStatus {
        inside_jail: is_inside_jail(),
        sysctl_jailed: sysctl_jail_jailed(),
        helper: resolve_jail_helper(),
        marker_env: std::env::var_os(JAIL_ENV_VAR).is_some(),
    }
}

/// Snapshot of FreeBSD jail sandbox plumbing for probes.
#[derive(Debug, Clone)]
pub struct JailBackendStatus {
    pub inside_jail: bool,
    pub sysctl_jailed: Option<bool>,
    pub helper: Option<PathBuf>,
    pub marker_env: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // Serialize env mutations across tests in this module.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn jail_env_var_name_is_stable() {
        assert_eq!(JAIL_ENV_VAR, "__GROK_INSIDE_JAIL");
        assert_eq!(JAIL_HELPER_ENV, "GROK_JAIL_HELPER");
    }

    #[test]
    fn reexec_returns_none_without_helper() {
        let _g = ENV_LOCK.lock().unwrap();
        // Ensure we don't accidentally pick up a helper from the environment.
        let prev_helper = std::env::var_os(JAIL_HELPER_ENV);
        let prev_marker = std::env::var_os(JAIL_ENV_VAR);
        unsafe {
            std::env::remove_var(JAIL_HELPER_ENV);
            std::env::remove_var(JAIL_ENV_VAR);
        }
        // Empty PATH so DEFAULT_HELPER_NAMES cannot resolve.
        let prev_path = std::env::var_os("PATH");
        unsafe {
            std::env::set_var("PATH", "");
        }

        assert!(
            jail_reexec_command(&["/tmp"], &[]).is_none(),
            "without helper, reexec must be None"
        );
        assert!(
            jail_reexec_for_profile(&crate::ProfileName::Workspace, Path::new("/tmp")).is_none()
        );

        unsafe {
            match prev_helper {
                Some(v) => std::env::set_var(JAIL_HELPER_ENV, v),
                None => std::env::remove_var(JAIL_HELPER_ENV),
            }
            match prev_marker {
                Some(v) => std::env::set_var(JAIL_ENV_VAR, v),
                None => std::env::remove_var(JAIL_ENV_VAR),
            }
            match prev_path {
                Some(v) => std::env::set_var("PATH", v),
                None => std::env::remove_var("PATH"),
            }
        }
    }

    #[test]
    fn reexec_none_when_marker_set() {
        let _g = ENV_LOCK.lock().unwrap();
        let prev = std::env::var_os(JAIL_ENV_VAR);
        unsafe {
            std::env::set_var(JAIL_ENV_VAR, "1");
        }
        assert!(is_inside_jail());
        assert!(jail_reexec_command(&[], &[]).is_none());
        unsafe {
            match prev {
                Some(v) => std::env::set_var(JAIL_ENV_VAR, v),
                None => std::env::remove_var(JAIL_ENV_VAR),
            }
        }
    }

    #[test]
    fn helper_env_points_at_file() {
        let _g = ENV_LOCK.lock().unwrap();
        let dir = std::env::temp_dir().join(format!(
            "grok-jail-helper-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let helper = dir.join("fake-helper");
        std::fs::write(&helper, b"#!/bin/sh\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let prev = std::env::var_os(JAIL_HELPER_ENV);
        let prev_marker = std::env::var_os(JAIL_ENV_VAR);
        unsafe {
            std::env::set_var(JAIL_HELPER_ENV, &helper);
            std::env::remove_var(JAIL_ENV_VAR);
        }
        let resolved = resolve_jail_helper();
        assert_eq!(resolved.as_deref(), Some(helper.as_path()));
        let cmd = jail_reexec_command(&["/var/tmp"], &["/secret"]).expect("helper present");
        let program = cmd.get_program().to_string_lossy();
        assert!(program.contains("fake-helper") || Path::new(program.as_ref()) == helper);
        unsafe {
            match prev {
                Some(v) => std::env::set_var(JAIL_HELPER_ENV, v),
                None => std::env::remove_var(JAIL_HELPER_ENV),
            }
            match prev_marker {
                Some(v) => std::env::set_var(JAIL_ENV_VAR, v),
                None => std::env::remove_var(JAIL_ENV_VAR),
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sysctl_jail_jailed_is_readable() {
        // Outside a jail this is typically Some(false); inside Some(true).
        // On weird CI without the sysctl, None is acceptable.
        let v = sysctl_jail_jailed();
        if let Some(jailed) = v {
            assert!(!jailed || is_inside_jail());
        }
    }

    #[test]
    fn backend_status_does_not_panic() {
        let s = jail_backend_status();
        let _ = format!("{s:?}");
    }
}
