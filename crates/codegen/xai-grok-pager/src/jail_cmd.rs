//! FreeBSD jail helpers for the **TUI product** (agent isolation), not a jail OS manager.
//!
//! Default path: optional privileged **helper** (bubblewrap analog). Full base.txz
//! roots / SSH are advanced (`setup --full`) and still local-only.

use clap::{Args, Subcommand};
use std::process::Command;
use std::time::{Duration, Instant};
use xai_grok_sandbox::jail_catalog::{
    FreeBsdVersion, JailProvisionPlan, JailSetupScope, PRIVILEGE_REASON_ONE_LINE, base_txz_url,
    catalog_index_urls, jail_userland_compatible, parse_directory_index, parse_host_version,
    preferred_jail_base,
};

const FETCH_TIMEOUT: Duration = Duration::from_secs(12);

#[derive(Debug, Args, Clone)]
pub struct JailArgs {
    #[command(subcommand)]
    pub command: JailCommand,
}

#[derive(Debug, Subcommand, Clone)]
pub enum JailCommand {
    /// Host jail backend status (offline, fast)
    Status {
        #[arg(long)]
        json: bool,
    },
    /// List FreeBSD bases from download.freebsd.org (online; optional)
    Catalog {
        #[arg(long)]
        json: bool,
        #[arg(long)]
        arch: Option<String>,
    },
    /// Dry-run setup plan + privilege text (default: helper-only, no download)
    Setup {
        #[arg(long)]
        json: bool,
        /// Advanced: plan a full base.txz root (still local-only; not the TUI default)
        #[arg(long)]
        full: bool,
        /// Base version for --full (default: preferred compatible RELEASE)
        #[arg(long)]
        base: Option<String>,
        /// Not implemented — would escalate after explicit confirm
        #[arg(long)]
        apply: bool,
    },
}

pub async fn run(args: JailArgs) -> anyhow::Result<()> {
    match args.command {
        JailCommand::Status { json } => run_status(json),
        JailCommand::Catalog { json, arch } => run_catalog(json, arch).await,
        JailCommand::Setup {
            json,
            full,
            base,
            apply,
        } => run_setup(json, full, base, apply).await,
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
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "productPath": "helper-only (ephemeral agent isolation)",
                "hostRelease": release,
                "hostParsed": host.as_ref().map(|h| &h.raw),
                "arch": host_arch(),
                "backend": backend,
                "helper": helper,
                "networkDefault": "none",
                "tip": PRIVILEGE_REASON_ONE_LINE,
                "next": "grok jail setup   # dry-run; optional",
            }))?
        );
        return Ok(());
    }

    println!("Jail (agent isolation — not a system manager)");
    println!("  Host:    {release}");
    if let Some(h) = &host {
        println!("  Parsed:  {}.{} ({})", h.major, h.minor, h.branch);
    }
    println!("  Helper:  {helper}");
    println!("  Network: none by default (console/jexec)");
    println!("  Tip:     {PRIVILEGE_REASON_ONE_LINE}");
    println!("  Next:    grok jail setup");
    Ok(())
}

async fn run_catalog(json: bool, arch: Option<String>) -> anyhow::Result<()> {
    let arch = arch.unwrap_or_else(host_arch);
    let release = host_release_string();
    let host = parse_host_version(&release)
        .ok_or_else(|| anyhow::anyhow!("cannot parse host FreeBSD version from {release:?}"))?;

    let mut all = Vec::new();
    for url in catalog_index_urls(&arch) {
        match fetch_text(&url) {
            Ok(html) => all.extend(parse_directory_index(&html)),
            Err(e) => eprintln!("warn: {url}: {e}"),
        }
    }
    all.sort_by(|a, b| a.raw.cmp(&b.raw));
    all.dedup_by(|a, b| a.raw == b.raw);

    let preferred = preferred_jail_base(&host, &all).map(|v| v.raw.clone());
    let mut rows = Vec::new();
    for v in &all {
        rows.push(serde_json::json!({
            "version": v.raw,
            "compatible": jail_userland_compatible(&host, v),
            "baseTxz": base_txz_url(&arch, v),
        }));
    }

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "note": "Catalog is for advanced --full roots; TUI default is helper-only",
                "source": "https://download.freebsd.org/ftp",
                "host": host.raw,
                "preferred": preferred,
                "bases": rows,
            }))?
        );
        return Ok(());
    }

    println!("Bases for host {} (userland ≤ host only)", host.raw);
    println!("  preferred: {}", preferred.as_deref().unwrap_or("(none)"));
    println!("  (TUI default does not install these — helper-only path.)");
    for v in &all {
        let mark = if jail_userland_compatible(&host, v) {
            "✓"
        } else {
            "✗"
        };
        println!("  {mark} {}", v.raw);
    }
    Ok(())
}

async fn run_setup(json: bool, full: bool, base: Option<String>, apply: bool) -> anyhow::Result<()> {
    let arch = host_arch();
    let release = host_release_string();
    let host = parse_host_version(&release)
        .ok_or_else(|| anyhow::anyhow!("cannot parse host FreeBSD version from {release:?}"))?;

    let plan = if full {
        let selected = if let Some(b) = base {
            FreeBsdVersion::parse(&b).ok_or_else(|| anyhow::anyhow!("bad --base {b}"))?
        } else {
            let mut catalog = Vec::new();
            for url in catalog_index_urls(&arch) {
                if let Ok(html) = fetch_text(&url) {
                    catalog.extend(parse_directory_index(&html));
                }
            }
            if let Some(p) = preferred_jail_base(&host, &catalog) {
                p.clone()
            } else {
                let guess = format!("{}.{}-RELEASE", host.major, host.minor);
                FreeBsdVersion::parse(&guess)
                    .ok_or_else(|| anyhow::anyhow!("no base for {}", host.raw))?
            }
        };
        JailProvisionPlan::for_host(host, selected, &arch).map_err(anyhow::Error::msg)?
    } else {
        JailProvisionPlan::helper_only(host)
    };

    let escalators = (
        which("doas").is_some(),
        which("sudo").is_some(),
    );

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "dryRun": !apply,
                "applyImplemented": false,
                "scope": format!("{:?}", plan.scope),
                "host": plan.host.raw,
                "base": plan.selected_base.as_ref().map(|b| &b.raw),
                "baseUrl": plan.base_url,
                "network": format!("{:?}", plan.network),
                "rootPath": plan.root_path,
                "user": plan.create_user,
                "localSsh": plan.enable_local_ssh,
                "privilegeReason": plan.privilege_reason,
                "doas": escalators.0,
                "sudo": escalators.1,
                "workflow": "Optional. Run only if you want OS isolation. doctor --ci does not require this.",
            }))?
        );
        return Ok(());
    }

    println!("Jail setup (dry-run) — optional for FreeGrok TUI");
    println!(
        "  Scope:    {}",
        match plan.scope {
            JailSetupScope::HelperOnly => "helper-only (recommended)",
            JailSetupScope::FullRootfs => "full rootfs (advanced)",
        }
    );
    println!("  Host:     {}", plan.host.raw);
    if let Some(b) = &plan.selected_base {
        println!("  Base:     {b}");
    }
    if let Some(u) = &plan.base_url {
        println!("  URL:      {u}");
    }
    println!("  Network:  {:?} (no public exposure)", plan.network);
    println!("  doas/sudo: {}/{}", escalators.0, escalators.1);
    println!();
    println!("--- why admin? (modal copy) ---");
    for line in plan.privilege_reason.lines() {
        println!("{line}");
    }
    println!("---");
    if apply {
        anyhow::bail!("--apply not implemented yet (helper install + optional base)");
    }
    println!("No changes. Decline anytime; agent still runs.");
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
    if which("fetch").is_some() {
        let mut cmd = Command::new("fetch");
        cmd.args(["-q", "-o", "-", "-T", "10", url]);
        let out = timed_output(cmd, FETCH_TIMEOUT).map_err(|e| e.to_string())?;
        if out.status.success() {
            return Ok(String::from_utf8_lossy(&out.stdout).into_owned());
        }
        return Err(String::from_utf8_lossy(&out.stderr).into_owned());
    }
    if which("curl").is_some() {
        let mut cmd = Command::new("curl");
        cmd.args(["-fsSL", "--max-time", "10", url]);
        let out = timed_output(cmd, FETCH_TIMEOUT).map_err(|e| e.to_string())?;
        if out.status.success() {
            return Ok(String::from_utf8_lossy(&out.stdout).into_owned());
        }
        return Err(String::from_utf8_lossy(&out.stderr).into_owned());
    }
    Err("need fetch(1) or curl".into())
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
                    "timed out",
                ));
            }
            None => std::thread::sleep(Duration::from_millis(20)),
        }
    }
}
