//! CLI: `grok config export|import|ssh-*` — portable config + remote SSH probes.

use clap::{Parser, Subcommand};
use std::io::{self, Write};
use std::path::PathBuf;
use xai_grok_shell::import::{
    auth_diagnostics, export_config, export_summary, import_config, ExportOptions, ImportOptions,
    SshAuth, SshSession,
};
use xai_grok_shell::util::grok_home::grok_home;

#[derive(Debug, Clone, Parser)]
pub struct ConfigArgs {
    #[command(subcommand)]
    pub command: ConfigCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum ConfigCommand {
    /// Export the entire Grok home config to a directory bundle.
    Export {
        /// Destination directory (created). Default: ./grok-config-export
        #[arg(default_value = "grok-config-export")]
        dest: PathBuf,
        /// Include auth.json / credentials.json secrets.
        #[arg(long)]
        include_secrets: bool,
        /// Skip user skills trees.
        #[arg(long)]
        no_skills: bool,
        /// Skip installed-plugins (smaller bundle; reinstall from marketplace later).
        #[arg(long)]
        no_plugins: bool,
        /// Skip user memory/.
        #[arg(long)]
        no_memory: bool,
        /// Also snapshot project `.grok/` from this directory.
        #[arg(long)]
        project: Option<PathBuf>,
    },
    /// Import a previously exported bundle into ~/.grok.
    Import {
        /// Bundle directory (must contain manifest.json).
        bundle: PathBuf,
        /// Overwrite files that already exist and differ.
        #[arg(long)]
        overwrite: bool,
        /// Show actions without writing.
        #[arg(long)]
        dry_run: bool,
        /// Import secrets from the bundle (auth/credentials).
        #[arg(long)]
        secrets: bool,
        /// Skip skills trees.
        #[arg(long)]
        no_skills: bool,
        /// Skip installed-plugins.
        #[arg(long)]
        no_plugins: bool,
        /// Skip memory/.
        #[arg(long)]
        no_memory: bool,
    },
    /// Show how OpenSSH would authenticate to a host (config, keys, agent).
    ///
    /// Uses the system `ssh` binary so `~/.ssh/config` Host aliases,
    /// IdentityFile, ProxyJump, and ssh-agent are honored. Does not open a
    /// full remote shell unless `--ping` is set.
    SshProbe {
        /// Host alias or `user@host` (same as `ssh <destination>`).
        destination: String,
        /// Explicit user (`ssh -l`), otherwise from config / default.
        #[arg(short = 'l', long)]
        user: Option<String>,
        /// Explicit port (`ssh -p`).
        #[arg(short = 'p', long)]
        port: Option<u16>,
        /// Extra identity file (`ssh -i`), in addition to config / agent.
        #[arg(short = 'i', long)]
        identity: Vec<PathBuf>,
        /// Alternate SSH config (`ssh -F`). Default: `~/.ssh/config`.
        #[arg(short = 'F', long)]
        config: Option<PathBuf>,
        /// Password for this host. Prefer keys when possible; password is
        /// tried after publickey via SSH_ASKPASS (never logged).
        #[arg(long)]
        password: Option<String>,
        /// Prompt on stdin for a password (hidden if possible).
        #[arg(long)]
        ask_password: bool,
        /// Prefer password before publickey (unusual; for hosts that reject key attempts).
        #[arg(long)]
        prefer_password: bool,
        /// Actually connect and run `echo grok-ssh-ok` (tests full auth).
        #[arg(long)]
        ping: bool,
    },
}

pub async fn run(args: ConfigArgs) -> anyhow::Result<()> {
    let home = grok_home();
    match args.command {
        ConfigCommand::Export {
            dest,
            include_secrets,
            no_skills,
            no_plugins,
            no_memory,
            project,
        } => {
            let mut opts = if include_secrets {
                ExportOptions::full_with_secrets()
            } else {
                ExportOptions::safe_defaults()
            };
            opts.include_skills = !no_skills;
            opts.include_plugins = !no_plugins;
            opts.include_memory = !no_memory;
            opts.project_dir = project;
            opts.git_sha = std::process::Command::new("git")
                .args(["rev-parse", "--short", "HEAD"])
                .output()
                .ok()
                .filter(|o| o.status.success())
                .and_then(|o| String::from_utf8(o.stdout).ok())
                .map(|s| s.trim().to_string());

            let res = export_config(&home, &dest, &opts)
                .map_err(|e| anyhow::anyhow!("export failed: {e}"))?;
            println!("{}", export_summary(&res));
            Ok(())
        }
        ConfigCommand::Import {
            bundle,
            overwrite,
            dry_run,
            secrets,
            no_skills,
            no_plugins,
            no_memory,
        } => {
            let opts = ImportOptions {
                overwrite,
                dry_run,
                skills: !no_skills,
                secrets,
                memory: !no_memory,
                plugins: !no_plugins,
                agents_rules: true,
            };
            let rep = import_config(&bundle, &home, &opts)
                .map_err(|e| anyhow::anyhow!("import failed: {e}"))?;
            println!("{}", rep.summary_text());
            if !rep.conflicts.is_empty() && !overwrite {
                anyhow::bail!(
                    "{} conflict(s); re-run with --overwrite to replace",
                    rep.conflicts.len()
                );
            }
            Ok(())
        }
        ConfigCommand::SshProbe {
            destination,
            user,
            port,
            identity,
            config,
            password,
            ask_password,
            prefer_password,
            ping,
        } => {
            let mut password = password;
            if ask_password && password.is_none() {
                eprint!("SSH password for {destination}: ");
                let _ = io::stderr().flush();
                // stdin (visible on FreeBSD without rpassword dep — acceptable for CLI probe)
                let mut line = String::new();
                io::stdin().read_line(&mut line)?;
                let p = line.trim_end_matches(['\r', '\n']).to_string();
                if !p.is_empty() {
                    password = Some(p);
                }
            }

            let mut session = SshSession::new(destination);
            if let Some(u) = user {
                session = session.user(u);
            }
            if let Some(p) = port {
                session = session.port(p);
            }
            if let Some(cfg) = config {
                session = session.config_file(cfg);
            }
            for id in identity {
                session = session.identity(id);
            }
            if let Some(pw) = password {
                session = session.password(pw);
            }
            if prefer_password {
                session.auth.prefer_password = true;
            }

            println!("{}", auth_diagnostics(&session.target, &session.auth));
            if ping {
                println!("— connecting —");
                session
                    .ping()
                    .map_err(|e| anyhow::anyhow!("ssh ping failed: {e}"))?;
                let uname = session.uname().unwrap_or_else(|_| "?".into());
                let rhome = session.home().unwrap_or_else(|_| "?".into());
                println!("ok uname={uname} home={rhome}");
            } else {
                println!("(pass --ping to open a connection and verify auth)");
            }
            // Scrub password from process memory best-effort
            session.auth = SshAuth::keys_and_config();
            Ok(())
        }
    }
}
