//! Scan the machine for LLM-related installs (local, env, config, sibling tools).

use serde::Serialize;
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Default local OpenAI-compatible endpoints to probe.
pub const DEFAULT_LOCAL_ENDPOINTS: &[(&str, &str, u16)] = &[
    ("ollama", "http://127.0.0.1:11434/v1", 11434),
    ("lmstudio", "http://127.0.0.1:1234/v1", 1234),
    ("llama.cpp", "http://127.0.0.1:8080/v1", 8080),
];

/// Common third-party API key env vars (presence only — never read values into logs).
pub const COMMON_API_KEY_ENVS: &[(&str, &str)] = &[
    ("OPENAI_API_KEY", "openai"),
    ("ANTHROPIC_API_KEY", "anthropic"),
    ("ANTHROPIC_AUTH_TOKEN", "anthropic"),
    ("MINIMAX_API_KEY", "minimax"),
    ("XAI_API_KEY", "xai"),
    ("GROK_CODE_XAI_API_KEY", "xai"),
    ("OPENROUTER_API_KEY", "openrouter"),
    ("TOGETHER_API_KEY", "togetherai"),
    ("GROQ_API_KEY", "groq"),
    ("DEEPSEEK_API_KEY", "deepseek"),
    ("MISTRAL_API_KEY", "mistral"),
    ("GOOGLE_API_KEY", "google"),
    ("GEMINI_API_KEY", "google"),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FoundKind {
    LocalServer,
    EnvCredential,
    GrokModelConfig,
    SiblingTool,
    Binary,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FoundItem {
    pub kind: FoundKind,
    pub id: String,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    /// Credential is present in env/file — never the secret itself.
    pub credential_present: bool,
    /// TCP/L0 reachable when applicable.
    pub reachable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FoundInstallReport {
    pub items: Vec<FoundItem>,
    pub scanned_at_unix: u64,
}

#[derive(Debug, Clone)]
pub struct DiscoverOptions {
    pub tcp_timeout: Duration,
    pub probe_local: bool,
    pub probe_env: bool,
    pub probe_sibling: bool,
    pub probe_binaries: bool,
    /// Project cwd for sibling/project config paths.
    pub cwd: Option<PathBuf>,
}

impl Default for DiscoverOptions {
    fn default() -> Self {
        Self {
            tcp_timeout: Duration::from_millis(200),
            probe_local: true,
            probe_env: true,
            probe_sibling: true,
            probe_binaries: true,
            cwd: std::env::current_dir().ok(),
        }
    }
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn tcp_reachable(host: &str, port: u16, timeout: Duration) -> bool {
    let addr = format!("{host}:{port}");
    let Ok(mut addrs) = addr.to_socket_addrs() else {
        return false;
    };
    let Some(sock): Option<SocketAddr> = addrs.next() else {
        return false;
    };
    TcpStream::connect_timeout(&sock, timeout).is_ok()
}

fn path_exists(p: &Path) -> bool {
    p.exists()
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

/// Run discovery (sync, bounded — suitable for doctor/startup).
pub fn discover_installed(opts: &DiscoverOptions) -> FoundInstallReport {
    let mut items = Vec::new();

    if opts.probe_local {
        for (name, base_url, port) in DEFAULT_LOCAL_ENDPOINTS {
            let ok = tcp_reachable("127.0.0.1", *port, opts.tcp_timeout);
            items.push(FoundItem {
                kind: FoundKind::LocalServer,
                id: format!("local:{name}"),
                label: format!("{name} ({base_url})"),
                provider_id: Some((*name).to_string()),
                base_url: Some((*base_url).to_string()),
                credential_present: false,
                reachable: ok,
                detail: if ok {
                    Some("TCP open".into())
                } else {
                    Some("not listening".into())
                },
            });
        }
    }

    if opts.probe_env {
        for (env_name, provider) in COMMON_API_KEY_ENVS {
            if let Ok(v) = std::env::var(env_name) {
                if !v.trim().is_empty() {
                    items.push(FoundItem {
                        kind: FoundKind::EnvCredential,
                        id: format!("env:{env_name}"),
                        label: format!("{env_name} set"),
                        provider_id: Some((*provider).to_string()),
                        base_url: xai_grok_models::default_base_url(provider).map(str::to_string),
                        credential_present: true,
                        reachable: false, // not probed here (cloud)
                        detail: Some("env present (value hidden)".into()),
                    });
                }
            }
        }
    }

    if opts.probe_binaries {
        for bin in ["ollama"] {
            if which_ok(bin) {
                items.push(FoundItem {
                    kind: FoundKind::Binary,
                    id: format!("bin:{bin}"),
                    label: format!("`{bin}` on PATH"),
                    provider_id: Some(bin.to_string()),
                    base_url: None,
                    credential_present: false,
                    reachable: true,
                    detail: Some("binary found".into()),
                });
            }
        }
    }

    if opts.probe_sibling {
        if let Some(home) = home_dir() {
            let siblings: &[(&str, PathBuf, &str)] = &[
                (
                    "claude",
                    home.join(".claude"),
                    "Claude Code config dir",
                ),
                (
                    "cursor",
                    home.join(".cursor"),
                    "Cursor config dir",
                ),
                (
                    "opencode",
                    home.join(".config/opencode"),
                    "OpenCode config dir",
                ),
                (
                    "opencode-auth",
                    home.join(".local/share/opencode"),
                    "OpenCode auth store",
                ),
                (
                    "antigravity",
                    home.join(".gemini"),
                    "Antigravity / Gemini home (agy)",
                ),
            ];
            for (id, path, label) in siblings {
                if path_exists(path) {
                    let provider_id = match *id {
                        "antigravity" => Some("antigravity".to_string()),
                        "opencode" | "opencode-auth" => Some("opencode".to_string()),
                        _ => None,
                    };
                    items.push(FoundItem {
                        kind: FoundKind::SiblingTool,
                        id: format!("sibling:{id}"),
                        label: (*label).to_string(),
                        provider_id,
                        base_url: None,
                        credential_present: false,
                        reachable: true,
                        detail: Some(path.display().to_string()),
                    });
                }
            }
            // OpenCode project file
            if let Some(cwd) = &opts.cwd {
                for name in ["opencode.json", "opencode.jsonc"] {
                    let p = cwd.join(name);
                    if p.is_file() {
                        items.push(FoundItem {
                            kind: FoundKind::SiblingTool,
                            id: format!("sibling:opencode-project-{name}"),
                            label: format!("project {name}"),
                            provider_id: None,
                            base_url: None,
                            credential_present: false,
                            reachable: true,
                            detail: Some(p.display().to_string()),
                        });
                    }
                }
            }
        }
    }

    // Existing Grok [model.*] is caller-enriched; scan ~/.grok/config.toml lightly.
    if let Some(home) = home_dir() {
        let cfg = home.join(".grok/config.toml");
        if cfg.is_file() {
            if let Ok(text) = std::fs::read_to_string(&cfg) {
                // Cheap count of [model. sections
                let n = text.matches("[model.").count();
                if n > 0 {
                    items.push(FoundItem {
                        kind: FoundKind::GrokModelConfig,
                        id: "grok:config-models".into(),
                        label: format!("{n} [model.*] block(s) in ~/.grok/config.toml"),
                        provider_id: None,
                        base_url: None,
                        credential_present: false,
                        reachable: true,
                        detail: Some(cfg.display().to_string()),
                    });
                }
            }
        }
        let creds = home.join(".grok/credentials.json");
        if creds.is_file() {
            items.push(FoundItem {
                kind: FoundKind::GrokModelConfig,
                id: "grok:credentials".into(),
                label: "credentials store present".into(),
                provider_id: None,
                base_url: None,
                credential_present: true,
                reachable: true,
                detail: Some(creds.display().to_string()),
            });
        }
    }

    FoundInstallReport {
        items,
        scanned_at_unix: now_unix(),
    }
}

fn which_ok(bin: &str) -> bool {
    let Ok(path) = std::env::var("PATH") else {
        return false;
    };
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(bin);
        if candidate.is_file() {
            return true;
        }
    }
    false
}

impl FoundInstallReport {
    /// Human multi-line summary for TUI / CLI.
    pub fn summary_text(&self) -> String {
        if self.items.is_empty() {
            return "No local providers, API keys, or sibling tool configs detected."
                .to_string();
        }
        let mut lines = vec!["Found on this machine:".to_string()];
        for it in &self.items {
            let mark = if it.reachable || it.credential_present {
                "•"
            } else {
                "○"
            };
            let extra = it.detail.as_deref().unwrap_or("");
            lines.push(format!("  {mark} {} — {extra}", it.label));
        }
        lines.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discover_runs_offline() {
        let report = discover_installed(&DiscoverOptions {
            tcp_timeout: Duration::from_millis(50),
            ..Default::default()
        });
        // Always at least probes local endpoints (even if down).
        assert!(
            report.items.iter().any(|i| i.kind == FoundKind::LocalServer),
            "expected local server probes"
        );
    }

    #[test]
    fn env_key_detected_when_set() {
        // SAFETY: test-only, single-threaded test process.
        unsafe { std::env::set_var("GROK_TEST_FAKE_PROVIDER_KEY", "sk-test") };
        // Use a real key name
        unsafe { std::env::set_var("OPENAI_API_KEY", "sk-test-not-real") };
        let report = discover_installed(&DiscoverOptions {
            probe_local: false,
            probe_sibling: false,
            probe_binaries: false,
            probe_env: true,
            tcp_timeout: Duration::from_millis(10),
            cwd: None,
        });
        unsafe { std::env::remove_var("OPENAI_API_KEY") };
        unsafe { std::env::remove_var("GROK_TEST_FAKE_PROVIDER_KEY") };
        assert!(
            report
                .items
                .iter()
                .any(|i| i.id == "env:OPENAI_API_KEY" && i.credential_present),
            "items={:?}",
            report.items
        );
    }
}
