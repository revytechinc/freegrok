//! Product self-diagnostics (`grok doctor` / `grok-build doctor`).
//!
//! Default tier is **unprivileged and offline**: no network, no model calls,
//! no sudo/doas, no FreeBSD jail creation. Opt-in flags (planned) open online,
//! auth, MCP, sandbox-deep, and live inference.
//!
//! FreeBSD note: unprivileged processes cannot create jails (`jail_set` → EPERM).
//! Missing jail helper is a **warning**, not a critical failure.

use serde::Serialize;
use std::time::Instant;

mod checks;

/// Which diagnostic tier was requested.
#[derive(Debug, Clone, Copy, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DoctorOptions {
    /// Machine-readable JSON on stdout.
    pub json: bool,
    /// Critical subset only.
    pub quick: bool,
    /// Warnings count as hard failures for exit code 2.
    pub strict: bool,
    /// Allow network probes (not implemented in D1 beyond skip).
    pub online: bool,
    /// Use credentials / models (not implemented in D1 beyond skip).
    pub auth: bool,
    /// Run MCP connectivity (not implemented in D1 beyond skip).
    pub mcp: bool,
    /// Attempt sandbox helper deep checks (not implemented in D1 beyond skip).
    pub sandbox_deep: bool,
    /// One live inference turn (not implemented in D1 beyond skip).
    pub live: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Severity {
    Critical,
    Warn,
    Info,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Status {
    Pass,
    Warn,
    Fail,
    Skip,
    Info,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckResult {
    pub id: String,
    pub tier: String,
    pub severity: Severity,
    pub status: Status,
    pub summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fix: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub requires: Vec<String>,
    /// Wall time for this check in milliseconds.
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DoctorSummary {
    pub pass: usize,
    pub warn: usize,
    pub fail: usize,
    pub skip: usize,
    pub info: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OsInfo {
    pub family: String,
    pub release: String,
    pub arch: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BinaryInfo {
    pub path: String,
    pub brand: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DoctorReport {
    pub grok_version: String,
    pub channel: String,
    pub os: OsInfo,
    pub binary: BinaryInfo,
    pub checks: Vec<CheckResult>,
    pub summary: DoctorSummary,
    /// Process exit code recommended by this report.
    pub exit_code: i32,
}

impl DoctorSummary {
    fn from_checks(checks: &[CheckResult]) -> Self {
        let mut s = Self {
            pass: 0,
            warn: 0,
            fail: 0,
            skip: 0,
            info: 0,
        };
        for c in checks {
            match c.status {
                Status::Pass => s.pass += 1,
                Status::Warn => s.warn += 1,
                Status::Fail => s.fail += 1,
                Status::Skip => s.skip += 1,
                Status::Info => s.info += 1,
            }
        }
        s
    }
}

fn exit_code_for(summary: &DoctorSummary, strict: bool) -> i32 {
    if summary.fail > 0 {
        return 2;
    }
    if summary.warn > 0 || (strict && summary.warn > 0) {
        return 1;
    }
    0
}

/// Run product diagnostics and print the report. Returns the process exit code.
pub async fn run(opts: DoctorOptions) -> anyhow::Result<i32> {
    let report = collect_report(opts).await?;
    if opts.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_human(&report);
    }
    Ok(report.exit_code)
}

/// Collect the full report without printing (testable).
pub async fn collect_report(opts: DoctorOptions) -> anyhow::Result<DoctorReport> {
    // Pager-bin may pass a richer string via env; otherwise package version.
    let version = std::env::var("GROK_DOCTOR_VERSION_OVERRIDE").unwrap_or_else(|_| {
        format!("{} (shell)", env!("CARGO_PKG_VERSION"))
    });
    let channel = std::env::var("GROK_CHANNEL").unwrap_or_else(|_| "unknown".into());

    let exe = std::env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "(unknown)".into());
    let brand = checks::binary_brand(&exe);

    let mut checks = Vec::new();
    checks.extend(checks::run_default_tier(opts.quick).await);

    // Opt-in tiers not fully implemented yet: explicit skips so flags are honest.
    if opts.online {
        checks.push(skipped(
            "net.online",
            "online",
            "Online probes",
            "Tier not fully implemented yet; use `grok update --check` for now",
            &["network"],
        ));
    }
    if opts.auth {
        checks.push(skipped(
            "auth.auth",
            "auth",
            "Auth / models probes",
            "Tier not fully implemented yet; use `grok models`",
            &["auth"],
        ));
    }
    if opts.mcp {
        checks.push(skipped(
            "mcp.doctor",
            "mcp",
            "MCP doctor integration",
            "Use `grok mcp doctor` until folded into this command",
            &["mcp"],
        ));
    }
    if opts.sandbox_deep {
        checks.push(checks::sandbox_deep_check());
    }
    if opts.live {
        checks.push(skipped(
            "live.single_turn",
            "live",
            "Live inference probe",
            "Tier not fully implemented yet; use `grok -p \"…\" --max-turns 1`",
            &["auth", "network"],
        ));
    }

    let summary = DoctorSummary::from_checks(&checks);
    let exit_code = exit_code_for(&summary, opts.strict);

    Ok(DoctorReport {
        grok_version: version,
        channel,
        os: OsInfo {
            family: std::env::consts::OS.to_string(),
            release: checks::os_release(),
            arch: std::env::consts::ARCH.to_string(),
        },
        binary: BinaryInfo {
            path: exe,
            brand,
        },
        checks,
        summary,
        exit_code,
    })
}

fn skipped(id: &str, tier: &str, summary: &str, detail: &str, requires: &[&str]) -> CheckResult {
    CheckResult {
        id: id.into(),
        tier: tier.into(),
        severity: Severity::Info,
        status: Status::Skip,
        summary: summary.into(),
        detail: Some(detail.into()),
        fix: None,
        requires: requires.iter().map(|s| (*s).to_string()).collect(),
        duration_ms: 0,
    }
}

fn timed<F: FnOnce() -> CheckResult>(f: F) -> CheckResult {
    let start = Instant::now();
    let mut r = f();
    r.duration_ms = start.elapsed().as_millis() as u64;
    r
}

async fn timed_async<F, Fut>(f: F) -> CheckResult
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = CheckResult>,
{
    let start = Instant::now();
    let mut r = f().await;
    r.duration_ms = start.elapsed().as_millis() as u64;
    r
}

fn print_human(report: &DoctorReport) {
    println!();
    println!("Grok Doctor");
    println!("  Version:  {}", report.grok_version);
    println!("  Channel:  {}", report.channel);
    println!(
        "  OS:       {} {} ({})",
        report.os.family, report.os.release, report.os.arch
    );
    println!(
        "  Binary:   {} [{}]",
        report.binary.path, report.binary.brand
    );
    println!();

    for c in &report.checks {
        let mark = match c.status {
            Status::Pass => "✓",
            Status::Warn => "!",
            Status::Fail => "✗",
            Status::Skip => "·",
            Status::Info => "i",
        };
        let sev = match c.severity {
            Severity::Critical => "critical",
            Severity::Warn => "warn",
            Severity::Info => "info",
        };
        println!(
            "  {mark} [{sev}] {} — {}",
            c.id, c.summary
        );
        if let Some(ref d) = c.detail {
            for line in d.lines() {
                println!("      {line}");
            }
        }
        if let Some(ref fix) = c.fix {
            println!("      → {fix}");
        }
    }

    println!();
    println!(
        "Summary: {} pass, {} warn, {} fail, {} skip, {} info",
        report.summary.pass,
        report.summary.warn,
        report.summary.fail,
        report.summary.skip,
        report.summary.info
    );
    if report.exit_code == 0 {
        println!("Result: OK");
    } else if report.exit_code == 1 {
        println!("Result: WARNINGS (exit {})", report.exit_code);
    } else {
        println!("Result: FAILURES (exit {})", report.exit_code);
    }
    println!();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_code_fail_is_2() {
        let s = DoctorSummary {
            pass: 1,
            warn: 0,
            fail: 1,
            skip: 0,
            info: 0,
        };
        assert_eq!(exit_code_for(&s, false), 2);
    }

    #[test]
    fn exit_code_warn_is_1() {
        let s = DoctorSummary {
            pass: 1,
            warn: 1,
            fail: 0,
            skip: 0,
            info: 0,
        };
        assert_eq!(exit_code_for(&s, false), 1);
    }

    #[test]
    fn exit_code_all_pass_is_0() {
        let s = DoctorSummary {
            pass: 3,
            warn: 0,
            fail: 0,
            skip: 1,
            info: 1,
        };
        assert_eq!(exit_code_for(&s, false), 0);
    }

    #[tokio::test]
    async fn collect_report_default_does_not_panic() {
        let report = collect_report(DoctorOptions {
            json: true,
            ..DoctorOptions::default()
        })
        .await
        .expect("doctor report");
        assert!(!report.checks.is_empty());
        assert!(!report.binary.brand.is_empty());
    }

    #[test]
    fn parse_freebsd_major_stable_and_release() {
        assert_eq!(checks::parse_freebsd_major("15.1-STABLE"), Some(15));
        assert_eq!(checks::parse_freebsd_major("14.3-RELEASE"), Some(14));
        assert_eq!(checks::parse_freebsd_major("16.0-CURRENT"), Some(16));
        assert_eq!(checks::parse_freebsd_major("bogus"), None);
        assert_eq!(checks::MIN_FREEBSD_MAJOR, 14);
    }
}
