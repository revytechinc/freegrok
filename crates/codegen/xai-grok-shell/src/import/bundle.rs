//! Native Grok config **export** / **import** (portable bundle).
//!
//! Format: a directory (or `.tar` later) with:
//!
//! ```text
//! manifest.json       # version, host, created_at, file list, redaction flags
//! config.toml         # ~/.grok/config.toml
//! credentials.json    # optional (opt-in secrets)
//! auth.json           # optional session auth (opt-in secrets)
//! trusted_folders.toml
//! skills/<name>/…     # user skills (SKILL.md trees)
//! accounts/…          # multi-account index + creds if present
//! hooks/…             # if present
//! project/            # optional project .grok snapshot
//! ```
//!
//! Re-import is deterministic (no LLM): merge or replace with conflict report.

use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub const BUNDLE_FORMAT: &str = "grok-config-export";
pub const BUNDLE_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BundleManifest {
    pub format: String,
    pub version: u32,
    pub created_at_unix: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_host: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_os: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_sha: Option<String>,
    /// Secrets were included in this bundle.
    pub includes_secrets: bool,
    pub files: Vec<BundleFileEntry>,
    #[serde(default)]
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BundleFileEntry {
    /// Path relative to bundle root (posix separators).
    pub path: String,
    pub kind: String,
    pub bytes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256_hex: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct ExportOptions {
    pub include_secrets: bool,
    pub include_skills: bool,
    pub include_auth: bool,
    pub include_credentials: bool,
    pub include_trusted_folders: bool,
    pub include_hooks: bool,
    pub include_accounts: bool,
    /// `mcp_preferences.json` (server enable/disable prefs).
    pub include_mcp_preferences: bool,
    /// User `memory/` (MEMORY.md etc.).
    pub include_memory: bool,
    /// `installed-plugins/` (registry + plugin trees). Large but portable.
    pub include_plugins: bool,
    /// User `agents/` and `rules/` if present.
    pub include_agents_rules: bool,
    /// Optional project directory to snapshot `.grok/`.
    pub project_dir: Option<PathBuf>,
    pub source_host: Option<String>,
    pub source_os: Option<String>,
    pub git_sha: Option<String>,
}

impl ExportOptions {
    pub fn safe_defaults() -> Self {
        Self {
            include_secrets: false,
            include_skills: true,
            include_auth: false,
            include_credentials: false,
            include_trusted_folders: true,
            include_hooks: true,
            include_accounts: true,
            include_mcp_preferences: true,
            include_memory: true,
            include_plugins: true,
            include_agents_rules: true,
            project_dir: None,
            source_host: hostname(),
            source_os: Some(std::env::consts::OS.to_string()),
            git_sha: None,
        }
    }

    pub fn full_with_secrets() -> Self {
        Self {
            include_secrets: true,
            include_skills: true,
            include_auth: true,
            include_credentials: true,
            include_trusted_folders: true,
            include_hooks: true,
            include_accounts: true,
            include_mcp_preferences: true,
            include_memory: true,
            include_plugins: true,
            include_agents_rules: true,
            project_dir: None,
            source_host: hostname(),
            source_os: Some(std::env::consts::OS.to_string()),
            git_sha: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ExportResult {
    pub dest: PathBuf,
    pub manifest: BundleManifest,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportConflict {
    pub path: String,
    pub reason: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportReport {
    pub applied: Vec<String>,
    pub skipped_same: Vec<String>,
    pub conflicts: Vec<ImportConflict>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ImportOptions {
    /// Overwrite existing files that differ.
    pub overwrite: bool,
    /// Only list what would happen.
    pub dry_run: bool,
    /// Import skills trees.
    pub skills: bool,
    /// Import secrets files if present in bundle.
    pub secrets: bool,
    /// Import memory/.
    pub memory: bool,
    /// Import installed-plugins/.
    pub plugins: bool,
    /// Import agents/ and rules/.
    pub agents_rules: bool,
}

impl Default for ImportOptions {
    fn default() -> Self {
        Self {
            overwrite: false,
            dry_run: false,
            skills: true,
            secrets: false,
            memory: true,
            plugins: true,
            agents_rules: true,
        }
    }
}

fn hostname() -> Option<String> {
    std::env::var("HOSTNAME")
        .ok()
        .or_else(|| std::env::var("COMPUTERNAME").ok())
        .or_else(|| {
            std::fs::read_to_string("/etc/hostname")
                .ok()
                .map(|s| s.trim().to_string())
        })
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn simple_hash(bytes: &[u8]) -> String {
    // FNV-1a 64-bit hex — fine for conflict detection; not crypto.
    let mut h: u64 = 0xcbf29ce484222325;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{h:016x}")
}

fn push_entry(files: &mut Vec<BundleFileEntry>, rel: &str, kind: &str, bytes: &[u8]) {
    files.push(BundleFileEntry {
        path: rel.replace('\\', "/"),
        kind: kind.into(),
        bytes: bytes.len() as u64,
        sha256_hex: Some(simple_hash(bytes)),
    });
}

fn write_bytes(dest: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(p) = dest.parent() {
        fs::create_dir_all(p)?;
    }
    fs::write(dest, bytes)
}

/// Export `~/.grok` (and optional project) into `dest_dir`.
pub fn export_config(grok_home: &Path, dest_dir: &Path, opts: &ExportOptions) -> std::io::Result<ExportResult> {
    fs::create_dir_all(dest_dir)?;
    let mut files: Vec<BundleFileEntry> = Vec::new();
    let mut notes = Vec::new();

    // config.toml
    let cfg = grok_home.join("config.toml");
    if cfg.is_file() {
        let bytes = fs::read(&cfg)?;
        write_bytes(&dest_dir.join("config.toml"), &bytes)?;
        push_entry(&mut files, "config.toml", "config", &bytes);
    } else {
        notes.push("no config.toml at export time".into());
    }

    // trusted_folders
    if opts.include_trusted_folders {
        let tf = grok_home.join("trusted_folders.toml");
        if tf.is_file() {
            let bytes = fs::read(&tf)?;
            write_bytes(&dest_dir.join("trusted_folders.toml"), &bytes)?;
            push_entry(&mut files, "trusted_folders.toml", "trusted_folders", &bytes);
        }
    }

    // mcp_preferences.json
    if opts.include_mcp_preferences {
        let mcp = grok_home.join("mcp_preferences.json");
        if mcp.is_file() {
            let bytes = fs::read(&mcp)?;
            write_bytes(&dest_dir.join("mcp_preferences.json"), &bytes)?;
            push_entry(&mut files, "mcp_preferences.json", "mcp_preferences", &bytes);
        }
    }

    // credentials / auth (secrets)
    if opts.include_credentials || opts.include_secrets {
        let cred = grok_home.join("credentials.json");
        if cred.is_file() {
            if opts.include_secrets || opts.include_credentials {
                let bytes = fs::read(&cred)?;
                write_bytes(&dest_dir.join("credentials.json"), &bytes)?;
                push_entry(&mut files, "credentials.json", "credentials", &bytes);
            }
        }
    }
    if opts.include_auth || opts.include_secrets {
        let auth = grok_home.join("auth.json");
        if auth.is_file() {
            if opts.include_secrets || opts.include_auth {
                let bytes = fs::read(&auth)?;
                write_bytes(&dest_dir.join("auth.json"), &bytes)?;
                push_entry(&mut files, "auth.json", "auth", &bytes);
            } else {
                notes.push("auth.json omitted (secrets not included)".into());
            }
        }
    } else {
        notes.push("secrets omitted (use --include-secrets to export auth/credentials)".into());
    }

    // accounts
    if opts.include_accounts {
        let acc = grok_home.join("accounts");
        if acc.is_dir() {
            copy_dir_filtered(
                &acc,
                &dest_dir.join("accounts"),
                "accounts",
                "accounts",
                &mut files,
            )?;
        }
    }

    // hooks
    if opts.include_hooks {
        let hooks = grok_home.join("hooks");
        if hooks.is_dir() {
            copy_dir_filtered(&hooks, &dest_dir.join("hooks"), "hooks", "hooks", &mut files)?;
        }
    }

    // skills (user trees)
    if opts.include_skills {
        let skills = grok_home.join("skills");
        if skills.is_dir() {
            copy_dir_filtered(
                &skills,
                &dest_dir.join("skills"),
                "skills",
                "skills",
                &mut files,
            )?;
        }
    }

    // memory
    if opts.include_memory {
        let mem = grok_home.join("memory");
        if mem.is_dir() {
            copy_dir_filtered(&mem, &dest_dir.join("memory"), "memory", "memory", &mut files)?;
        }
    }

    // agents + rules
    if opts.include_agents_rules {
        for name in ["agents", "rules"] {
            let dir = grok_home.join(name);
            if dir.is_dir() {
                copy_dir_filtered(&dir, &dest_dir.join(name), name, name, &mut files)?;
            }
        }
    }

    // installed-plugins (registry + trees)
    if opts.include_plugins {
        let plugins = grok_home.join("installed-plugins");
        if plugins.is_dir() {
            copy_dir_filtered(
                &plugins,
                &dest_dir.join("installed-plugins"),
                "installed-plugins",
                "plugins",
                &mut files,
            )?;
        }
    }

    // project snapshot
    if let Some(proj) = &opts.project_dir {
        let pg = proj.join(".grok");
        if pg.is_dir() {
            copy_dir_filtered(
                &pg,
                &dest_dir.join("project/.grok"),
                "project/.grok",
                "project",
                &mut files,
            )?;
        }
    }

    // Warn when config.toml may embed secrets (MCP headers, tokens).
    if !opts.include_secrets {
        notes.push(
            "config.toml may still contain MCP tokens/headers — review before sharing the bundle"
                .into(),
        );
    }

    let includes_secrets = opts.include_secrets
        || (opts.include_auth && files.iter().any(|f| f.kind == "auth"))
        || (opts.include_credentials && files.iter().any(|f| f.kind == "credentials"));

    let manifest = BundleManifest {
        format: BUNDLE_FORMAT.into(),
        version: BUNDLE_VERSION,
        created_at_unix: now_unix(),
        source_host: opts.source_host.clone(),
        source_os: opts.source_os.clone(),
        git_sha: opts.git_sha.clone(),
        includes_secrets,
        files,
        notes,
    };

    let man_bytes = serde_json::to_vec_pretty(&manifest)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    write_bytes(&dest_dir.join("manifest.json"), &man_bytes)?;

    // also write a short README for humans
    let readme = format!(
        "# Grok config export\n\nformat={BUNDLE_FORMAT} version={BUNDLE_VERSION}\n\
         includes_secrets={includes_secrets}\n\
         Restore: `grok config import {}`\n",
        dest_dir.display()
    );
    write_bytes(&dest_dir.join("README.md"), readme.as_bytes())?;

    Ok(ExportResult {
        dest: dest_dir.to_path_buf(),
        manifest,
    })
}

/// Copy a directory tree into the bundle, recording manifest paths relative to
/// the bundle root (`rel_prefix`/`name`/…).
fn copy_dir_filtered(
    src: &Path,
    dest: &Path,
    rel_prefix: &str,
    kind: &str,
    files: &mut Vec<BundleFileEntry>,
) -> std::io::Result<()> {
    if !src.is_dir() {
        return Ok(());
    }
    fs::create_dir_all(dest)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let ft = entry.file_type()?;
        let name = entry.file_name();
        let n = name.to_string_lossy();
        // Skip locks, caches, and hidden entries (allow only `.grok` for project).
        if n.ends_with(".lock") || n == "logs" || n == "cache" {
            continue;
        }
        if n.starts_with('.') && n != ".grok" {
            continue;
        }
        let from = entry.path();
        let to = dest.join(&name);
        let child_rel = if rel_prefix.is_empty() {
            n.to_string()
        } else {
            format!("{rel_prefix}/{n}")
        };
        if ft.is_dir() {
            copy_dir_filtered(&from, &to, &child_rel, kind, files)?;
        } else if ft.is_file() {
            let bytes = fs::read(&from)?;
            write_bytes(&to, &bytes)?;
            push_entry(files, &child_rel, kind, &bytes);
        }
    }
    Ok(())
}

/// Load and validate a bundle directory.
pub fn read_manifest(bundle_dir: &Path) -> std::io::Result<BundleManifest> {
    let p = bundle_dir.join("manifest.json");
    let bytes = fs::read(&p)?;
    let man: BundleManifest = serde_json::from_slice(&bytes)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    if man.format != BUNDLE_FORMAT {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("unknown format {}", man.format),
        ));
    }
    if man.version > BUNDLE_VERSION {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "bundle version {} newer than supported {BUNDLE_VERSION}",
                man.version
            ),
        ));
    }
    Ok(man)
}

/// Import bundle into `grok_home`.
pub fn import_config(
    bundle_dir: &Path,
    grok_home: &Path,
    opts: &ImportOptions,
) -> std::io::Result<ImportReport> {
    let man = read_manifest(bundle_dir)?;
    let mut report = ImportReport::default();
    if man.includes_secrets && !opts.secrets {
        report.notes.push(
            "bundle includes secrets but --secrets not set; skipping auth/credentials".into(),
        );
    }

    // config.toml
    apply_file(
        &bundle_dir.join("config.toml"),
        &grok_home.join("config.toml"),
        "config.toml",
        opts,
        &mut report,
        true,
    )?;

    apply_file(
        &bundle_dir.join("trusted_folders.toml"),
        &grok_home.join("trusted_folders.toml"),
        "trusted_folders.toml",
        opts,
        &mut report,
        true,
    )?;

    apply_file(
        &bundle_dir.join("mcp_preferences.json"),
        &grok_home.join("mcp_preferences.json"),
        "mcp_preferences.json",
        opts,
        &mut report,
        true,
    )?;

    if opts.secrets {
        apply_file(
            &bundle_dir.join("credentials.json"),
            &grok_home.join("credentials.json"),
            "credentials.json",
            opts,
            &mut report,
            true,
        )?;
        apply_file(
            &bundle_dir.join("auth.json"),
            &grok_home.join("auth.json"),
            "auth.json",
            opts,
            &mut report,
            true,
        )?;
    }

    if bundle_dir.join("accounts").is_dir() {
        apply_tree(
            &bundle_dir.join("accounts"),
            &grok_home.join("accounts"),
            "accounts",
            opts,
            &mut report,
        )?;
    }
    if bundle_dir.join("hooks").is_dir() {
        apply_tree(
            &bundle_dir.join("hooks"),
            &grok_home.join("hooks"),
            "hooks",
            opts,
            &mut report,
        )?;
    }
    if opts.skills && bundle_dir.join("skills").is_dir() {
        apply_tree(
            &bundle_dir.join("skills"),
            &grok_home.join("skills"),
            "skills",
            opts,
            &mut report,
        )?;
    }
    if opts.memory && bundle_dir.join("memory").is_dir() {
        apply_tree(
            &bundle_dir.join("memory"),
            &grok_home.join("memory"),
            "memory",
            opts,
            &mut report,
        )?;
    }
    if opts.agents_rules {
        for name in ["agents", "rules"] {
            if bundle_dir.join(name).is_dir() {
                apply_tree(
                    &bundle_dir.join(name),
                    &grok_home.join(name),
                    name,
                    opts,
                    &mut report,
                )?;
            }
        }
    }
    if opts.plugins && bundle_dir.join("installed-plugins").is_dir() {
        apply_tree(
            &bundle_dir.join("installed-plugins"),
            &grok_home.join("installed-plugins"),
            "installed-plugins",
            opts,
            &mut report,
        )?;
    }

    if bundle_dir.join("project/.grok").is_dir() {
        report.notes.push(
            "project/.grok snapshot present — apply manually to a project cwd if desired".into(),
        );
    }

    Ok(report)
}

fn apply_file(
    src: &Path,
    dest: &Path,
    label: &str,
    opts: &ImportOptions,
    report: &mut ImportReport,
    required_if_missing_src_ok: bool,
) -> std::io::Result<()> {
    if !src.is_file() {
        if !required_if_missing_src_ok {
            report.notes.push(format!("missing in bundle: {label}"));
        }
        return Ok(());
    }
    let new_bytes = fs::read(src)?;
    if dest.is_file() {
        let old = fs::read(dest)?;
        if old == new_bytes {
            report.skipped_same.push(label.into());
            return Ok(());
        }
        if !opts.overwrite {
            report.conflicts.push(ImportConflict {
                path: label.into(),
                reason: "exists and differs (pass overwrite to replace)".into(),
            });
            return Ok(());
        }
    }
    if opts.dry_run {
        report.applied.push(format!("dry-run:{label}"));
        return Ok(());
    }
    if let Some(p) = dest.parent() {
        fs::create_dir_all(p)?;
    }
    // atomic-ish write
    let tmp = dest.with_extension("import-tmp");
    fs::write(&tmp, &new_bytes)?;
    fs::rename(&tmp, dest)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if label.contains("auth") || label.contains("credentials") {
            let _ = fs::set_permissions(dest, fs::Permissions::from_mode(0o600));
        }
    }
    report.applied.push(label.into());
    Ok(())
}

fn apply_tree(
    src: &Path,
    dest_root: &Path,
    label: &str,
    opts: &ImportOptions,
    report: &mut ImportReport,
) -> std::io::Result<()> {
    if !src.is_dir() {
        return Ok(());
    }
    for entry in walk_files(src)? {
        let rel = entry.strip_prefix(src).unwrap_or(&entry);
        let dest = dest_root.join(rel);
        let lab = format!("{label}/{}", rel.display());
        apply_file(&entry, &dest, &lab, opts, report, true)?;
    }
    Ok(())
}

fn walk_files(dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    fn rec(dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
        for e in fs::read_dir(dir)? {
            let e = e?;
            let p = e.path();
            if e.file_type()?.is_dir() {
                rec(&p, out)?;
            } else if e.file_type()?.is_file() {
                out.push(p);
            }
        }
        Ok(())
    }
    rec(dir, &mut out)?;
    Ok(out)
}

impl ImportReport {
    pub fn summary_text(&self) -> String {
        let mut lines = vec![
            format!("applied:       {}", self.applied.len()),
            format!("skipped same:  {}", self.skipped_same.len()),
            format!("conflicts:     {}", self.conflicts.len()),
        ];
        for a in self.applied.iter().take(20) {
            lines.push(format!("  + {a}"));
        }
        for c in &self.conflicts {
            lines.push(format!("  ! {} — {}", c.path, c.reason));
        }
        for n in &self.notes {
            lines.push(format!("note: {n}"));
        }
        lines.join("\n")
    }
}

/// Human-readable export summary.
pub fn export_summary(res: &ExportResult) -> String {
    format!(
        "Exported Grok config bundle → {}\nformat={} v{} files={} secrets={}\n{}",
        res.dest.display(),
        res.manifest.format,
        res.manifest.version,
        res.manifest.files.len(),
        res.manifest.includes_secrets,
        res.manifest
            .notes
            .iter()
            .map(|n| format!("note: {n}"))
            .collect::<Vec<_>>()
            .join("\n")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_import_roundtrip_config_toml() {
        let root = std::env::temp_dir().join(format!("grok-bundle-test-{}", std::process::id()));
        let home = root.join("home/.grok");
        let bundle = root.join("bundle");
        let restore = root.join("restore/.grok");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&home).unwrap();
        fs::write(home.join("config.toml"), b"[models]\ndefault = \"grok-4.5\"\n").unwrap();
        fs::create_dir_all(home.join("skills/my-skill")).unwrap();
        fs::write(
            home.join("skills/my-skill/SKILL.md"),
            b"---\nname: my-skill\n---\n# Hi\n",
        )
        .unwrap();

        let exp = export_config(&home, &bundle, &ExportOptions::safe_defaults()).unwrap();
        assert!(bundle.join("manifest.json").is_file());
        assert!(bundle.join("config.toml").is_file());
        assert_eq!(exp.manifest.version, BUNDLE_VERSION);

        fs::create_dir_all(&restore).unwrap();
        let rep = import_config(
            &bundle,
            &restore,
            &ImportOptions {
                overwrite: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(rep.conflicts.is_empty(), "{:?}", rep.conflicts);
        assert!(restore.join("config.toml").is_file());
        let cfg = fs::read_to_string(restore.join("config.toml")).unwrap();
        assert!(cfg.contains("grok-4.5"));
        assert!(restore.join("skills/my-skill/SKILL.md").is_file());

        // second import without overwrite → same
        let rep2 = import_config(&bundle, &restore, &ImportOptions::default()).unwrap();
        assert!(rep2.skipped_same.iter().any(|s| s == "config.toml"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn conflict_without_overwrite() {
        let root = std::env::temp_dir().join(format!("grok-bundle-c-{}", std::process::id()));
        let home = root.join("home/.grok");
        let bundle = root.join("bundle");
        let restore = root.join("restore/.grok");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&home).unwrap();
        fs::write(home.join("config.toml"), b"a = 1\n").unwrap();
        export_config(&home, &bundle, &ExportOptions::safe_defaults()).unwrap();
        fs::create_dir_all(&restore).unwrap();
        fs::write(restore.join("config.toml"), b"a = 2\n").unwrap();
        let rep = import_config(&bundle, &restore, &ImportOptions::default()).unwrap();
        assert_eq!(rep.conflicts.len(), 1);
        let _ = fs::remove_dir_all(&root);
    }
}
