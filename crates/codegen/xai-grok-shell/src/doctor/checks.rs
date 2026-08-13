//! Default-tier diagnostic checks (unprivileged, offline).

use super::{CheckResult, Severity, Status, timed};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Per-command stall budget (build must not hang on a wedged child).
const CMD_TIMEOUT: Duration = Duration::from_secs(3);

/// Run a command with a hard timeout; kills the child on expiry.
fn output_timeout(mut cmd: Command, timeout: Duration) -> Result<std::process::Output, String> {
    use std::io::Read;
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| e.to_string())?;
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let mut stdout = Vec::new();
                let mut stderr = Vec::new();
                if let Some(mut out) = child.stdout.take() {
                    let _ = out.read_to_end(&mut stdout);
                }
                if let Some(mut err) = child.stderr.take() {
                    let _ = err.read_to_end(&mut stderr);
                }
                return Ok(std::process::Output {
                    status,
                    stdout,
                    stderr,
                });
            }
            Ok(None) if start.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("timed out after {}ms", timeout.as_millis()));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(15)),
            Err(e) => return Err(e.to_string()),
        }
    }
}

/// FreeBSD major floor: 14, 15, and 16 (CURRENT/dev).
pub const MIN_FREEBSD_MAJOR: u32 = 14;

pub async fn run_default_tier(quick: bool) -> Vec<CheckResult> {
    let mut out = Vec::new();

    out.push(timed(check_binary_identity));
    out.push(timed(check_freebsd_version));
    out.push(timed(check_version_info));
    out.push(timed(check_rg));
    out.push(timed(check_grok_home));
    out.push(timed(check_tmpdir));
    out.push(timed(check_config_parse));
    out.push(timed(check_sandbox_backend));
    out.push(timed(check_sandbox_jail_status));
    out.push(timed(check_shell_echo));

    if !quick {
        out.push(timed(check_cwd_readable));
        out.push(timed(check_git_optional));
        out.push(timed(check_update_channel_note));
    }

    out
}

pub fn binary_brand(exe: &str) -> String {
    // Prefer `file(1)` when available; fall back to OS compile target.
    let mut cmd = Command::new("file");
    cmd.arg("-b").arg(exe);
    if let Ok(out) = output_timeout(cmd, CMD_TIMEOUT) {
        if out.status.success() {
            let desc = String::from_utf8_lossy(&out.stdout).to_lowercase();
            if desc.contains("freebsd") {
                return "freebsd-elf".into();
            }
            if desc.contains("linux") || (desc.contains("sysv") && desc.contains("static")) {
                if !desc.contains("freebsd") {
                    return "linux-elf".into();
                }
            }
            if desc.contains("mach-o") {
                return "mach-o".into();
            }
            if desc.contains("pe32") || desc.contains("windows") {
                return "windows-pe".into();
            }
            return format!("other:{}", desc.lines().next().unwrap_or("unknown").trim());
        }
    }
    format!("{}-unknown", std::env::consts::OS)
}

pub fn os_release() -> String {
    #[cfg(target_os = "freebsd")]
    {
        let mut cmd = Command::new("freebsd-version");
        if let Ok(out) = output_timeout(cmd, CMD_TIMEOUT) {
            if out.status.success() {
                return String::from_utf8_lossy(&out.stdout).trim().to_string();
            }
        }
        let mut cmd = Command::new("uname");
        cmd.arg("-r");
        if let Ok(out) = output_timeout(cmd, CMD_TIMEOUT) {
            if out.status.success() {
                return String::from_utf8_lossy(&out.stdout).trim().to_string();
            }
        }
    }
    #[cfg(not(target_os = "freebsd"))]
    {
        let mut cmd = Command::new("uname");
        cmd.arg("-r");
        if let Ok(out) = output_timeout(cmd, CMD_TIMEOUT) {
            if out.status.success() {
                return String::from_utf8_lossy(&out.stdout).trim().to_string();
            }
        }
    }
    "unknown".into()
}

/// Parse FreeBSD major version from strings like `15.1-STABLE`, `14.3-RELEASE`, `14.0-CURRENT`.
/// Returns `None` if the major cannot be parsed.
pub fn parse_freebsd_major(release: &str) -> Option<u32> {
    let first = release.split(['-', '_', ' ']).next().unwrap_or(release);
    let major = first.split('.').next()?;
    major.parse().ok()
}

fn check_freebsd_version() -> CheckResult {
    #[cfg(target_os = "freebsd")]
    {
        let release = os_release();
        match parse_freebsd_major(&release) {
            Some(major) if major >= MIN_FREEBSD_MAJOR => CheckResult {
                id: "platform.freebsd_version".into(),
                tier: "default".into(),
                severity: Severity::Critical,
                status: Status::Pass,
                summary: format!("FreeBSD {release}"),
                detail: Some("Targets FreeBSD 14, 15, and 16 (dev).".into()),
                fix: None,
                requires: vec![],
                duration_ms: 0,
            },
            Some(major) => CheckResult {
                id: "platform.freebsd_version".into(),
                tier: "default".into(),
                severity: Severity::Critical,
                status: Status::Fail,
                summary: format!("FreeBSD {major} ({release})"),
                detail: Some("Needs FreeBSD 14, 15, or 16.".into()),
                fix: None,
                requires: vec![],
                duration_ms: 0,
            },
            None => CheckResult {
                id: "platform.freebsd_version".into(),
                tier: "default".into(),
                severity: Severity::Critical,
                status: Status::Warn,
                summary: format!("FreeBSD version unparsed ({release})"),
                detail: None,
                fix: None,
                requires: vec![],
                duration_ms: 0,
            },
        }
    }

    #[cfg(not(target_os = "freebsd"))]
    {
        CheckResult {
            id: "platform.freebsd_version".into(),
            tier: "default".into(),
            severity: Severity::Info,
            status: Status::Skip,
            summary: "FreeBSD version check N/A".into(),
            detail: None,
            fix: None,
            requires: vec![],
            duration_ms: 0,
        }
    }
}

fn check_binary_identity() -> CheckResult {
    let exe = std::env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "(unknown)".into());
    let brand = binary_brand(&exe);

    #[cfg(target_os = "freebsd")]
    {
        if brand.starts_with("linux") {
            return CheckResult {
                id: "binary.identity".into(),
                tier: "default".into(),
                severity: Severity::Critical,
                status: Status::Fail,
                summary: "Running a Linux-brand binary on FreeBSD".into(),
                detail: Some(format!(
                    "path={exe} brand={brand}. This is likely the linuxulator image, not the native FreeBSD build."
                )),
                fix: Some(
                    "Install the native package (devel/freegrok → bin/freegrok) or build from source on FreeBSD."
                        .into(),
                ),
                requires: vec![],
                duration_ms: 0,
            };
        }
        if brand.starts_with("freebsd") {
            return CheckResult {
                id: "binary.identity".into(),
                tier: "default".into(),
                severity: Severity::Critical,
                status: Status::Pass,
                summary: "Native FreeBSD ELF binary".into(),
                detail: Some(format!("path={exe}")),
                fix: None,
                requires: vec![],
                duration_ms: 0,
            };
        }
    }

    CheckResult {
        id: "binary.identity".into(),
        tier: "default".into(),
        severity: Severity::Info,
        status: Status::Info,
        summary: format!("Binary brand: {brand}"),
        detail: Some(format!("path={exe}")),
        fix: None,
        requires: vec![],
        duration_ms: 0,
    }
}

fn check_version_info() -> CheckResult {
    let ver = std::env::var("GROK_DOCTOR_VERSION_OVERRIDE")
        .unwrap_or_else(|_| env!("CARGO_PKG_VERSION").to_string());
    CheckResult {
        id: "binary.version".into(),
        tier: "default".into(),
        severity: Severity::Info,
        status: Status::Info,
        summary: format!("Version {ver}"),
        detail: None,
        fix: None,
        requires: vec![],
        duration_ms: 0,
    }
}

fn check_rg() -> CheckResult {
    let which = which_bin("rg");
    match which {
        Some(path) => {
            let mut cmd = Command::new(&path);
            cmd.arg("--version");
            let ver = match output_timeout(cmd, CMD_TIMEOUT) {
                Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout)
                    .lines()
                    .next()
                    .unwrap_or("rg")
                    .to_string(),
                Ok(_) => "(rg present but --version failed)".into(),
                Err(e) => format!("(rg --version {e})"),
            };
            // Still pass if binary exists; version probe failure is detail only
            // unless we got a hard timeout with no path usability.
            CheckResult {
                id: "deps.rg".into(),
                tier: "default".into(),
                severity: Severity::Critical,
                status: Status::Pass,
                summary: "ripgrep (rg) available".into(),
                detail: Some(format!("{} — {ver}", path.display())),
                fix: None,
                requires: vec![],
                duration_ms: 0,
            }
        }
        None => CheckResult {
            id: "deps.rg".into(),
            tier: "default".into(),
            severity: Severity::Critical,
            status: Status::Fail,
            summary: "ripgrep (rg) not found on PATH".into(),
            detail: Some("System rg required (no FreeBSD rg bundle).".into()),
            fix: Some(rg_install_fix()),
            requires: vec![],
            duration_ms: 0,
        },
    }
}

fn check_grok_home() -> CheckResult {
    let home = dirs::home_dir();
    let Some(home) = home else {
        return CheckResult {
            id: "fs.grok_home".into(),
            tier: "default".into(),
            severity: Severity::Critical,
            status: Status::Fail,
            summary: "Cannot resolve home directory".into(),
            detail: None,
            fix: Some("Set HOME to a writable user directory".into()),
            requires: vec![],
            duration_ms: 0,
        };
    };
    let grok = xai_grok_config::grok_home();
    match std::fs::create_dir_all(&grok) {
        Ok(()) => {
            let probe = grok.join(".doctor-write-probe");
            match std::fs::write(&probe, b"ok") {
                Ok(()) => {
                    let _ = std::fs::remove_file(&probe);
                    CheckResult {
                        id: "fs.grok_home".into(),
                        tier: "default".into(),
                        severity: Severity::Critical,
                        status: Status::Pass,
                        summary: "config home is writable".into(),
                        detail: Some(grok.display().to_string()),
                        fix: None,
                        requires: vec![],
                        duration_ms: 0,
                    }
                }
                Err(e) => CheckResult {
                    id: "fs.grok_home".into(),
                    tier: "default".into(),
                    severity: Severity::Critical,
                    status: Status::Fail,
                    summary: "config home is not writable".into(),
                    detail: Some(format!("{}: {e}", grok.display())),
                    fix: Some("Fix permissions on ~/.freegrok (or $FREEGROK_HOME / $GROK_HOME)".into()),
                    requires: vec![],
                    duration_ms: 0,
                },
            }
        }
        Err(e) => CheckResult {
            id: "fs.grok_home".into(),
            tier: "default".into(),
            severity: Severity::Critical,
            status: Status::Fail,
            summary: "Cannot create config home".into(),
            detail: Some(e.to_string()),
            fix: Some("Ensure HOME is set and ~/.freegrok (or $FREEGROK_HOME) is writable".into()),
            requires: vec![],
            duration_ms: 0,
        },
    }
}

fn check_tmpdir() -> CheckResult {
    let dir = std::env::temp_dir();
    let probe = dir.join(format!(
        "grok-doctor-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    match std::fs::write(&probe, b"ok") {
        Ok(()) => {
            let _ = std::fs::remove_file(&probe);
            CheckResult {
                id: "fs.tmpdir".into(),
                tier: "default".into(),
                severity: Severity::Critical,
                status: Status::Pass,
                summary: "Temp directory is writable".into(),
                detail: Some(dir.display().to_string()),
                fix: None,
                requires: vec![],
                duration_ms: 0,
            }
        }
        Err(e) => CheckResult {
            id: "fs.tmpdir".into(),
            tier: "default".into(),
            severity: Severity::Critical,
            status: Status::Fail,
            summary: "Temp directory is not writable".into(),
            detail: Some(format!("{}: {e}", dir.display())),
            fix: Some("Fix TMPDIR /tmp permissions or free disk space".into()),
            requires: vec![],
            duration_ms: 0,
        },
    }
}

fn check_config_parse() -> CheckResult {
    match crate::config::load_effective_config_disk_only() {
        Ok(_) => CheckResult {
            id: "config.parse".into(),
            tier: "default".into(),
            severity: Severity::Critical,
            status: Status::Pass,
            summary: "Config loads successfully".into(),
            detail: Some("load_effective_config_disk_only".into()),
            fix: None,
            requires: vec![],
            duration_ms: 0,
        },
        Err(e) => CheckResult {
            id: "config.parse".into(),
            tier: "default".into(),
            severity: Severity::Critical,
            status: Status::Fail,
            summary: "Config failed to load".into(),
            detail: Some(e.to_string()),
            fix: Some("Fix syntax in ~/.grok/config.toml or project config".into()),
            requires: vec![],
            duration_ms: 0,
        },
    }
}

fn check_sandbox_backend() -> CheckResult {
    // Platform (not enforce-gated host): dependents build sandbox without
    // default features; doctor still reports the OS backend label.
    let kind = xai_grok_sandbox::platform_backend_kind();
    let backend = kind.as_str();
    let detail = kind.doctor_detail();

    CheckResult {
        id: "sandbox.backend".into(),
        tier: "default".into(),
        severity: Severity::Info,
        status: Status::Info,
        summary: format!("Sandbox backend: {backend}"),
        detail: Some(detail.into()),
        fix: None,
        requires: vec![],
        duration_ms: 0,
    }
}

fn check_sandbox_jail_status() -> CheckResult {
    #[cfg(target_os = "freebsd")]
    {
        let status = xai_grok_sandbox::jail_backend_status();
        let helper = status
            .helper
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "(none)".into());
        let detail = format!(
            "inside_jail={} sysctl_jailed={:?} marker_env={} helper={helper}\n\
             Unprivileged processes cannot create jails (jail_set → EPERM).\n\
             Without a privileged grok-jail-helper, sandbox degrades and the agent still runs.",
            status.inside_jail, status.sysctl_jailed, status.marker_env
        );

        if status.inside_jail {
            return CheckResult {
                id: "sandbox.jail_status".into(),
                tier: "default".into(),
                severity: Severity::Warn,
                status: Status::Pass,
                summary: "Running inside a FreeBSD jail".into(),
                detail: Some(detail),
                fix: None,
                requires: vec![],
                duration_ms: 0,
            };
        }

        if status.helper.is_some() {
            return CheckResult {
                id: "sandbox.jail_status".into(),
                tier: "default".into(),
                severity: Severity::Warn,
                status: Status::Warn,
                summary: "Jail helper found but process is not jailed".into(),
                detail: Some(detail),
                fix: Some(
                    "Sandbox apply/re-exec is Phase 2; re-run with --sandbox-deep when helper supports dry-run."
                        .into(),
                ),
                requires: vec![],
                duration_ms: 0,
            };
        }

        return CheckResult {
            id: "sandbox.jail_status".into(),
            tier: "default".into(),
            severity: Severity::Warn,
            status: Status::Warn,
            summary: "OS isolation optional (no jail helper)".into(),
            detail: Some(detail),
            fix: Some(
                "Optional: `grok jail setup` (dry-run). Agent works without it. doctor --ci ignores this warn."
                    .into(),
            ),
            requires: vec![],
            duration_ms: 0,
        };
    }

    #[cfg(not(target_os = "freebsd"))]
    {
        CheckResult {
            id: "sandbox.jail_status".into(),
            tier: "default".into(),
            severity: Severity::Info,
            status: Status::Skip,
            summary: "Jail status N/A on this OS".into(),
            detail: Some(xai_grok_sandbox::jail_backend_status_string()),
            fix: None,
            requires: vec![],
            duration_ms: 0,
        }
    }
}

/// Opt-in deep sandbox check: never creates a jail; never calls sudo.
pub fn sandbox_deep_check() -> CheckResult {
    #[cfg(target_os = "freebsd")]
    {
        let status = xai_grok_sandbox::jail_backend_status();
        if status.helper.is_none() {
            return CheckResult {
                id: "sandbox.deep".into(),
                tier: "sandbox-deep".into(),
                severity: Severity::Warn,
                status: Status::Skip,
                summary: "Sandbox-deep skipped: no jail helper".into(),
                detail: Some(
                    "Cannot exercise jail re-exec without grok-jail-helper. Doctor never prompts for sudo/doas."
                        .into(),
                ),
                fix: Some(
                    "Install grok-jail-helper with narrow privileges (setuid or doas rule), then re-run --sandbox-deep."
                        .into(),
                ),
                requires: vec!["helper".into()],
                duration_ms: 0,
            };
        }
        // Helper present but full dry-run protocol is Phase 2 / D3.
        return CheckResult {
            id: "sandbox.deep".into(),
            tier: "sandbox-deep".into(),
            severity: Severity::Warn,
            status: Status::Warn,
            summary: "Jail helper present; deep deny probe not implemented yet".into(),
            detail: Some(format!("helper={}", status.helper.unwrap().display())),
            fix: Some("Await Phase 2 helper --dry-run protocol".into()),
            requires: vec!["helper".into()],
            duration_ms: 0,
        };
    }

    #[cfg(not(target_os = "freebsd"))]
    {
        CheckResult {
            id: "sandbox.deep".into(),
            tier: "sandbox-deep".into(),
            severity: Severity::Info,
            status: Status::Skip,
            summary: "Sandbox-deep FreeBSD jail path N/A".into(),
            detail: Some(
                "On Linux/macOS use existing sandbox profiles / nono support_info.".into(),
            ),
            fix: None,
            requires: vec![],
            duration_ms: 0,
        }
    }
}

fn check_shell_echo() -> CheckResult {
    // Prefer a portable shell. FreeBSD has /bin/sh.
    let sh = if Path::new("/bin/sh").exists() {
        "/bin/sh"
    } else {
        "sh"
    };
    let mut cmd = Command::new(sh);
    cmd.args(["-c", "echo grok-doctor-ok"]);
    match output_timeout(cmd, CMD_TIMEOUT) {
        Ok(o) if o.status.success() => {
            let stdout = String::from_utf8_lossy(&o.stdout);
            if stdout.contains("grok-doctor-ok") {
                CheckResult {
                    id: "tools.shell_echo".into(),
                    tier: "default".into(),
                    severity: Severity::Critical,
                    status: Status::Pass,
                    summary: "Shell spawn works".into(),
                    detail: Some(format!("{sh} -c echo")),
                    fix: None,
                    requires: vec![],
                    duration_ms: 0,
                }
            } else {
                CheckResult {
                    id: "tools.shell_echo".into(),
                    tier: "default".into(),
                    severity: Severity::Critical,
                    status: Status::Fail,
                    summary: "Shell ran but output unexpected".into(),
                    detail: Some(stdout.into()),
                    fix: None,
                    requires: vec![],
                    duration_ms: 0,
                }
            }
        }
        Ok(o) => CheckResult {
            id: "tools.shell_echo".into(),
            tier: "default".into(),
            severity: Severity::Critical,
            status: Status::Fail,
            summary: "Shell spawn failed".into(),
            detail: Some(format!(
                "status={} stderr={}",
                o.status,
                String::from_utf8_lossy(&o.stderr)
            )),
            fix: Some("Ensure /bin/sh exists and is executable".into()),
            requires: vec![],
            duration_ms: 0,
        },
        Err(e) => CheckResult {
            id: "tools.shell_echo".into(),
            tier: "default".into(),
            severity: Severity::Critical,
            status: Status::Fail,
            summary: "Could not spawn shell".into(),
            detail: Some(e),
            fix: Some("Ensure a POSIX shell is installed".into()),
            requires: vec![],
            duration_ms: 0,
        },
    }
}

fn check_cwd_readable() -> CheckResult {
    let cwd = std::env::current_dir();
    match cwd {
        Ok(p) => match std::fs::read_dir(&p) {
            Ok(_) => CheckResult {
                id: "fs.cwd".into(),
                tier: "default".into(),
                severity: Severity::Warn,
                status: Status::Pass,
                summary: "Current directory is readable".into(),
                detail: Some(p.display().to_string()),
                fix: None,
                requires: vec![],
                duration_ms: 0,
            },
            Err(e) => CheckResult {
                id: "fs.cwd".into(),
                tier: "default".into(),
                severity: Severity::Warn,
                status: Status::Warn,
                summary: "Current directory not readable".into(),
                detail: Some(format!("{}: {e}", p.display())),
                fix: None,
                requires: vec![],
                duration_ms: 0,
            },
        },
        Err(e) => CheckResult {
            id: "fs.cwd".into(),
            tier: "default".into(),
            severity: Severity::Warn,
            status: Status::Warn,
            summary: "Cannot resolve current directory".into(),
            detail: Some(e.to_string()),
            fix: None,
            requires: vec![],
            duration_ms: 0,
        },
    }
}

fn check_git_optional() -> CheckResult {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let git_dir = cwd.join(".git");
    if !git_dir.exists() {
        return CheckResult {
            id: "git.optional".into(),
            tier: "default".into(),
            severity: Severity::Info,
            status: Status::Skip,
            summary: "No .git in CWD".into(),
            detail: None,
            fix: None,
            requires: vec![],
            duration_ms: 0,
        };
    }
    let mut cmd = Command::new("git");
    cmd.args(["status", "--porcelain"]).current_dir(&cwd);
    match output_timeout(cmd, CMD_TIMEOUT) {
        Ok(o) if o.status.success() => CheckResult {
            id: "git.optional".into(),
            tier: "default".into(),
            severity: Severity::Warn,
            status: Status::Pass,
            summary: "git status works in CWD".into(),
            detail: Some(cwd.display().to_string()),
            fix: None,
            requires: vec![],
            duration_ms: 0,
        },
        Ok(o) => CheckResult {
            id: "git.optional".into(),
            tier: "default".into(),
            severity: Severity::Warn,
            status: Status::Warn,
            summary: "git status failed".into(),
            detail: Some(String::from_utf8_lossy(&o.stderr).into()),
            fix: None,
            requires: vec![],
            duration_ms: 0,
        },
        Err(e) => CheckResult {
            id: "git.optional".into(),
            tier: "default".into(),
            severity: Severity::Warn,
            status: Status::Warn,
            summary: "git probe failed".into(),
            detail: Some(e),
            fix: None,
            requires: vec![],
            duration_ms: 0,
        },
    }
}

fn check_update_channel_note() -> CheckResult {
    #[cfg(target_os = "freebsd")]
    {
        CheckResult {
            id: "update.channel_note".into(),
            tier: "default".into(),
            severity: Severity::Info,
            status: Status::Info,
            summary: "FreeBSD has no official native auto-update channel yet".into(),
            detail: Some(
                "Official downloads are linux-x86_64 (linuxulator). Prefer ports/pkg freegrok (bin/freegrok)."
                    .into(),
            ),
            fix: Some("Update via FreeBSD ports/pkg when available; leave Linux ~/.freegrok / ~/.grok install alone.".into()),
            requires: vec![],
            duration_ms: 0,
        }
    }
    #[cfg(not(target_os = "freebsd"))]
    {
        CheckResult {
            id: "update.channel_note".into(),
            tier: "default".into(),
            severity: Severity::Info,
            status: Status::Info,
            summary: "Use `grok update --check` for release channel status".into(),
            detail: None,
            fix: None,
            requires: vec![],
            duration_ms: 0,
        }
    }
}

fn which_bin(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn rg_install_fix() -> String {
    #[cfg(target_os = "freebsd")]
    {
        "pkg install ripgrep   # or: cd /usr/ports/textproc/ripgrep && make install".into()
    }
    #[cfg(not(target_os = "freebsd"))]
    {
        "Install ripgrep (rg) for your OS and ensure it is on PATH".into()
    }
}
