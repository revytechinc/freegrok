//! OpenSSH-backed remote transport for config import.
//!
//! **Why system `ssh`:** OpenSSH already loads `~/.ssh/config`, agent keys
//! (`SSH_AUTH_SOCK`), `IdentityFile` entries, `Host` aliases, `ProxyJump`,
//! `User`, `Port`, and `CertificateFile`. Reimplementing that in-process would
//! regress user setups.
//!
//! **Auth order (default):**
//! 1. Public keys from agent + files listed in config / `-i`
//! 2. Password / keyboard-interactive when a password is supplied (via
//!    `SSH_ASKPASS` + `SSH_ASKPASS_REQUIRE=force`) or when the session is
//!    interactive and the remote offers password auth
//!
//! Password auth is **never** disabled by `BatchMode` when a password is
//! provided. Keys-only non-interactive scans use `BatchMode=yes` so a missing
//! key fails fast instead of hanging on a password prompt.

use std::ffi::OsString;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use thiserror::Error;

/// How authentication should behave for a remote connection.
#[derive(Debug, Clone, Default)]
pub struct SshAuth {
    /// Optional password. When set, OpenSSH is configured to try publickey
    /// first, then password / keyboard-interactive via `SSH_ASKPASS`.
    pub password: Option<String>,
    /// Extra `-i` identity files (in addition to config / defaults).
    pub identity_files: Vec<PathBuf>,
    /// Override config path (`-F`). Default: OpenSSH uses `~/.ssh/config`.
    pub config_file: Option<PathBuf>,
    /// When true and **no** password is set, pass `-o BatchMode=yes` so
    /// password prompts cannot hang automation.
    pub batch_if_no_password: bool,
    /// Prefer password over keys (still allows keys first unless this is set).
    pub prefer_password: bool,
    /// Extra raw `-o key=value` options.
    pub extra_options: Vec<(String, String)>,
    /// Connect timeout seconds (`ConnectTimeout`).
    pub connect_timeout_secs: Option<u64>,
    /// Path to `ssh` binary (default: `ssh` on `PATH`).
    pub ssh_bin: Option<PathBuf>,
}

impl SshAuth {
    /// Typical import scan: use config + keys; fail fast if no key matches
    /// (no password hang). Call [`SshAuth::with_password`] when the user
    /// supplies a password.
    pub fn keys_and_config() -> Self {
        Self {
            batch_if_no_password: true,
            connect_timeout_secs: Some(15),
            ..Default::default()
        }
    }

    pub fn with_password(mut self, password: impl Into<String>) -> Self {
        self.password = Some(password.into());
        // Never batch when we intend to use password auth.
        self.batch_if_no_password = false;
        self
    }

    pub fn with_identity(mut self, path: impl Into<PathBuf>) -> Self {
        self.identity_files.push(path.into());
        self
    }

    pub fn with_config_file(mut self, path: impl Into<PathBuf>) -> Self {
        self.config_file = Some(path.into());
        self
    }
}

/// Remote host as accepted by OpenSSH (Host alias, hostname, or `user@host`).
#[derive(Debug, Clone)]
pub struct SshTarget {
    /// Exactly what is passed as the final `ssh` destination argument.
    /// Examples: `mybox`, `user@192.168.1.10`, `git@github.com`.
    pub destination: String,
    /// Optional explicit port (`-p`). Prefer Host config when possible.
    pub port: Option<u16>,
    /// Optional explicit user (`-l`). Prefer Host config when possible.
    pub user: Option<String>,
}

impl SshTarget {
    pub fn new(destination: impl Into<String>) -> Self {
        Self {
            destination: destination.into(),
            port: None,
            user: None,
        }
    }

    pub fn with_port(mut self, port: u16) -> Self {
        self.port = Some(port);
        self
    }

    pub fn with_user(mut self, user: impl Into<String>) -> Self {
        self.user = Some(user.into());
        self
    }
}

#[derive(Debug, Error)]
pub enum SshError {
    #[error("ssh binary not found (install OpenSSH client)")]
    SshNotFound,
    #[error("ssh failed (exit {status}): {stderr}")]
    RemoteFailed { status: i32, stderr: String },
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("askpass helper: {0}")]
    Askpass(String),
    #[error("empty remote command")]
    EmptyCommand,
}

/// Built argv + env for an `ssh` invocation (testable without network).
#[derive(Debug, Clone)]
pub struct SshInvocation {
    pub program: PathBuf,
    pub args: Vec<OsString>,
    pub env: Vec<(OsString, OsString)>,
    /// When set, parent must keep this dir alive until the process exits.
    pub askpass_dir: Option<PathBuf>,
}

impl Drop for SshInvocation {
    fn drop(&mut self) {
        if let Some(dir) = self.askpass_dir.take() {
            let _ = fs::remove_dir_all(dir);
        }
    }
}

/// Build OpenSSH argv/env. Does not spawn.
pub fn build_ssh_invocation(
    target: &SshTarget,
    auth: &SshAuth,
    remote_command: &str,
) -> Result<SshInvocation, SshError> {
    if remote_command.is_empty() {
        return Err(SshError::EmptyCommand);
    }

    let program = auth
        .ssh_bin
        .clone()
        .unwrap_or_else(|| PathBuf::from("ssh"));

    let mut args: Vec<OsString> = Vec::new();
    // Disable local ssh ControlMaster surprises for short probe sessions.
    args.push("-o".into());
    args.push("ControlMaster=no".into());
    args.push("-o".into());
    args.push("ControlPath=none".into());

    // Keep agent + config identity search; do NOT set IdentitiesOnly unless
    // the user only wants explicit -i files (not the default).
    if auth.password.is_some() {
        // Keys first, then password methods. OpenSSH still reads IdentityFile
        // from ~/.ssh/config and SSH_AUTH_SOCK.
        let order = if auth.prefer_password {
            "password,keyboard-interactive,publickey"
        } else {
            "publickey,password,keyboard-interactive"
        };
        args.push("-o".into());
        args.push(format!("PreferredAuthentications={order}").into());
        args.push("-o".into());
        args.push("PubkeyAuthentication=yes".into());
        args.push("-o".into());
        args.push("PasswordAuthentication=yes".into());
        args.push("-o".into());
        args.push("KbdInteractiveAuthentication=yes".into());
        args.push("-o".into());
        args.push("NumberOfPasswordPrompts=1".into());
        // Never BatchMode when password supplied — BatchMode disables password.
        args.push("-o".into());
        args.push("BatchMode=no".into());
    } else if auth.batch_if_no_password {
        args.push("-o".into());
        args.push("BatchMode=yes".into());
    }

    if let Some(secs) = auth.connect_timeout_secs {
        args.push("-o".into());
        args.push(format!("ConnectTimeout={secs}").into());
    }

    for (k, v) in &auth.extra_options {
        args.push("-o".into());
        args.push(format!("{k}={v}").into());
    }

    if let Some(cfg) = &auth.config_file {
        args.push("-F".into());
        args.push(cfg.as_os_str().to_owned());
    }

    for id in &auth.identity_files {
        args.push("-i".into());
        args.push(id.as_os_str().to_owned());
    }

    if let Some(port) = target.port {
        args.push("-p".into());
        args.push(port.to_string().into());
    }
    if let Some(user) = &target.user {
        args.push("-l".into());
        args.push(user.into());
    }

    // Destination last among options, then remote command as single argv so
    // the remote shell is not used unless the command itself is a shell.
    args.push(target.destination.as_str().into());
    args.push(remote_command.into());

    let mut env: Vec<(OsString, OsString)> = Vec::new();
    // Preserve agent for key auth.
    if let Ok(sock) = std::env::var("SSH_AUTH_SOCK") {
        env.push(("SSH_AUTH_SOCK".into(), sock.into()));
    }
    if let Ok(pid) = std::env::var("SSH_AGENT_PID") {
        env.push(("SSH_AGENT_PID".into(), pid.into()));
    }

    let mut askpass_dir = None;
    if let Some(password) = &auth.password {
        let dir = make_askpass_helper(password)?;
        let helper = dir.join("askpass");
        env.push(("SSH_ASKPASS".into(), helper.into()));
        // OpenSSH 8.4+: force askpass even without a DISPLAY / TTY.
        env.push(("SSH_ASKPASS_REQUIRE".into(), "force".into()));
        // Some older builds still want DISPLAY set to enable askpass path.
        if std::env::var_os("DISPLAY").is_none() {
            env.push(("DISPLAY".into(), "grok-ssh-askpass".into()));
        }
        // Avoid inheriting a conflicting askpass from the parent.
        askpass_dir = Some(dir);
    }

    Ok(SshInvocation {
        program,
        args,
        env,
        askpass_dir,
    })
}

/// Write a mode-0700 askpass script that prints the password once.
fn make_askpass_helper(password: &str) -> Result<PathBuf, SshError> {
    let dir = std::env::temp_dir().join(format!(
        "grok-ssh-askpass-{}-{}",
        std::process::id(),
        now_nanos()
    ));
    fs::create_dir_all(&dir).map_err(SshError::Io)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))?;
    }

    let helper = dir.join("askpass");
    // Escape for single-quoted shell: ' -> '\'' 
    let escaped = password.replace('\'', "'\\''");
    let script = format!("#!/bin/sh\nprintf '%s\\n' '{escaped}'\n");
    {
        let mut f = fs::File::create(&helper)?;
        f.write_all(script.as_bytes())?;
        f.sync_all()?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&helper, fs::Permissions::from_mode(0o700))?;
    }
    #[cfg(not(unix))]
    {
        // Windows: OpenSSH may accept a .bat; keep sh for WSL/Git-Bash.
        let _ = &helper;
    }

    if !helper.is_file() {
        return Err(SshError::Askpass("failed to create helper".into()));
    }
    Ok(dir)
}

fn now_nanos() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}

/// Run a remote command via OpenSSH; returns stdout on success.
pub fn ssh_exec(target: &SshTarget, auth: &SshAuth, remote_command: &str) -> Result<Vec<u8>, SshError> {
    let inv = build_ssh_invocation(target, auth, remote_command)?;
    let mut cmd = Command::new(&inv.program);
    cmd.args(&inv.args);
    // Clear potentially conflicting askpass from parent, then apply ours.
    cmd.env_remove("SSH_ASKPASS");
    cmd.env_remove("SSH_ASKPASS_REQUIRE");
    for (k, v) in &inv.env {
        cmd.env(k, v);
    }
    // Do not inherit stdin (prevents accidental password consumption).
    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    let output = cmd.output().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            SshError::SshNotFound
        } else {
            SshError::Io(e)
        }
    })?;

    finish_output(output)
}

fn finish_output(output: Output) -> Result<Vec<u8>, SshError> {
    if output.status.success() {
        return Ok(output.stdout);
    }
    let status = output.status.code().unwrap_or(-1);
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    Err(SshError::RemoteFailed { status, stderr })
}

/// Async wrapper around [`ssh_exec`] (runs on blocking pool).
pub async fn ssh_exec_async(
    target: SshTarget,
    auth: SshAuth,
    remote_command: String,
) -> Result<Vec<u8>, SshError> {
    tokio::task::spawn_blocking(move || ssh_exec(&target, &auth, &remote_command))
        .await
        .map_err(|e| {
            SshError::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("join: {e}"),
            ))
        })?
}

/// Probe: resolve effective config for a destination (`ssh -G`).
/// Uses local OpenSSH only (no network) — validates Host alias / IdentityFile.
pub fn ssh_resolve_config(target: &SshTarget, auth: &SshAuth) -> Result<SshResolvedConfig, SshError> {
    let program = auth
        .ssh_bin
        .clone()
        .unwrap_or_else(|| PathBuf::from("ssh"));
    let mut cmd = Command::new(program);
    cmd.arg("-G");
    if let Some(cfg) = &auth.config_file {
        cmd.arg("-F").arg(cfg);
    }
    for id in &auth.identity_files {
        cmd.arg("-i").arg(id);
    }
    if let Some(port) = target.port {
        cmd.arg("-p").arg(port.to_string());
    }
    if let Some(user) = &target.user {
        cmd.arg("-l").arg(user);
    }
    cmd.arg(&target.destination);
    cmd.stdin(Stdio::null());
    let output = cmd.output().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            SshError::SshNotFound
        } else {
            SshError::Io(e)
        }
    })?;
    if !output.status.success() {
        return finish_output(output).map(|_| unreachable!());
    }
    Ok(SshResolvedConfig::parse(&String::from_utf8_lossy(
        &output.stdout,
    )))
}

/// Subset of `ssh -G` fields useful for diagnostics / UI.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SshResolvedConfig {
    pub user: Option<String>,
    pub hostname: Option<String>,
    pub port: Option<u16>,
    pub identity_files: Vec<String>,
    pub proxy_jump: Option<String>,
    pub password_authentication: Option<bool>,
    pub pubkey_authentication: Option<bool>,
    pub batchmode: Option<bool>,
}

impl SshResolvedConfig {
    pub fn parse(stdout: &str) -> Self {
        let mut c = Self::default();
        for line in stdout.lines() {
            let mut parts = line.splitn(2, char::is_whitespace);
            let key = parts.next().unwrap_or("").to_ascii_lowercase();
            let val = parts.next().unwrap_or("").trim();
            match key.as_str() {
                "user" => c.user = Some(val.to_string()),
                "hostname" => c.hostname = Some(val.to_string()),
                "port" => c.port = val.parse().ok(),
                "identityfile" => c.identity_files.push(val.to_string()),
                "proxyjump" => {
                    if !val.is_empty() && val != "none" {
                        c.proxy_jump = Some(val.to_string());
                    }
                }
                "passwordauthentication" => c.password_authentication = parse_yes_no(val),
                "pubkeyauthentication" => c.pubkey_authentication = parse_yes_no(val),
                "batchmode" => c.batchmode = parse_yes_no(val),
                _ => {}
            }
        }
        c
    }
}

fn parse_yes_no(s: &str) -> Option<bool> {
    match s.to_ascii_lowercase().as_str() {
        "yes" | "true" | "1" => Some(true),
        "no" | "false" | "0" => Some(false),
        _ => None,
    }
}

/// Expand `~` in IdentityFile paths from `ssh -G` (diagnostic only).
pub fn expand_tilde(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = dirs_home() {
            return home.join(rest);
        }
    }
    if path == "~" {
        if let Some(home) = dirs_home() {
            return home;
        }
    }
    PathBuf::from(path)
}

fn dirs_home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
}

/// List which of `paths` exist on the remote (POSIX `test -e`).
pub fn remote_paths_exist(
    target: &SshTarget,
    auth: &SshAuth,
    paths: &[impl AsRef<str>],
) -> Result<Vec<(String, bool)>, SshError> {
    // Build a tiny shell loop; paths are single-quoted.
    let mut script = String::from("set --");
    for p in paths {
        let p = p.as_ref();
        script.push(' ');
        script.push_str(&shell_single_quote(p));
    }
    script.push_str(
        "; for p do if [ -e \"$p\" ]; then echo \"1 $p\"; else echo \"0 $p\"; fi; done",
    );
    let remote = format!("sh -c {}", shell_single_quote(&script));
    let out = ssh_exec(target, auth, &remote)?;
    let text = String::from_utf8_lossy(&out);
    let mut rows = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let (flag, path) = line.split_once(' ').unwrap_or((line, ""));
        rows.push((path.to_string(), flag == "1"));
    }
    Ok(rows)
}

/// Read a remote file via `ssh … cat -- 'path'` (binary-safe stdout).
pub fn remote_read_file(
    target: &SshTarget,
    auth: &SshAuth,
    path: &str,
) -> Result<Vec<u8>, SshError> {
    let remote = format!("cat -- {}", shell_single_quote(path));
    ssh_exec(target, auth, &remote)
}

/// Detect remote OS via `uname -s`.
pub fn remote_uname(target: &SshTarget, auth: &SshAuth) -> Result<String, SshError> {
    let out = ssh_exec(target, auth, "uname -s")?;
    Ok(String::from_utf8_lossy(&out).trim().to_string())
}

/// Remote `$HOME` (or Windows USERPROFILE via uname branch).
pub fn remote_home(target: &SshTarget, auth: &SshAuth) -> Result<String, SshError> {
    let out = ssh_exec(
        target,
        auth,
        "sh -c 'printf %s \"$HOME\"'",
    )?;
    let s = String::from_utf8_lossy(&out).trim().to_string();
    if s.is_empty() {
        return Err(SshError::RemoteFailed {
            status: 1,
            stderr: "remote HOME empty".into(),
        });
    }
    Ok(s)
}

fn shell_single_quote(s: &str) -> String {
    // 'foo'bar'baz' -> 'foo'\''bar'\''baz'
    let mut out = String::from("'");
    for ch in s.chars() {
        if ch == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(ch);
        }
    }
    out.push('\'');
    out
}

/// High-level client holding target + auth for repeated probes.
#[derive(Debug, Clone)]
pub struct SshSession {
    pub target: SshTarget,
    pub auth: SshAuth,
}

impl SshSession {
    pub fn new(destination: impl Into<String>) -> Self {
        Self {
            target: SshTarget::new(destination),
            auth: SshAuth::keys_and_config(),
        }
    }

    pub fn password(mut self, password: impl Into<String>) -> Self {
        self.auth = self.auth.with_password(password);
        self
    }

    pub fn identity(mut self, path: impl Into<PathBuf>) -> Self {
        self.auth = self.auth.with_identity(path);
        self
    }

    pub fn config_file(mut self, path: impl Into<PathBuf>) -> Self {
        self.auth = self.auth.with_config_file(path);
        self
    }

    pub fn user(mut self, user: impl Into<String>) -> Self {
        self.target = self.target.with_user(user);
        self
    }

    pub fn port(mut self, port: u16) -> Self {
        self.target = self.target.with_port(port);
        self
    }

    pub fn resolve_config(&self) -> Result<SshResolvedConfig, SshError> {
        ssh_resolve_config(&self.target, &self.auth)
    }

    pub fn exec(&self, remote_command: &str) -> Result<Vec<u8>, SshError> {
        ssh_exec(&self.target, &self.auth, remote_command)
    }

    pub fn uname(&self) -> Result<String, SshError> {
        remote_uname(&self.target, &self.auth)
    }

    pub fn home(&self) -> Result<String, SshError> {
        remote_home(&self.target, &self.auth)
    }

    pub fn path_exists(&self, path: &str) -> Result<bool, SshError> {
        let rows = remote_paths_exist(&self.target, &self.auth, &[path])?;
        Ok(rows.first().map(|(_, e)| *e).unwrap_or(false))
    }

    pub fn read_file(&self, path: &str) -> Result<Vec<u8>, SshError> {
        remote_read_file(&self.target, &self.auth, path)
    }

    /// Smoke-test connectivity (auth included).
    pub fn ping(&self) -> Result<(), SshError> {
        let out = self.exec("echo grok-ssh-ok")?;
        let s = String::from_utf8_lossy(&out);
        if s.contains("grok-ssh-ok") {
            Ok(())
        } else {
            Err(SshError::RemoteFailed {
                status: 1,
                stderr: format!("unexpected ping output: {s:?}"),
            })
        }
    }
}

/// Default locations OpenSSH searches for keys (for UI hints).
pub fn default_ssh_key_candidates() -> Vec<PathBuf> {
    let Some(home) = dirs_home() else {
        return Vec::new();
    };
    let ssh = home.join(".ssh");
    [
        "id_ed25519",
        "id_ecdsa",
        "id_rsa",
        "id_dsa",
        "id_ed25519_sk",
        "id_ecdsa_sk",
    ]
    .into_iter()
    .map(|n| ssh.join(n))
    .filter(|p| p.is_file())
    .collect()
}

/// Whether an ssh-agent appears available.
pub fn ssh_agent_present() -> bool {
    match std::env::var_os("SSH_AUTH_SOCK") {
        Some(s) if !s.is_empty() => {
            let p = PathBuf::from(&s);
            // Socket path existence is enough of a hint.
            p.exists() || cfg!(unix)
        }
        _ => false,
    }
}

/// Path to user SSH config if present.
pub fn default_ssh_config_path() -> Option<PathBuf> {
    let home = dirs_home()?;
    let p = home.join(".ssh").join("config");
    p.is_file().then_some(p)
}

/// Human summary for diagnostics before connecting.
pub fn auth_diagnostics(target: &SshTarget, auth: &SshAuth) -> String {
    let mut lines = vec![
        format!("destination: {}", target.destination),
        format!(
            "ssh config: {}",
            auth.config_file
                .as_ref()
                .map(|p| p.display().to_string())
                .or_else(|| default_ssh_config_path().map(|p| p.display().to_string()))
                .unwrap_or_else(|| "(OpenSSH default ~/.ssh/config)".into())
        ),
        format!("agent: {}", if ssh_agent_present() { "yes" } else { "no" }),
        format!(
            "password: {}",
            if auth.password.is_some() {
                "provided (askpass)"
            } else {
                "not provided"
            }
        ),
        format!(
            "batch_if_no_password: {}",
            auth.batch_if_no_password && auth.password.is_none()
        ),
    ];
    let keys = default_ssh_key_candidates();
    if keys.is_empty() {
        lines.push("default keys in ~/.ssh: (none found)".into());
    } else {
        lines.push(format!(
            "default keys in ~/.ssh: {}",
            keys.iter()
                .map(|p| p
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("?")
                    .to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    for id in &auth.identity_files {
        lines.push(format!("extra -i: {}", id.display()));
    }
    if let Ok(resolved) = ssh_resolve_config(target, auth) {
        if let Some(h) = &resolved.hostname {
            lines.push(format!("resolved hostname: {h}"));
        }
        if let Some(u) = &resolved.user {
            lines.push(format!("resolved user: {u}"));
        }
        if let Some(p) = resolved.port {
            lines.push(format!("resolved port: {p}"));
        }
        if !resolved.identity_files.is_empty() {
            lines.push(format!(
                "resolved IdentityFile: {}",
                resolved.identity_files.join(", ")
            ));
        }
        if let Some(j) = &resolved.proxy_jump {
            lines.push(format!("ProxyJump: {j}"));
        }
        if let Some(v) = resolved.password_authentication {
            lines.push(format!("PasswordAuthentication: {v}"));
        }
        if let Some(v) = resolved.pubkey_authentication {
            lines.push(format!("PubkeyAuthentication: {v}"));
        }
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_quote_escapes_single_quotes() {
        assert_eq!(shell_single_quote("abc"), "'abc'");
        assert_eq!(shell_single_quote("a'b"), "'a'\\''b'");
    }

    #[test]
    fn build_invocation_uses_config_and_no_batch_with_password() {
        let target = SshTarget::new("myhost");
        let auth = SshAuth::keys_and_config().with_password("s3cret");
        let inv = build_ssh_invocation(&target, &auth, "uname -s").unwrap();
        let args: Vec<String> = inv
            .args
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert!(args.iter().any(|a| a == "BatchMode=no"));
        assert!(!args.iter().any(|a| a == "BatchMode=yes"));
        assert!(args.iter().any(|a| a.contains("PreferredAuthentications=")));
        assert!(args.iter().any(|a| a.contains("password")));
        assert_eq!(args[args.len() - 2], "myhost");
        assert_eq!(args[args.len() - 1], "uname -s");
        let env_keys: Vec<_> = inv
            .env
            .iter()
            .map(|(k, _)| k.to_string_lossy().into_owned())
            .collect();
        assert!(env_keys.iter().any(|k| k == "SSH_ASKPASS"));
        assert!(env_keys.iter().any(|k| k == "SSH_ASKPASS_REQUIRE"));
        // askpass helper exists
        let ask = inv
            .env
            .iter()
            .find(|(k, _)| k == "SSH_ASKPASS")
            .map(|(_, v)| PathBuf::from(v))
            .unwrap();
        assert!(ask.is_file());
    }

    #[test]
    fn build_invocation_batch_without_password() {
        let target = SshTarget::new("box");
        let auth = SshAuth::keys_and_config();
        let inv = build_ssh_invocation(&target, &auth, "true").unwrap();
        let args: Vec<String> = inv
            .args
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert!(args.iter().any(|a| a == "BatchMode=yes"));
        let env_keys: Vec<_> = inv
            .env
            .iter()
            .map(|(k, _)| k.to_string_lossy().into_owned())
            .collect();
        assert!(!env_keys.iter().any(|k| k == "SSH_ASKPASS"));
    }

    #[test]
    fn build_invocation_identity_and_port_user() {
        let target = SshTarget::new("host.example").with_port(2222).with_user("deploy");
        let auth = SshAuth::keys_and_config()
            .with_identity("/tmp/id_test")
            .with_config_file("/tmp/ssh_config_test");
        let inv = build_ssh_invocation(&target, &auth, "id").unwrap();
        let args: Vec<String> = inv
            .args
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert!(args.windows(2).any(|w| w[0] == "-p" && w[1] == "2222"));
        assert!(args.windows(2).any(|w| w[0] == "-l" && w[1] == "deploy"));
        assert!(args.windows(2).any(|w| w[0] == "-i" && w[1] == "/tmp/id_test"));
        assert!(args
            .windows(2)
            .any(|w| w[0] == "-F" && w[1] == "/tmp/ssh_config_test"));
    }

    #[test]
    fn prefer_password_reorders_auth() {
        let target = SshTarget::new("h");
        let mut auth = SshAuth::keys_and_config().with_password("x");
        auth.prefer_password = true;
        let inv = build_ssh_invocation(&target, &auth, "true").unwrap();
        let joined = inv
            .args
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join(" ");
        assert!(joined.contains("PreferredAuthentications=password,keyboard-interactive,publickey"));
    }

    #[test]
    fn parse_ssh_g_output() {
        let sample = "\
user alice
hostname 10.0.0.5
port 22
identityfile ~/.ssh/id_ed25519
identityfile ~/.ssh/id_work
passwordauthentication yes
pubkeyauthentication true
batchmode no
proxyjump bastion
";
        let c = SshResolvedConfig::parse(sample);
        assert_eq!(c.user.as_deref(), Some("alice"));
        assert_eq!(c.hostname.as_deref(), Some("10.0.0.5"));
        assert_eq!(c.port, Some(22));
        assert_eq!(c.identity_files.len(), 2);
        assert_eq!(c.proxy_jump.as_deref(), Some("bastion"));
        assert_eq!(c.password_authentication, Some(true));
        assert_eq!(c.pubkey_authentication, Some(true));
        assert_eq!(c.batchmode, Some(false));
    }

    #[test]
    fn expand_tilde_home() {
        if std::env::var_os("HOME").is_some() {
            let p = expand_tilde("~/.ssh/id_rsa");
            assert!(p.to_string_lossy().contains(".ssh"));
            assert!(!p.to_string_lossy().starts_with('~'));
        }
    }

    /// Live: `ssh -G` against local OpenSSH config (no network connect).
    #[test]
    fn live_ssh_g_localhost() {
        let target = SshTarget::new("localhost");
        let auth = SshAuth::keys_and_config();
        match ssh_resolve_config(&target, &auth) {
            Ok(c) => {
                assert!(c.hostname.is_some() || c.user.is_some());
                // User's config lists IdentityFile ~/.ssh/* — expect some.
                // (may be empty on minimal CI images)
            }
            Err(SshError::SshNotFound) => {
                // CI without OpenSSH — skip
            }
            Err(e) => panic!("unexpected: {e}"),
        }
    }

    #[test]
    fn askpass_script_prints_password() {
        let dir = make_askpass_helper("p@ss'word").unwrap();
        let helper = dir.join("askpass");
        let out = Command::new(&helper).output().unwrap();
        assert!(out.status.success());
        assert_eq!(String::from_utf8_lossy(&out.stdout), "p@ss'word\n");
        let _ = fs::remove_dir_all(dir);
    }
}
