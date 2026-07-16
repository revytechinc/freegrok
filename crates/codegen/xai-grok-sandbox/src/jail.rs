//! FreeBSD jail-based sandbox (bubblewrap analog).
//!
//! Linux enforces many deny paths by re-execing under `bwrap` with bind mounts.
//! FreeBSD has no Landlock/Seatbelt in `nono`; isolation is planned via an
//! ephemeral jail + nullfs overlays, optionally through a privileged helper
//! (`grok-jail-helper`). See `docs/freebsd-port-and-jail-sandbox.md`.
//!
//! **Phase 1a (this module):** detection + re-exec scaffolding only.
//! `jail_reexec_command` returns `None` until the helper/path is implemented,
//! so startup degrades gracefully (no crash) when the sandbox cannot apply.

/// Env marker set when the process is running inside a grok-managed jail.
pub const JAIL_ENV_VAR: &str = "__GROK_INSIDE_JAIL";

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
fn sysctl_jail_jailed() -> Option<bool> {
    // libc::sysctlbyname is available on FreeBSD; keep the call isolated so the
    // rest of the crate does not grow FreeBSD-specific surface.
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

/// Build a command that re-execs the current process inside a FreeBSD jail
/// with deny paths applied (nullfs RO / placeholder bind-over), mirroring
/// [`crate::bwrap_reexec_command`].
///
/// Returns `None` when:
/// - already inside a jail, or
/// - the privileged helper is missing / Phase 2 not implemented yet
///   (caller must degrade: log and continue without sandbox).
///
/// When implemented, the helper should set [`JAIL_ENV_VAR`] in the child env.
pub fn jail_reexec_command(
    _deny_write: &[&str],
    _deny_read: &[&str],
) -> Option<std::process::Command> {
    if is_inside_jail() {
        return None;
    }
    // Phase 2: invoke `grok-jail-helper` (or root `jail`/`jail_set` + nullfs)
    // with deny_write / deny_read, then exec current_exe + args with
    // JAIL_ENV_VAR=1. Until then, report "no re-exec available" so apply()
    // can log apply_failed and continue unsandboxed.
    tracing::debug!(
        "FreeBSD jail re-exec not implemented yet; set {} only after helper lands",
        JAIL_ENV_VAR
    );
    None
}

/// Profile-aware jail re-exec (twin of `bwrap_reexec_for_profile` on Linux).
///
/// Phase 1a: always `None` (no mounts planned until Phase 2 resolves denies).
pub fn jail_reexec_for_profile(
    _profile: &crate::ProfileName,
    _workspace: &std::path::Path,
) -> Option<std::process::Command> {
    if is_inside_jail() {
        return None;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jail_env_var_name_is_stable() {
        assert_eq!(JAIL_ENV_VAR, "__GROK_INSIDE_JAIL");
    }

    #[test]
    fn reexec_returns_none_while_unimplemented() {
        // Outside a jail marker this is still None until the helper exists.
        if std::env::var_os(JAIL_ENV_VAR).is_none() {
            assert!(jail_reexec_command(&["/tmp"], &[]).is_none());
        }
    }
}
