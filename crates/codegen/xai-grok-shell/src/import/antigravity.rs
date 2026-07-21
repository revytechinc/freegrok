//! Scan Google Antigravity CLI (`agy`) / Gemini home for importable config.
//!
//! Layout (documented + observed on FreeBSD):
//!
//! ```text
//! ~/.gemini/
//!   config/
//!     mcp_config.json          # MCP servers (stdio or serverUrl)
//!     config.json              # light user settings
//!     skills/                  # optional user skills
//!   antigravity-cli/
//!     settings.json            # permissions, trusted workspaces, flags
//!     antigravity-oauth-token  # Google OAuth (NOT a GEMINI_API_KEY)
//!     builtin/skills/<name>/   # shipped skills
//! ```
//!
//! **Gemini models into Grok:** AGY auth is typically Google consumer OAuth.
//! Grok's Google provider expects an API key (`GEMINI_API_KEY` /
//! `GOOGLE_API_KEY`) against the OpenAI-compat Gemini endpoint. We surface
//! OAuth presence as an opt-in *re-auth* finding and only treat real API keys
//! (env or files) as provider credentials.

use super::dedup::{
    equivalence_account, equivalence_env, equivalence_mcp_stdio, equivalence_mcp_url,
    equivalence_skill,
};
use super::findings::{
    FindingKind, LocalDiff, RemoteFinding, SecretPolicy, SourceProduct,
};
use super::paths::{
    antigravity_cli_dir, antigravity_mcp_config_path, antigravity_oauth_token_path,
    antigravity_settings_path, gemini_config_json_path, gemini_home, gemini_skills_roots,
};
use serde_json::Value;
use std::path::{Path, PathBuf};

/// OpenAI-compat base for Google AI Studio / Gemini API keys.
pub const GEMINI_OPENAI_COMPAT_BASE: &str =
    "https://generativelanguage.googleapis.com/v1beta/openai";


#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgyAuthKind {
    /// No oauth file and no API key found.
    None,
    /// Google OAuth token file present (Antigravity login).
    GoogleOauth {
        auth_method: Option<String>,
        has_refresh: bool,
    },
    /// API key available via env or file (usable by Grok Google provider).
    ApiKey { source: String },
}

#[derive(Debug, Clone)]
pub struct AgyMcpServer {
    pub name: String,
    pub command: Option<String>,
    pub args: Vec<String>,
    pub url: Option<String>,
}

#[derive(Debug, Clone)]
pub struct AgySkill {
    pub name: String,
    pub path: PathBuf,
    pub builtin: bool,
}

#[derive(Debug, Clone, Default)]
pub struct AntigravityImport {
    pub gemini_home: Option<PathBuf>,
    pub cli_dir: Option<PathBuf>,
    pub settings_path: Option<PathBuf>,
    pub mcp_path: Option<PathBuf>,
    pub oauth_path: Option<PathBuf>,
    pub auth: AgyAuthKind,
    pub mcp_servers: Vec<AgyMcpServer>,
    pub skills: Vec<AgySkill>,
    pub model_hints: Vec<String>,
    pub env_api_key_present: bool,
    pub settings_summary: Option<String>,
    pub notes: Vec<String>,
    /// Structured findings for dedup UI.
    pub findings: Vec<RemoteFinding>,
}

impl Default for AgyAuthKind {
    fn default() -> Self {
        Self::None
    }
}

/// Scan default user home for Antigravity / Gemini config.
pub fn scan_antigravity() -> AntigravityImport {
    scan_antigravity_in(gemini_home().as_deref(), None)
}

/// Scan under an explicit `~/.gemini`-equivalent root (tests + remote copies).
///
/// `env_keys`: optional map of env var name → value for API key detection
/// (pass `None` to read process env).
pub fn scan_antigravity_in(
    gemini_root: Option<&Path>,
    env_keys: Option<&[(String, String)]>,
) -> AntigravityImport {
    let mut out = AntigravityImport::default();

    let Some(root) = gemini_root else {
        out.notes
            .push("No HOME / Gemini root — cannot scan Antigravity".into());
        return out;
    };
    out.gemini_home = Some(root.to_path_buf());

    if !root.is_dir() {
        out.notes
            .push(format!("Gemini home missing: {}", root.display()));
        return out;
    }

    let cli = root.join("antigravity-cli");
    if cli.is_dir() {
        out.cli_dir = Some(cli.clone());
    }

    // settings.json
    let settings = root.join("antigravity-cli/settings.json");
    if settings.is_file() {
        out.settings_path = Some(settings.clone());
        match std::fs::read_to_string(&settings) {
            Ok(raw) => match serde_json::from_str::<Value>(&raw) {
                Ok(v) => {
                    out.settings_summary = Some(summarize_settings(&v));
                    out.findings.push(path_finding(
                        "agy.settings",
                        FindingKind::Permission,
                        &settings,
                        "Antigravity settings",
                        out.settings_summary.as_deref().unwrap_or("settings.json"),
                        content_hash(raw.as_bytes()),
                        format!("agy:settings:{}", content_hash(raw.as_bytes())),
                        SecretPolicy::Never,
                    ));
                }
                Err(e) => out
                    .notes
                    .push(format!("settings.json parse error: {e}")),
            },
            Err(e) => out.notes.push(format!("could not read settings: {e}")),
        }
    }

    // MCP
    let mcp = root.join("config/mcp_config.json");
    if mcp.is_file() {
        out.mcp_path = Some(mcp.clone());
        match std::fs::read_to_string(&mcp) {
            Ok(raw) => {
                if raw.trim().is_empty() {
                    out.notes.push("mcp_config.json is empty".into());
                } else {
                    match serde_json::from_str::<Value>(&raw) {
                        Ok(v) => {
                            out.mcp_servers = parse_mcp_servers(&v);
                            for s in &out.mcp_servers {
                                out.findings.push(mcp_finding(s, &mcp));
                            }
                        }
                        Err(e) => out.notes.push(format!("mcp_config.json parse error: {e}")),
                    }
                }
            }
            Err(e) => out.notes.push(format!("could not read mcp_config: {e}")),
        }
    }

    // Skills (user + builtin under this root)
    for (skills_root, builtin) in [
        (root.join("config/skills"), false),
        (root.join("antigravity-cli/builtin/skills"), true),
    ] {
        if skills_root.is_dir() {
            collect_skills(&skills_root, builtin, &mut out.skills);
        }
    }
    for sk in &out.skills {
        out.findings.push(RemoteFinding {
            id: format!("agy.skill.{}", sk.name),
            kind: FindingKind::Skill,
            source_tool: SourceProduct::Antigravity,
            remote_path: Some(sk.path.clone()),
            display_name: sk.name.clone(),
            summary: if sk.builtin {
                format!("builtin skill {}", sk.name)
            } else {
                format!("user skill {}", sk.name)
            },
            content_hash: content_hash(sk.name.as_bytes()),
            equivalence_key: equivalence_skill(&sk.name),
            exists_locally: false,
            local_diff: LocalDiff::Missing,
            account_id: None,
            secret_policy: SecretPolicy::Never,
            seen_in_products: vec![SourceProduct::Antigravity],
            canonical_source: SourceProduct::Antigravity,
            duplicate_of: None,
            payload: Some(serde_json::json!({
                "name": sk.name,
                "builtin": sk.builtin,
            })),
        });
    }

    // OAuth token (structure only — never copy secret into findings payload)
    let oauth = root.join("antigravity-cli/antigravity-oauth-token");
    let mut oauth_kind = AgyAuthKind::None;
    if oauth.is_file() {
        out.oauth_path = Some(oauth.clone());
        match std::fs::read_to_string(&oauth) {
            Ok(raw) => match serde_json::from_str::<Value>(&raw) {
                Ok(v) => {
                    let auth_method = v
                        .get("auth_method")
                        .and_then(|x| x.as_str())
                        .map(str::to_string);
                    let has_refresh = v
                        .pointer("/token/refresh_token")
                        .and_then(|x| x.as_str())
                        .map(|s| !s.is_empty())
                        .unwrap_or(false);
                    let has_access = v
                        .pointer("/token/access_token")
                        .and_then(|x| x.as_str())
                        .map(|s| !s.is_empty())
                        .unwrap_or(false);
                    if has_access || has_refresh {
                        oauth_kind = AgyAuthKind::GoogleOauth {
                            auth_method: auth_method.clone(),
                            has_refresh,
                        };
                        out.findings.push(RemoteFinding {
                            id: "agy.oauth".into(),
                            kind: FindingKind::Account,
                            source_tool: SourceProduct::Antigravity,
                            remote_path: Some(oauth.clone()),
                            display_name: "Antigravity Google OAuth".into(),
                            summary: format!(
                                "Google OAuth present (method={}); not a Gemini API key — re-auth or set GEMINI_API_KEY for Grok",
                                auth_method.as_deref().unwrap_or("unknown")
                            ),
                            content_hash: content_hash(b"agy-oauth-present"),
                            equivalence_key: equivalence_account("google", "agy-oauth"),
                            exists_locally: false,
                            local_diff: LocalDiff::Missing,
                            account_id: Some("google:agy-oauth".into()),
                            secret_policy: SecretPolicy::Never, // do not auto-apply OAuth
                            seen_in_products: vec![SourceProduct::Antigravity],
                            canonical_source: SourceProduct::Antigravity,
                            duplicate_of: None,
                            payload: Some(serde_json::json!({
                                "auth_method": auth_method,
                                "has_refresh": has_refresh,
                                "usable_as_api_key": false,
                            })),
                        });
                        out.notes.push(
                            "OAuth login found; Grok Google provider needs GEMINI_API_KEY/GOOGLE_API_KEY (or native OAuth support later)"
                                .into(),
                        );
                    }
                }
                Err(e) => out.notes.push(format!("oauth token parse error: {e}")),
            },
            Err(e) => out.notes.push(format!("could not read oauth token: {e}")),
        }
    }

    // API keys from env
    let key_from_env = detect_api_key(env_keys);
    out.env_api_key_present = key_from_env.is_some();
    if let Some((var, _val)) = &key_from_env {
        out.auth = AgyAuthKind::ApiKey {
            source: format!("env:{var}"),
        };
        out.findings.push(RemoteFinding {
            id: format!("agy.env.{var}"),
            kind: FindingKind::EnvVar,
            source_tool: SourceProduct::Antigravity,
            remote_path: None,
            display_name: var.clone(),
            summary: format!(
                "{var} set — usable as Grok Google provider key @ {GEMINI_OPENAI_COMPAT_BASE}"
            ),
            content_hash: content_hash(var.as_bytes()),
            equivalence_key: equivalence_env(var),
            exists_locally: true,
            local_diff: LocalDiff::Same,
            account_id: Some(format!("google:env:{var}")),
            secret_policy: SecretPolicy::OptIn,
            seen_in_products: vec![SourceProduct::Antigravity],
            canonical_source: SourceProduct::Antigravity,
            duplicate_of: None,
            payload: Some(serde_json::json!({
                "provider": "google",
                "base_url": GEMINI_OPENAI_COMPAT_BASE,
                "env_var": var,
            })),
        });
        out.findings.push(RemoteFinding {
            id: "agy.provider.google".into(),
            kind: FindingKind::ModelProvider,
            source_tool: SourceProduct::Antigravity,
            remote_path: None,
            display_name: "Google Gemini (API key)".into(),
            summary: format!("OpenAI-compat Gemini @ {GEMINI_OPENAI_COMPAT_BASE}"),
            content_hash: content_hash(GEMINI_OPENAI_COMPAT_BASE.as_bytes()),
            equivalence_key: format!("endpoint:{}", GEMINI_OPENAI_COMPAT_BASE.to_ascii_lowercase()),
            exists_locally: false,
            local_diff: LocalDiff::Missing,
            account_id: None,
            secret_policy: SecretPolicy::OptIn,
            seen_in_products: vec![SourceProduct::Antigravity],
            canonical_source: SourceProduct::Antigravity,
            duplicate_of: None,
            payload: Some(serde_json::json!({
                "provider": "google",
                "base_url": GEMINI_OPENAI_COMPAT_BASE,
                "auth_scheme": "bearer",
            })),
        });
    } else if matches!(oauth_kind, AgyAuthKind::GoogleOauth { .. }) {
        out.auth = oauth_kind;
    } else {
        out.auth = oauth_kind;
    }

    // Dynamic model discovery (same pipeline as every other provider).
    // skip_live when no API key: Gemini OpenAI-compat needs a key.
    let mut mreq = crate::providers::ModelDiscoveryRequest::for_provider("antigravity");
    mreq.gemini_home = Some(root.to_path_buf());
    mreq.base_url = Some(GEMINI_OPENAI_COMPAT_BASE.into());
    if let Some((var, val)) = &key_from_env {
        let _ = var;
        mreq.api_key = Some(val.clone());
        mreq.skip_live = false;
    } else {
        // Still try CLI (`agy models`) + config + catalog + recent
        mreq.skip_live = true;
    }
    // When env_keys is Some (tests), avoid accidental process-env CLI noise only
    // if we're under a fixture root without real CLI expectation — still allow CLI.
    let model_rep = crate::providers::discover_models(&mreq);
    out.model_hints = model_rep.ids();
    if !model_rep.models.is_empty() {
        let ids: Vec<_> = model_rep.ids();
        out.findings.push(RemoteFinding {
            id: "agy.models.discovered".into(),
            kind: FindingKind::ModelProvider,
            source_tool: SourceProduct::Antigravity,
            remote_path: out.cli_dir.clone(),
            display_name: format!(
                "Antigravity models ({} via {})",
                ids.len(),
                model_rep.primary_source.label()
            ),
            summary: format!(
                "{} model(s) discovered; primary={}",
                ids.len(),
                model_rep.primary_source.label()
            ),
            content_hash: content_hash(ids.join(",").as_bytes()),
            equivalence_key: "agy:models".into(),
            exists_locally: false,
            local_diff: LocalDiff::Missing,
            account_id: None,
            secret_policy: SecretPolicy::Never,
            seen_in_products: vec![SourceProduct::Antigravity],
            canonical_source: SourceProduct::Antigravity,
            duplicate_of: None,
            payload: Some(serde_json::json!({
                "models": ids,
                "primary_source": format!("{:?}", model_rep.primary_source),
                "sources_tried": model_rep.sources_tried,
            })),
        });
        // Cache CLI results for offline re-scan
        if model_rep
            .models
            .iter()
            .any(|m| m.source == crate::providers::ModelListSource::ProviderCli)
        {
            let pairs: Vec<_> = model_rep
                .models
                .iter()
                .map(|m| (m.id.clone(), m.display_name.clone()))
                .collect();
            let _ = crate::providers::cache_discovered_models(root, &pairs);
        }
    }
    for n in model_rep.notes {
        out.notes.push(format!("models: {n}"));
    }

    // config.json presence
    let cfg = root.join("config/config.json");
    if cfg.is_file() {
        out.findings.push(path_finding(
            "agy.config",
            FindingKind::PathEntry,
            &cfg,
            "Gemini config.json",
            "Antigravity/Gemini shared config",
            content_hash(b"config.json"),
            "agy:config.json".into(),
            SecretPolicy::Never,
        ));
    }

    if out.cli_dir.is_none()
        && out.settings_path.is_none()
        && out.mcp_path.is_none()
        && out.skills.is_empty()
    {
        out.notes.push(
            "No Antigravity CLI tree under Gemini home (install `agy` or copy ~/.gemini)"
                .into(),
        );
    }

    out
}

fn detect_api_key(env_keys: Option<&[(String, String)]>) -> Option<(String, String)> {
    const VARS: &[&str] = &["GEMINI_API_KEY", "GOOGLE_API_KEY", "GOOGLE_GENAI_API_KEY"];
    if let Some(pairs) = env_keys {
        for var in VARS {
            if let Some((_, v)) = pairs.iter().find(|(k, v)| k == var && !v.is_empty()) {
                return Some(((*var).to_string(), v.clone()));
            }
        }
        return None;
    }
    for var in VARS {
        if let Ok(v) = std::env::var(var) {
            if !v.is_empty() {
                return Some(((*var).to_string(), v));
            }
        }
    }
    None
}

pub fn parse_mcp_servers(v: &Value) -> Vec<AgyMcpServer> {
    let mut out = Vec::new();
    // Support { "mcpServers": { ... } } and bare map
    let map = v
        .get("mcpServers")
        .and_then(|x| x.as_object())
        .or_else(|| v.as_object());
    let Some(map) = map else {
        return out;
    };
    // If top-level has non-server keys only, still iterate
    for (name, cfg) in map {
        if name == "mcpServers" {
            continue;
        }
        if !cfg.is_object() {
            continue;
        }
        let command = cfg
            .get("command")
            .and_then(|x| x.as_str())
            .map(str::to_string);
        let args: Vec<String> = cfg
            .get("args")
            .and_then(|a| a.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|x| x.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        let url = cfg
            .get("serverUrl")
            .or_else(|| cfg.get("url"))
            .and_then(|x| x.as_str())
            .map(str::to_string);
        if command.is_none() && url.is_none() {
            continue;
        }
        out.push(AgyMcpServer {
            name: name.clone(),
            command,
            args,
            url,
        });
    }
    out
}

fn collect_skills(root: &Path, builtin: bool, out: &mut Vec<AgySkill>) {
    let Ok(rd) = std::fs::read_dir(root) else {
        return;
    };
    for ent in rd.flatten() {
        let path = ent.path();
        if !path.is_dir() {
            continue;
        }
        let skill_md = path.join("SKILL.md");
        if !skill_md.is_file() {
            continue;
        }
        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("skill")
            .to_string();
        out.push(AgySkill {
            name,
            path,
            builtin,
        });
    }
}

fn summarize_settings(v: &Value) -> String {
    let mut parts = Vec::new();
    if let Some(p) = v.get("toolPermission").and_then(|x| x.as_str()) {
        parts.push(format!("toolPermission={p}"));
    }
    if let Some(n) = v
        .pointer("/permissions/allow")
        .and_then(|a| a.as_array())
        .map(|a| a.len())
    {
        parts.push(format!("allow_rules={n}"));
    }
    if let Some(n) = v
        .get("trustedWorkspaces")
        .and_then(|a| a.as_array())
        .map(|a| a.len())
    {
        parts.push(format!("trusted_workspaces={n}"));
    }
    if v.get("useG1Credits").and_then(|x| x.as_bool()) == Some(true) {
        parts.push("useG1Credits".into());
    }
    if parts.is_empty() {
        "settings present".into()
    } else {
        parts.join(", ")
    }
}

fn content_hash(bytes: &[u8]) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{h:016x}")
}

fn path_finding(
    id: &str,
    kind: FindingKind,
    path: &Path,
    display: &str,
    summary: &str,
    hash: String,
    eq: String,
    secret: SecretPolicy,
) -> RemoteFinding {
    RemoteFinding {
        id: id.into(),
        kind,
        source_tool: SourceProduct::Antigravity,
        remote_path: Some(path.to_path_buf()),
        display_name: display.into(),
        summary: summary.into(),
        content_hash: hash,
        equivalence_key: eq,
        exists_locally: false,
        local_diff: LocalDiff::Missing,
        account_id: None,
        secret_policy: secret,
        seen_in_products: vec![SourceProduct::Antigravity],
        canonical_source: SourceProduct::Antigravity,
        duplicate_of: None,
        payload: None,
    }
}

fn mcp_finding(s: &AgyMcpServer, mcp_path: &Path) -> RemoteFinding {
    let (eq, summary) = if let Some(url) = &s.url {
        (
            equivalence_mcp_url(url),
            format!("MCP {} @ {url}", s.name),
        )
    } else {
        let cmd = s.command.as_deref().unwrap_or("");
        (
            equivalence_mcp_stdio(cmd, &s.args),
            format!(
                "MCP {} stdio {} {}",
                s.name,
                cmd,
                s.args.join(" ")
            ),
        )
    };
    RemoteFinding {
        id: format!("agy.mcp.{}", s.name),
        kind: FindingKind::McpServer,
        source_tool: SourceProduct::Antigravity,
        remote_path: Some(mcp_path.to_path_buf()),
        display_name: s.name.clone(),
        summary,
        content_hash: content_hash(eq.as_bytes()),
        equivalence_key: eq,
        exists_locally: false,
        local_diff: LocalDiff::Missing,
        account_id: None,
        secret_policy: SecretPolicy::Never,
        seen_in_products: vec![SourceProduct::Antigravity],
        canonical_source: SourceProduct::Antigravity,
        duplicate_of: None,
        payload: Some(serde_json::json!({
            "name": s.name,
            "command": s.command,
            "args": s.args,
            "url": s.url,
        })),
    }
}

impl AntigravityImport {
    pub fn summary_text(&self) -> String {
        let mut lines = vec!["Antigravity / agy import scan:".to_string()];
        if let Some(h) = &self.gemini_home {
            lines.push(format!("  gemini home: {}", h.display()));
        }
        if let Some(c) = &self.cli_dir {
            lines.push(format!("  cli dir:     {}", c.display()));
        }
        if let Some(s) = &self.settings_path {
            lines.push(format!("  settings:    {}", s.display()));
        }
        if let Some(sum) = &self.settings_summary {
            lines.push(format!("               ({sum})"));
        }
        if let Some(m) = &self.mcp_path {
            lines.push(format!(
                "  mcp:         {} ({} server(s))",
                m.display(),
                self.mcp_servers.len()
            ));
        }
        for s in &self.mcp_servers {
            if let Some(u) = &s.url {
                lines.push(format!("    • {} → {u}", s.name));
            } else {
                lines.push(format!(
                    "    • {} → {} {}",
                    s.name,
                    s.command.as_deref().unwrap_or("?"),
                    s.args.join(" ")
                ));
            }
        }
        lines.push(format!(
            "  skills:      {} ({} builtin)",
            self.skills.len(),
            self.skills.iter().filter(|s| s.builtin).count()
        ));
        for sk in self.skills.iter().take(12) {
            let tag = if sk.builtin { "builtin" } else { "user" };
            lines.push(format!("    • {} [{tag}]", sk.name));
        }
        if self.skills.len() > 12 {
            lines.push(format!("    … +{} more", self.skills.len() - 12));
        }
        match &self.auth {
            AgyAuthKind::None => lines.push("  auth:        none".into()),
            AgyAuthKind::GoogleOauth {
                auth_method,
                has_refresh,
            } => lines.push(format!(
                "  auth:        Google OAuth (method={}, refresh={}) — not importable as API key",
                auth_method.as_deref().unwrap_or("?"),
                has_refresh
            )),
            AgyAuthKind::ApiKey { source } => {
                lines.push(format!("  auth:        API key via {source}"));
                lines.push(format!("  gemini base: {GEMINI_OPENAI_COMPAT_BASE}"));
            }
        }
        if !self.model_hints.is_empty() {
            lines.push(format!("  model hints: {}", self.model_hints.join(", ")));
        }
        lines.push(format!("  findings:    {}", self.findings.len()));
        for n in &self.notes {
            lines.push(format!("  note: {n}"));
        }
        lines.push(
            "\nNext:\n  • MCP/skills: elect via remote import UI or copy mcp_config into [mcp_servers]\n  • Gemini API: export GEMINI_API_KEY=… then grok providers validate --base-url \\\n      https://generativelanguage.googleapis.com/v1beta/openai --env-key GEMINI_API_KEY\n  • OAuth-only AGY login cannot drive Grok's Google provider yet"
                .into(),
        );
        lines.join("\n")
    }
}

/// Path helpers used by remote catalog (re-export convenience).
pub fn default_gemini_paths(home: &Path) -> Vec<(String, PathBuf, &'static str)> {
    vec![
        (
            "agy.settings".into(),
            home.join(".gemini/antigravity-cli/settings.json"),
            "Antigravity CLI settings",
        ),
        (
            "agy.oauth".into(),
            home.join(".gemini/antigravity-cli/antigravity-oauth-token"),
            "Antigravity OAuth token",
        ),
        (
            "agy.mcp".into(),
            home.join(".gemini/config/mcp_config.json"),
            "Antigravity MCP config",
        ),
        (
            "agy.config".into(),
            home.join(".gemini/config/config.json"),
            "Gemini shared config",
        ),
        (
            "agy.skills.user".into(),
            home.join(".gemini/config/skills"),
            "Antigravity user skills",
        ),
        (
            "agy.skills.builtin".into(),
            home.join(".gemini/antigravity-cli/builtin/skills"),
            "Antigravity builtin skills",
        ),
    ]
}

// Silence unused import warnings when path helpers only used in other modules via paths.rs
#[allow(dead_code)]
fn _path_helpers_linked() {
    let _ = antigravity_cli_dir();
    let _ = antigravity_mcp_config_path();
    let _ = antigravity_oauth_token_path();
    let _ = antigravity_settings_path();
    let _ = gemini_config_json_path();
    let _ = gemini_skills_roots();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write(path: &Path, body: &str) {
        if let Some(p) = path.parent() {
            std::fs::create_dir_all(p).unwrap();
        }
        let mut f = std::fs::File::create(path).unwrap();
        f.write_all(body.as_bytes()).unwrap();
    }

    fn temp_root(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "agy-scan-{}-{}-{}",
            name,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn missing_root_notes() {
        let imp = scan_antigravity_in(None, Some(&[]));
        assert!(imp.notes.iter().any(|n| n.contains("No HOME")));
        assert!(imp.findings.is_empty());
    }

    #[test]
    fn empty_dir_notes_no_cli() {
        let root = temp_root("empty");
        let imp = scan_antigravity_in(Some(&root), Some(&[]));
        assert!(imp.notes.iter().any(|n| n.contains("No Antigravity")));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn parse_mcp_stdio_and_url() {
        let v: Value = serde_json::from_str(
            r#"{
            "mcpServers": {
              "sqlite-helper": {
                "command": "sqlite-mcp-server",
                "args": ["/db.sqlite"],
                "env": {"DB_READONLY": "true"}
              },
              "remote": { "serverUrl": "https://mcp.example/sse" }
            }
        }"#,
        )
        .unwrap();
        let servers = parse_mcp_servers(&v);
        assert_eq!(servers.len(), 2);
        let sql = servers.iter().find(|s| s.name == "sqlite-helper").unwrap();
        assert_eq!(sql.command.as_deref(), Some("sqlite-mcp-server"));
        assert_eq!(sql.args, vec!["/db.sqlite"]);
        let rem = servers.iter().find(|s| s.name == "remote").unwrap();
        assert_eq!(rem.url.as_deref(), Some("https://mcp.example/sse"));
    }

    #[test]
    fn parse_mcp_bare_map() {
        let v: Value = serde_json::from_str(
            r#"{ "honcho": { "command": "npx", "args": ["-y", "x"] } }"#,
        )
        .unwrap();
        let servers = parse_mcp_servers(&v);
        assert_eq!(servers.len(), 1);
        assert_eq!(servers[0].name, "honcho");
    }

    #[test]
    fn parse_mcp_skips_incomplete() {
        let v: Value = serde_json::from_str(r#"{ "mcpServers": { "x": { "env": {} } } }"#).unwrap();
        assert!(parse_mcp_servers(&v).is_empty());
    }

    #[test]
    fn full_fixture_oauth_mcp_skills() {
        let root = temp_root("full");
        write(
            &root.join("antigravity-cli/settings.json"),
            r#"{
              "toolPermission": "always-proceed",
              "permissions": { "allow": ["command(ls)"] },
              "trustedWorkspaces": ["/tmp/ws"],
              "useG1Credits": true
            }"#,
        );
        write(
            &root.join("config/mcp_config.json"),
            r#"{
              "mcpServers": {
                "operant": { "command": "npx", "args": ["-y", "operant-mcp"] },
                "honcho": { "serverUrl": "https://mcp.example/honcho" }
              }
            }"#,
        );
        write(
            &root.join("antigravity-cli/antigravity-oauth-token"),
            r#"{
              "token": {
                "access_token": "ya29.test",
                "token_type": "Bearer",
                "refresh_token": "1//refresh",
                "expiry": "2026-07-21T12:00:00Z"
              },
              "auth_method": "consumer"
            }"#,
        );
        write(
            &root.join("config/config.json"),
            r#"{"userSettings":{}}"#,
        );
        write(
            &root.join("antigravity-cli/builtin/skills/agy-guide/SKILL.md"),
            "---\nname: agy-guide\n---\n# Guide\n",
        );
        write(
            &root.join("config/skills/my-skill/SKILL.md"),
            "---\nname: my-skill\n---\n# User\n",
        );

        let imp = scan_antigravity_in(Some(&root), Some(&[]));
        assert!(imp.cli_dir.is_some());
        assert_eq!(imp.mcp_servers.len(), 2);
        assert_eq!(imp.skills.len(), 2);
        assert!(imp.skills.iter().any(|s| s.name == "my-skill" && !s.builtin));
        assert!(imp
            .skills
            .iter()
            .any(|s| s.name == "agy-guide" && s.builtin));
        assert!(matches!(
            imp.auth,
            AgyAuthKind::GoogleOauth {
                has_refresh: true,
                ..
            }
        ));
        assert!(!imp.model_hints.is_empty());
        assert!(imp.findings.iter().any(|f| f.kind == FindingKind::McpServer));
        assert!(imp.findings.iter().any(|f| f.kind == FindingKind::Skill));
        assert!(imp.findings.iter().any(|f| f.id == "agy.oauth"));
        // OAuth must never be RequiredForApply / OptIn secret apply
        let oauth = imp.findings.iter().find(|f| f.id == "agy.oauth").unwrap();
        assert_eq!(oauth.secret_policy, SecretPolicy::Never);
        assert!(oauth
            .payload
            .as_ref()
            .unwrap()
            .get("usable_as_api_key")
            .and_then(|x| x.as_bool())
            == Some(false));

        let text = imp.summary_text();
        assert!(text.contains("Antigravity"));
        assert!(text.contains("Google OAuth"));
        assert!(text.contains("operant") || text.contains("honcho"));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn api_key_env_produces_provider_finding() {
        let root = temp_root("apikey");
        write(
            &root.join("antigravity-cli/settings.json"),
            r#"{"toolPermission":"default"}"#,
        );
        let env = vec![("GEMINI_API_KEY".into(), "AIza-test-key".into())];
        let imp = scan_antigravity_in(Some(&root), Some(&env));
        assert!(matches!(imp.auth, AgyAuthKind::ApiKey { .. }));
        assert!(imp.env_api_key_present);
        assert!(imp
            .findings
            .iter()
            .any(|f| f.kind == FindingKind::ModelProvider && f.id == "agy.provider.google"));
        assert!(imp
            .findings
            .iter()
            .any(|f| f.kind == FindingKind::EnvVar));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn empty_mcp_file_notes() {
        let root = temp_root("emptymcp");
        write(&root.join("config/mcp_config.json"), "");
        let imp = scan_antigravity_in(Some(&root), Some(&[]));
        assert!(imp.notes.iter().any(|n| n.contains("empty")));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn bad_json_notes() {
        let root = temp_root("badjson");
        write(&root.join("antigravity-cli/settings.json"), "{not json");
        write(&root.join("config/mcp_config.json"), "{also bad");
        write(
            &root.join("antigravity-cli/antigravity-oauth-token"),
            "not-json",
        );
        let imp = scan_antigravity_in(Some(&root), Some(&[]));
        assert!(imp.notes.iter().any(|n| n.contains("parse error")));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn summarize_settings_fields() {
        let v: Value = serde_json::from_str(
            r#"{"toolPermission":"always-proceed","permissions":{"allow":[1,2]},"trustedWorkspaces":["a"],"useG1Credits":true}"#,
        )
        .unwrap();
        let s = summarize_settings(&v);
        assert!(s.contains("toolPermission=always-proceed"));
        assert!(s.contains("allow_rules=2"));
        assert!(s.contains("trusted_workspaces=1"));
        assert!(s.contains("useG1Credits"));
    }

    #[test]
    fn summarize_settings_empty() {
        let v: Value = serde_json::json!({});
        assert_eq!(summarize_settings(&v), "settings present");
    }

    #[test]
    fn default_gemini_paths_under_home() {
        let paths = default_gemini_paths(Path::new("/home/alice"));
        assert!(paths.iter().any(|(id, p, _)| id == "agy.mcp"
            && p == Path::new("/home/alice/.gemini/config/mcp_config.json")));
    }

    #[test]
    fn mcp_findings_dedup_keys_stable() {
        let root = temp_root("dedup");
        write(
            &root.join("config/mcp_config.json"),
            r#"{"mcpServers":{"a":{"command":"npx","args":["-y","pkg"]}}}"#,
        );
        let imp = scan_antigravity_in(Some(&root), Some(&[]));
        let f = imp
            .findings
            .iter()
            .find(|f| f.kind == FindingKind::McpServer)
            .unwrap();
        assert!(f.equivalence_key.starts_with("mcp:stdio:"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn detect_api_key_prefers_gemini() {
        let env = vec![
            ("GOOGLE_API_KEY".into(), "g".into()),
            ("GEMINI_API_KEY".into(), "m".into()),
        ];
        // GEMINI listed first in VARS
        let got = detect_api_key(Some(&env)).unwrap();
        assert_eq!(got.0, "GEMINI_API_KEY");
    }

    #[test]
    fn detect_api_key_empty_skipped() {
        let env = vec![("GEMINI_API_KEY".into(), "".into())];
        assert!(detect_api_key(Some(&env)).is_none());
    }
}
