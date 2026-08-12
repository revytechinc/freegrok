//! grok-jail-helper — privileged FreeBSD jail re-exec front-end (bubblewrap analog).
//!
//! This binary is intentionally **not** setuid in the package. Admins who want
//! apply must wrap it (doas/sudo) themselves. Without root, `--apply` fails
//! closed. Default is `--dry-run`.
//!
//! Usage:
//!   grok-jail-helper [--dry-run|--apply] [--deny-write PATH]... [--deny-read PATH]... -- EXE [ARGS...]

use std::io::{self, Write};
use std::process::ExitCode;

#[derive(Debug, Default)]
struct Plan {
    apply: bool,
    deny_write: Vec<String>,
    deny_read: Vec<String>,
    exe: Option<String>,
    args: Vec<String>,
}

fn parse(argv: &[String]) -> Result<Plan, String> {
    let mut plan = Plan::default();
    let mut i = 0;
    while i < argv.len() {
        match argv[i].as_str() {
            "--dry-run" => {
                plan.apply = false;
                i += 1;
            }
            "--apply" => {
                plan.apply = true;
                i += 1;
            }
            "--deny-write" => {
                let p = argv.get(i + 1).ok_or("--deny-write requires a path")?;
                plan.deny_write.push(p.clone());
                i += 2;
            }
            "--deny-read" => {
                let p = argv.get(i + 1).ok_or("--deny-read requires a path")?;
                plan.deny_read.push(p.clone());
                i += 2;
            }
            "--" => {
                let rest = &argv[i + 1..];
                if rest.is_empty() {
                    return Err("missing EXE after --".into());
                }
                plan.exe = Some(rest[0].clone());
                plan.args = rest[1..].to_vec();
                return Ok(plan);
            }
            "-h" | "--help" => return Err("help".into()),
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    Err("missing -- EXE [ARGS...]".into())
}

fn print_help() {
    let _ = writeln!(
        io::stderr(),
        "grok-jail-helper — FreeBSD jail re-exec helper (not setuid)\n\n\
         Usage:\n  grok-jail-helper [--dry-run|--apply] [--deny-write P]... [--deny-read P]... -- EXE [ARGS...]\n\n\
         Default is --dry-run. --apply requires euid 0 and is refuse-closed until\n\
         a jail root is configured (GROK_JAIL_ROOT). The package never installs\n\
         this binary setuid."
    );
}

fn euid_is_root() -> bool {
    #[cfg(unix)]
    {
        // SAFETY: geteuid has no preconditions.
        unsafe { libc::geteuid() == 0 }
    }
    #[cfg(not(unix))]
    {
        false
    }
}

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let plan = match parse(&argv) {
        Ok(p) => p,
        Err(e) if e == "help" => {
            print_help();
            return ExitCode::SUCCESS;
        }
        Err(e) => {
            let _ = writeln!(io::stderr(), "error: {e}");
            print_help();
            return ExitCode::from(2);
        }
    };

    let exe = plan.exe.as_deref().unwrap_or("");
    println!("grok-jail-helper plan");
    println!("  apply={}", plan.apply);
    println!("  deny_write={:?}", plan.deny_write);
    println!("  deny_read={:?}", plan.deny_read);
    println!("  exe={exe}");
    println!("  args={:?}", plan.args);

    if !plan.apply {
        println!("dry-run: not creating a jail");
        return ExitCode::SUCCESS;
    }

    if !euid_is_root() {
        let _ = writeln!(
            io::stderr(),
            "error: --apply requires root (package is not setuid; use doas/sudo if you intend this)"
        );
        return ExitCode::from(1);
    }

    // Fail closed: do not jail path=/ (host root) — that is not a security boundary.
    let root = std::env::var("GROK_JAIL_ROOT").unwrap_or_default();
    if root.is_empty() || !std::path::Path::new(&root).is_dir() {
        let _ = writeln!(
            io::stderr(),
            "error: --apply refused: set GROK_JAIL_ROOT to a dedicated jail root directory\n\
             (refusing host path=/). Dry-run succeeded; apply is opt-in and fail-closed."
        );
        return ExitCode::from(3);
    }

    let _ = writeln!(
        io::stderr(),
        "error: jail create/exec is not enabled in this helper build (root={root}).\n\
         Refusing to call jail(8) until nullfs deny overlays land. Fail closed."
    );
    ExitCode::from(3)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_dry_run_with_denies() {
        let argv = vec![
            "--dry-run".into(),
            "--deny-write".into(),
            "/tmp/a".into(),
            "--deny-read".into(),
            "/etc/master.passwd".into(),
            "--".into(),
            "/usr/local/bin/grok-build".into(),
            "doctor".into(),
        ];
        let p = parse(&argv).unwrap();
        assert!(!p.apply);
        assert_eq!(p.deny_write, ["/tmp/a"]);
        assert_eq!(p.deny_read, ["/etc/master.passwd"]);
        assert_eq!(p.exe.as_deref(), Some("/usr/local/bin/grok-build"));
        assert_eq!(p.args, ["doctor"]);
    }

    #[test]
    fn parse_rejects_missing_exe() {
        assert!(parse(&["--dry-run".into()]).is_err());
    }
}
