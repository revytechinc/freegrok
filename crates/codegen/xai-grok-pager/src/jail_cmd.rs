//! FreeBSD jail catalog / dry-run setup (`grok jail …`).
//!
//! Real privilege escalation + jail create lands with `grok-jail-helper`.
//! This command is safe offline for `status` / plan; `catalog` needs network.

use clap::{Args, Subcommand};
use std::process::Command;
use std::time::{Duration, Instant};
use xai_grok_sandbox::jail_catalog::{
    FreeBsdVersion, JailProvisionPlan, PRIVILEGE_REASON, base_txz_url, catalog_index_urls,
    jail_userland_compatible, parse_directory_index, parse_host_version, preferred_jail_base,
};

const FETCH_TIMEOUT: Duration = Duration::from_secs(12);

#[derive(Debug, Args, Clone)]
pub struct JailArgs {
    #[command(subcommand)]
    pub command: JailCommand,
}

#[derive(Debug, Subcommand, Clone)]
pub enum JailCommand {
    /// Host jail backend status (offline)
    Status {
        #[arg(long)]
        json: bool,
    },
    /// List FreeBSD base versions from download.freebsd.org (online)
    Catalog {
        #[arg(long)]
        json: bool,
        /// CPU arch folder (default: host amd64/aarch64 mapping)
        #[arg(long)]
        arch: Option<String>,
    },
    /// Print privilege reason + dry-run provision plan (no sudo)
    Setup {
        #[arg(long)]
        json: bool,
        /// Base version raw name (default: preferred compatible RELEASE)
        #[arg(long)]
        base: Option<String>,
        /// Actually escalate (not implemented — prints how)
        #[arg(long)]
        apply: bool,
    },
}

pub async fn run(args: JailArgs) -> anyhow::Result<()> {
    match args.command {
        JailCommand::Status { json } => run_status(json),
        JailCommand::Catalog { json, arch } => run_catalog(json, arch).await,
        JailCommand::Setup { json, base, apply } => run_setup(json, base, apply).await,
    }
}

fn host_release_string() -> String {
    if let Ok(out) = timed_output(Command::new("freebsd-version"), FETCH_TIMEOUT) {
        if out.status.success() {
            return String::from_utf8_lossy(&out.stdout).trim().to_string();
        }
    }
    let mut uname = Command::new("uname");
    uname.arg("-r");
    if let Ok(out) = timed_output(uname, FETCH_TIMEOUT) {
        if out.status.success() {
            return String::from_utf8_lossy(&out.stdout).trim().to_string();
        }
    }
    "unknown".into()
}

fn host_arch() -> String {
    // FreeBSD download paths use amd64/aarch64 (not x86_64).
    match std::env::consts::ARCH {
        "x86_64" => "amd64".into(),
        "aarch64" => "aarch64".into(),
        other => other.into(),
    }
}

fn run_status(json: bool) -> anyhow::Result<()> {
    let release = host_release_string();
    let host = parse_host_version(&release);
    let backend = xai_grok_sandbox::jail_backend_status_string();
    #[cfg(target_os = "freebsd")]
    let helper = xai_grok_sandbox::resolve_jail_helper()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "(none)".into());
    #[cfg(not(target_os = "freebsd"))]
    let helper = "(n/a)".to_string();

    if json {
        let payload = serde_json::json!({
            "hostRelease": release,
            "hostParsed": host.as_ref().map(|h| &h.raw),
            "arch": host_arch(),
            "backend": backend,
            "helper": helper,
            "networkDefault": "none (console/jexec; Linux bwrap also needs no net for FS isolation)",
            "privilegeReason": PRIVILEGE_REASON,
        });
        println!("{}", serde_json::to_string_pretty(&payload)?);
        return Ok(());
    }

    println!("Grok jail status");
    println!("  Host:     {release}");
    if let Some(h) = &host {
        println!("  Parsed:   {} (major={}.{})", h.raw, h.major, h.minor);
    }
    println!("  Arch:     {}", host_arch());
    println!("  Backend:  {backend}");
    println!("  Helper:   {helper}");
    println!("  Network:  none by default (local console / jexec)");
    println!();
    println!("Privilege (shown before any doas/sudo):");
    for line in PRIVILEGE_REASON.lines() {
        println!("  {line}");
    }
    Ok(())
}

async fn run_catalog(json: bool, arch: Option<String>) -> anyhow::Result<()> {
    let arch = arch.unwrap_or_else(host_arch);
    let release = host_release_string();
    let host = parse_host_version(&release).ok_or_else(|| {
        anyhow::anyhow!("cannot parse host FreeBSD version from {release:?}")
    })?;

    let mut all = Vec::new();
    for url in catalog_index_urls(&arch) {
        match fetch_text(&url) {
            Ok(html) => all.extend(parse_directory_index(&html)),
            Err(e) => eprintln!("warn: catalog fetch {url}: {e}"),
        }
    }
    // Dedupe by raw
    all.sort_by(|a, b| a.raw.cmp(&b.raw));
    all.dedup_by(|a, b| a.raw == b.raw);

    let mut rows: Vec<serde_json::Value> = Vec::new();
    for v in &all {
        let ok = jail_userland_compatible(&host, v);
        rows.push(serde_json::json!({
            "version": v.raw,
            "compatible": ok,
            "baseTxz": base_txz_url(&arch, v),
        }));
    }
    let preferred = preferred_jail_base(&host, &all).map(|v| v.raw.clone());

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "source": "https://download.freebsd.org/ftp (ftp.freebsd.org redirects here)",
                "host": host.raw,
                "arch": arch,
                "preferred": preferred,
                "bases": rows,
            }))?
        );
        return Ok(());
    }

    println!("FreeBSD bases (download.freebsd.org) for host {}", host.raw);
    println!("  preferred: {}", preferred.as_deref().unwrap_or("(none)"));
    println!();
    for v in &all {
        let mark = if jail_userland_compatible(&host, v) {
            "✓"
        } else {
            "✗"
        };
        println!("  {mark} {}  {}", v.raw, base_txz_url(&arch, v));
    }
    println!();
    println!("✗ = newer than host — cannot install as jail userland on this kernel");
    Ok(())
}

async fn run_setup(json: bool, base: Option<String>, apply: bool) -> anyhow::Result<()> {
    let arch = host_arch();
    let release = host_release_string();
    let host = parse_host_version(&release).ok_or_else(|| {
        anyhow::anyhow!("cannot parse host FreeBSD version from {release:?}")
    })?;

    let selected = if let Some(b) = base {
        FreeBsdVersion::parse(&b).ok_or_else(|| anyhow::anyhow!("bad --base {b}"))?
    } else {
        // Prefer offline plan with host-matching RELEASE name; catalog if online works.
        let mut catalog = Vec::new();
        for url in catalog_index_urls(&arch) {
            if let Ok(html) = fetch_text(&url) {
                catalog.extend(parse_directory_index(&html));
            }
        }
        if let Some(p) = preferred_jail_base(&host, &catalog) {
            p.clone()
        } else {
            // Fallback guess: same major.minor RELEASE
            let guess = format!("{}.{}-RELEASE", host.major, host.minor);
            FreeBsdVersion::parse(&guess).ok_or_else(|| {
                anyhow::anyhow!("no catalog + cannot invent base for {}", host.raw)
            })?
        }
    };

    let plan = JailProvisionPlan::for_host(host, selected, &arch).map_err(anyhow::Error::msg)?;

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "dryRun": !apply,
                "applyImplemented": false,
                "host": plan.host.raw,
                "base": plan.selected_base.raw,
                "baseUrl": plan.base_url,
                "network": format!("{:?}", plan.network),
                "rootPath": plan.root_path,
                "jailName": plan.jail_name,
                "user": plan.create_user,
                "localSsh": plan.enable_local_ssh,
                "privilegeReason": plan.privilege_reason,
                "escalators": {
                    "doas": which("doas").is_some(),
                    "sudo": which("sudo").is_some(),
                },
            }))?
        );
        return Ok(());
    }

    println!("Grok jail setup (dry-run)");
    println!("  Host:     {}", plan.host.raw);
    println!("  Base:     {} ", plan.selected_base.raw);
    println!("  URL:      {}", plan.base_url);
    println!("  Path:     {}", plan.root_path);
    println!("  Name:     {}", plan.jail_name);
    println!("  User:     {}", plan.create_user);
    println!("  Network:  {:?} (no public IP; agent isolation is console/jexec)", plan.network);
    println!(
        "  Local SSH:{} (optional later; ListenAddress 127.0.0.1 only)",
        plan.enable_local_ssh
    );
    println!(
        "  Escalators: doas={} sudo={}",
        which("doas").is_some(),
        which("sudo").is_some()
    );
    println!();
    println!("=== privilege modal copy ===");
    for line in plan.privilege_reason.lines() {
        println!("{line}");
    }
    println!("=== end ===");
    println!();
    if apply {
        anyhow::bail!(
            "--apply is not implemented yet. Next: TUI modal → doas/sudo → grok-jail-helper create"
        );
    }
    println!("No changes made. Re-run with a future --apply after confirming the modal.");
    Ok(())
}

fn which(bin: &str) -> Option<std::path::PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let c = dir.join(bin);
        if c.is_file() {
            return Some(c);
        }
    }
    None
}

fn fetch_text(url: &str) -> Result<String, String> {
    // FreeBSD fetch(1); fall back to curl. Hard timeout — never stall builds.
    if which("fetch").is_some() {
        let mut cmd = Command::new("fetch");
        cmd.args(["-q", "-o", "-", "-T", "10", url]);
        let out = timed_output(cmd, FETCH_TIMEOUT).map_err(|e| e.to_string())?;
        if out.status.success() {
            return Ok(String::from_utf8_lossy(&out.stdout).into_owned());
        }
        return Err(format!(
            "fetch failed: {}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    if which("curl").is_some() {
        let mut cmd = Command::new("curl");
        cmd.args(["-fsSL", "--max-time", "10", url]);
        let out = timed_output(cmd, FETCH_TIMEOUT).map_err(|e| e.to_string())?;
        if out.status.success() {
            return Ok(String::from_utf8_lossy(&out.stdout).into_owned());
        }
        return Err(format!(
            "curl failed: {}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    Err("need fetch(1) or curl to download catalog".into())
}

fn timed_output(
    mut cmd: Command,
    timeout: Duration,
) -> Result<std::process::Output, std::io::Error> {
    use std::io::Read;
    use std::process::Stdio;
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn()?;
    let start = Instant::now();
    loop {
        match child.try_wait()? {
            Some(status) => {
                let mut stdout = Vec::new();
                let mut stderr = Vec::new();
                if let Some(mut o) = child.stdout.take() {
                    let _ = o.read_to_end(&mut stdout);
                }
                if let Some(mut e) = child.stderr.take() {
                    let _ = e.read_to_end(&mut stderr);
                }
                return Ok(std::process::Output {
                    status,
                    stdout,
                    stderr,
                });
            }
            None if start.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "command timed out",
                ));
            }
            None => std::thread::sleep(Duration::from_millis(20)),
        }
    }
}
