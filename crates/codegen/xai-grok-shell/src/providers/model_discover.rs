//! Dynamic model discovery for **any** provider.
//!
//! Order of preference (first non-empty wins for *primary*, but all sources
//! are merged/deduped by model id):
//!
//! 1. **Live endpoint** — `GET {base}/models` (OpenAI-compat / Ollama-style)
//! 2. **Provider CLI** — `ollama list`, `agy models`, …
//! 3. **Config files** — OpenCode, Antigravity settings/config, Grok config
//! 4. **History scrape** — recent ids in sibling tool logs (best-effort)
//! 5. **Catalog** — models.dev offline snapshot for that provider
//! 6. **Recent fallback** — curated recent ids when everything else is empty
//!
//! Import scanners and `grok providers models` both use this path so AGY is
//! not special-cased: every provider gets the same discovery pipeline.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;
use xai_grok_sampler::AuthScheme;

/// Where a model id was observed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelListSource {
    LiveEndpoint,
    ProviderCli,
    ConfigFile,
    History,
    Catalog,
    RecentFallback,
}

impl ModelListSource {
    pub fn label(self) -> &'static str {
        match self {
            Self::LiveEndpoint => "live /models",
            Self::ProviderCli => "provider CLI",
            Self::ConfigFile => "config file",
            Self::History => "history",
            Self::Catalog => "models.dev catalog",
            Self::RecentFallback => "recent fallback",
        }
    }

    /// Lower = preferred when merging duplicates.
    pub fn rank(self) -> u8 {
        match self {
            Self::LiveEndpoint => 0,
            Self::ProviderCli => 1,
            Self::ConfigFile => 2,
            Self::History => 3,
            Self::Catalog => 4,
            Self::RecentFallback => 5,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveredModel {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    pub provider_id: String,
    pub source: ModelListSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelDiscoveryReport {
    pub provider_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    pub models: Vec<DiscoveredModel>,
    /// Best source that contributed at least one model.
    pub primary_source: ModelListSource,
    #[serde(default)]
    pub sources_tried: Vec<String>,
    #[serde(default)]
    pub notes: Vec<String>,
}

impl ModelDiscoveryReport {
    pub fn summary_text(&self) -> String {
        let mut lines = vec![format!(
            "Models for provider `{}` ({} via {}):",
            self.provider_id,
            self.models.len(),
            self.primary_source.label()
        )];
        if let Some(u) = &self.base_url {
            lines.push(format!("  base_url: {u}"));
        }
        lines.push(format!("  tried: {}", self.sources_tried.join(" → ")));
        for m in self.models.iter().take(40) {
            let name = m.display_name.as_deref().unwrap_or("");
            if name.is_empty() || name == m.id {
                lines.push(format!("  • {}  [{}]", m.id, m.source.label()));
            } else {
                lines.push(format!(
                    "  • {} ({name})  [{}]",
                    m.id,
                    m.source.label()
                ));
            }
        }
        if self.models.len() > 40 {
            lines.push(format!("  … +{} more", self.models.len() - 40));
        }
        for n in &self.notes {
            lines.push(format!("  note: {n}"));
        }
        lines.join("\n")
    }

    pub fn ids(&self) -> Vec<String> {
        self.models.iter().map(|m| m.id.clone()).collect()
    }
}

/// Request parameters for [`discover_models`].
#[derive(Debug, Clone, Default)]
pub struct ModelDiscoveryRequest {
    /// Logical provider id: `openai`, `google`, `ollama`, `antigravity`, …
    pub provider_id: String,
    pub base_url: Option<String>,
    pub api_key: Option<String>,
    pub auth_scheme: AuthScheme,
    /// Skip live HTTP (tests / offline).
    pub skip_live: bool,
    /// Skip spawning provider CLIs.
    pub skip_cli: bool,
    /// Skip models.dev catalog.
    pub skip_catalog: bool,
    /// Skip recent builtin fallback (usually leave false).
    pub skip_recent_fallback: bool,
    /// Optional gemini/agy home for config scrape.
    pub gemini_home: Option<PathBuf>,
    /// Optional project cwd for OpenCode project configs.
    pub cwd: Option<PathBuf>,
    pub http_timeout: Duration,
}

impl ModelDiscoveryRequest {
    pub fn for_provider(provider_id: impl Into<String>) -> Self {
        let provider_id = provider_id.into();
        let base_url = xai_grok_models::default_base_url(&provider_id).map(str::to_string);
        // Local aliases
        let base_url = base_url.or_else(|| match provider_id.as_str() {
            "ollama" => Some("http://127.0.0.1:11434/v1".into()),
            "lmstudio" => Some("http://127.0.0.1:1234/v1".into()),
            "antigravity" | "agy" | "gemini" => {
                Some("https://generativelanguage.googleapis.com/v1beta/openai".into())
            }
            _ => None,
        });
        let auth_scheme = match provider_id.as_str() {
            "anthropic" => AuthScheme::XApiKey,
            _ => AuthScheme::Bearer,
        };
        Self {
            provider_id,
            base_url,
            auth_scheme,
            http_timeout: Duration::from_secs(12),
            ..Default::default()
        }
    }

    pub fn with_base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = Some(url.into());
        self
    }

    pub fn with_api_key(mut self, key: impl Into<String>) -> Self {
        self.api_key = Some(key.into());
        self
    }

    pub fn offline_only(mut self) -> Self {
        self.skip_live = true;
        self.skip_cli = true;
        self
    }
}

/// Discover models for a provider using every available source.
pub fn discover_models(req: &ModelDiscoveryRequest) -> ModelDiscoveryReport {
    let mut notes = Vec::new();
    let mut tried = Vec::new();
    let mut by_id: BTreeMap<String, DiscoveredModel> = BTreeMap::new();

    let provider = normalize_provider_id(&req.provider_id);
    let base = req.base_url.clone();

    // 1. Live endpoint
    if !req.skip_live {
        if let Some(url) = base.as_deref() {
            tried.push("live".into());
            match list_models_http(url, req.api_key.as_deref(), req.auth_scheme, req.http_timeout)
            {
                Ok(ids) if !ids.is_empty() => {
                    for id in ids {
                        insert_model(
                            &mut by_id,
                            DiscoveredModel {
                                id: id.clone(),
                                display_name: None,
                                provider_id: provider.clone(),
                                source: ModelListSource::LiveEndpoint,
                                base_url: Some(url.to_string()),
                            },
                        );
                    }
                }
                Ok(_) => notes.push(format!("live /models returned empty for {url}")),
                Err(e) => notes.push(format!("live /models: {e}")),
            }
        } else {
            tried.push("live(skipped-no-url)".into());
        }
    }

    // 2. Provider CLI
    if !req.skip_cli {
        tried.push("cli".into());
        match list_models_cli(&provider) {
            Ok(rows) if !rows.is_empty() => {
                for (id, display) in rows {
                    insert_model(
                        &mut by_id,
                        DiscoveredModel {
                            id,
                            display_name: display,
                            provider_id: provider.clone(),
                            source: ModelListSource::ProviderCli,
                            base_url: base.clone(),
                        },
                    );
                }
            }
            Ok(_) => notes.push(format!("CLI listed no models for {provider}")),
            Err(e) => notes.push(format!("CLI: {e}")),
        }
    }

    // 3. Config files
    tried.push("config".into());
    let config_models = list_models_from_configs(&provider, req);
    if config_models.is_empty() {
        notes.push("no model ids in local config files".into());
    } else {
        for m in config_models {
            insert_model(&mut by_id, m);
        }
    }

    // 4. History (agy / opencode lightweight)
    tried.push("history".into());
    let hist = list_models_from_history(&provider, req);
    for m in hist {
        insert_model(&mut by_id, m);
    }

    // 5. models.dev catalog
    if !req.skip_catalog {
        tried.push("catalog".into());
        match list_models_from_catalog(&provider) {
            Ok(ids) if !ids.is_empty() => {
                for id in ids {
                    insert_model(
                        &mut by_id,
                        DiscoveredModel {
                            id,
                            display_name: None,
                            provider_id: provider.clone(),
                            source: ModelListSource::Catalog,
                            base_url: base.clone(),
                        },
                    );
                }
            }
            Ok(_) => notes.push(format!("catalog has no models for {provider}")),
            Err(e) => notes.push(format!("catalog: {e}")),
        }
    }

    // 6. Recent fallback — only if still empty
    if by_id.is_empty() && !req.skip_recent_fallback {
        tried.push("recent_fallback".into());
        for id in recent_models_for_provider(&provider) {
            insert_model(
                &mut by_id,
                DiscoveredModel {
                    id: id.to_string(),
                    display_name: None,
                    provider_id: provider.clone(),
                    source: ModelListSource::RecentFallback,
                    base_url: base.clone(),
                },
            );
        }
        if by_id.is_empty() {
            notes.push(format!(
                "no recent fallback models known for provider `{provider}`"
            ));
        } else {
            notes.push(
                "used recent fallback list (live/CLI/config/catalog empty)".into(),
            );
        }
    }

    let models: Vec<_> = by_id.into_values().collect();
    let primary_source = models
        .iter()
        .map(|m| m.source)
        .min_by_key(|s| s.rank())
        .unwrap_or(ModelListSource::RecentFallback);

    ModelDiscoveryReport {
        provider_id: provider,
        base_url: base,
        models,
        primary_source,
        sources_tried: tried,
        notes,
    }
}

fn normalize_provider_id(id: &str) -> String {
    match id.trim().to_ascii_lowercase().as_str() {
        "agy" | "gemini" | "google-antigravity" => "antigravity".into(),
        "google-ai" | "google_ai" => "google".into(),
        other => other.to_string(),
    }
}

fn insert_model(map: &mut BTreeMap<String, DiscoveredModel>, m: DiscoveredModel) {
    let key = m.id.to_ascii_lowercase();
    match map.get(&key) {
        Some(existing) if existing.source.rank() <= m.source.rank() => {
            // keep better source; maybe fill display_name
            if existing.display_name.is_none() && m.display_name.is_some() {
                let mut e = existing.clone();
                e.display_name = m.display_name;
                map.insert(key, e);
            }
        }
        _ => {
            map.insert(key, m);
        }
    }
}

/// GET `{base}/models` and extract ids (OpenAI / Ollama / Gemini-compat).
pub fn list_models_http(
    base_url: &str,
    api_key: Option<&str>,
    auth_scheme: AuthScheme,
    timeout: Duration,
) -> Result<Vec<String>, String> {
    let base = base_url.trim_end_matches('/');
    let url = format!("{base}/models");
    let client = reqwest::blocking::Client::builder()
        .timeout(timeout)
        .user_agent("grok-build-model-discover/0.1")
        .build()
        .map_err(|e| e.to_string())?;
    let mut req = client.get(&url);
    if let Some(key) = api_key.filter(|k| !k.is_empty()) {
        req = match auth_scheme {
            AuthScheme::XApiKey => req
                .header("x-api-key", key)
                .header("anthropic-version", "2023-06-01"),
            AuthScheme::Bearer => req.bearer_auth(key),
        };
    }
    let resp = req.send().map_err(|e| e.to_string())?;
    let status = resp.status();
    if !status.is_success() {
        return Err(format!("HTTP {status} from {url}"));
    }
    let v: serde_json::Value = resp.json().map_err(|e| e.to_string())?;
    Ok(extract_model_ids(&v))
}

/// Shared OpenAI-compat / Ollama JSON shapes.
pub fn extract_model_ids(v: &serde_json::Value) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(arr) = v.get("data").and_then(|d| d.as_array()) {
        for item in arr {
            if let Some(id) = item.get("id").and_then(|x| x.as_str()) {
                out.push(id.to_string());
            }
        }
    } else if let Some(arr) = v.as_array() {
        for item in arr {
            if let Some(id) = item
                .get("id")
                .or_else(|| item.get("name"))
                .and_then(|x| x.as_str())
            {
                out.push(id.to_string());
            }
        }
    } else if let Some(models) = v.get("models").and_then(|m| m.as_array()) {
        for item in models {
            if let Some(id) = item
                .get("name")
                .or_else(|| item.get("id"))
                .and_then(|x| x.as_str())
            {
                out.push(id.to_string());
            }
        }
    }
    // Dedup preserve order
    let mut seen = std::collections::BTreeSet::new();
    out.retain(|id| seen.insert(id.to_ascii_lowercase()));
    out
}

/// Provider-specific CLI listing → (id, optional display).
pub fn list_models_cli(provider: &str) -> Result<Vec<(String, Option<String>)>, String> {
    match provider {
        "ollama" => list_ollama_cli(),
        "antigravity" => list_agy_cli(),
        _ => Ok(Vec::new()),
    }
}

fn list_ollama_cli() -> Result<Vec<(String, Option<String>)>, String> {
    if !which_ok("ollama") {
        return Err("ollama not on PATH".into());
    }
    let out = Command::new("ollama")
        .args(["list"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(format!(
            "ollama list exit {}",
            out.status.code().unwrap_or(-1)
        ));
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let mut rows = Vec::new();
    for (i, line) in text.lines().enumerate() {
        if i == 0 && line.to_ascii_lowercase().contains("name") {
            continue; // header
        }
        let name = line.split_whitespace().next().unwrap_or("").trim();
        if name.is_empty() {
            continue;
        }
        rows.push((name.to_string(), None));
    }
    Ok(rows)
}

fn list_agy_cli() -> Result<Vec<(String, Option<String>)>, String> {
    let bin = if which_ok("agy") {
        "agy"
    } else if which_ok("antigravity") {
        "antigravity"
    } else {
        return Err("agy/antigravity not on PATH".into());
    };
    let out = Command::new(bin)
        .arg("models")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(format!(
            "{bin} models exit {}",
            out.status.code().unwrap_or(-1)
        ));
    }
    let text = String::from_utf8_lossy(&out.stdout);
    Ok(parse_agy_models_output(&text))
}

/// Parse human lines from `agy models` into (slug, display).
///
/// Example line: `Gemini 3.5 Flash (Medium)` → id `gemini-3.5-flash-medium`.
pub fn parse_agy_models_output(text: &str) -> Vec<(String, Option<String>)> {
    let mut rows = Vec::new();
    for line in text.lines() {
        let display = line.trim();
        if display.is_empty() || display.starts_with("Usage:") || display.starts_with("Flags:") {
            continue;
        }
        // Skip help noise
        if display.starts_with('-') || display.starts_with("List ") {
            continue;
        }
        let id = display_name_to_model_id(display);
        if id.is_empty() {
            continue;
        }
        rows.push((id, Some(display.to_string())));
    }
    rows
}

/// `Gemini 3.5 Flash (Medium)` → `gemini-3.5-flash-medium`
pub fn display_name_to_model_id(display: &str) -> String {
    let mut s = display.to_ascii_lowercase();
    s = s.replace('(', " ").replace(')', " ");
    s = s
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' {
                c
            } else if c.is_whitespace() || c == '/' {
                '-'
            } else {
                '-'
            }
        })
        .collect();
    while s.contains("--") {
        s = s.replace("--", "-");
    }
    s.trim_matches('-').to_string()
}

fn list_models_from_configs(
    provider: &str,
    req: &ModelDiscoveryRequest,
) -> Vec<DiscoveredModel> {
    let mut out = Vec::new();
    match provider {
        "antigravity" | "google" => {
            let root = req
                .gemini_home
                .clone()
                .or_else(|| home().map(|h| h.join(".gemini")));
            if let Some(root) = root {
                for rel in [
                    "antigravity-cli/settings.json",
                    "config/config.json",
                    "antigravity-cli/cache/onboarding.json",
                ] {
                    let p = root.join(rel);
                    if let Ok(raw) = std::fs::read_to_string(&p) {
                        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&raw) {
                            for id in collect_model_strings_in_json(&v) {
                                out.push(DiscoveredModel {
                                    id,
                                    display_name: None,
                                    provider_id: provider.into(),
                                    source: ModelListSource::ConfigFile,
                                    base_url: req.base_url.clone(),
                                });
                            }
                        }
                    }
                }
                // Persist cache written by a previous successful agy models run
                let cache = root.join("antigravity-cli/cache/discovered_models.json");
                if let Ok(raw) = std::fs::read_to_string(&cache) {
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&raw) {
                        if let Some(arr) = v.get("models").and_then(|m| m.as_array()) {
                            for item in arr {
                                let id = item
                                    .get("id")
                                    .and_then(|x| x.as_str())
                                    .or_else(|| item.as_str())
                                    .map(str::to_string);
                                let Some(id) = id else { continue };
                                out.push(DiscoveredModel {
                                    id,
                                    display_name: item
                                        .get("displayName")
                                        .or_else(|| item.get("name"))
                                        .and_then(|x| x.as_str())
                                        .map(str::to_string),
                                    provider_id: provider.into(),
                                    source: ModelListSource::ConfigFile,
                                    base_url: req.base_url.clone(),
                                });
                            }
                        }
                    }
                }
            }
        }
        "opencode" | "ollama" | "openai" | "anthropic" | "openrouter" | "xai" | "minimax"
        | "groq" | "mistral" | "deepseek" | "togetherai" => {
            // OpenCode config models
            for path in opencode_config_candidates(req.cwd.as_deref()) {
                if let Ok(raw) = std::fs::read_to_string(&path) {
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(
                        &strip_jsonc_line_comments(&raw),
                    ) {
                        out.extend(models_from_opencode_json(&v, provider, req.base_url.clone()));
                    }
                }
            }
        }
        _ => {}
    }

    // Grok config.toml [model.*] always useful
    if let Some(home) = home() {
        let cfg = home.join(".grok/config.toml");
        if let Ok(text) = std::fs::read_to_string(&cfg) {
            for id in models_from_grok_config_toml(&text, provider) {
                out.push(DiscoveredModel {
                    id,
                    display_name: None,
                    provider_id: provider.into(),
                    source: ModelListSource::ConfigFile,
                    base_url: req.base_url.clone(),
                });
            }
        }
    }
    out
}

fn list_models_from_history(
    provider: &str,
    req: &ModelDiscoveryRequest,
) -> Vec<DiscoveredModel> {
    let mut out = Vec::new();
    if provider != "antigravity" && provider != "google" {
        return out;
    }
    let root = req
        .gemini_home
        .clone()
        .or_else(|| home().map(|h| h.join(".gemini")));
    let Some(root) = root else {
        return out;
    };
    let hist = root.join("antigravity-cli/history.jsonl");
    if let Ok(raw) = std::fs::read_to_string(&hist) {
        // Cap scan size
        let slice = if raw.len() > 512_000 {
            &raw[raw.len() - 512_000..]
        } else {
            &raw
        };
        for id in scrape_model_ids_from_text(slice) {
            out.push(DiscoveredModel {
                id,
                display_name: None,
                provider_id: provider.into(),
                source: ModelListSource::History,
                base_url: req.base_url.clone(),
            });
        }
    }
    out
}

fn list_models_from_catalog(provider: &str) -> Result<Vec<String>, String> {
    let catalog_id = match provider {
        "antigravity" => "google",
        other => other,
    };
    let loaded = xai_grok_models::load_catalog().map_err(|e| e.to_string())?;
    let Some(prov) = loaded.catalog.get(catalog_id) else {
        return Ok(Vec::new());
    };
    let mut ids: Vec<String> = prov.models.keys().cloned().collect();
    ids.sort();
    // Cap huge catalogs to a usable recent slice (last 40 by name + first 20 popular-ish)
    if ids.len() > 60 {
        let mut head: Vec<_> = ids.iter().take(20).cloned().collect();
        let mut tail: Vec<_> = ids.iter().rev().take(40).cloned().collect();
        tail.reverse();
        head.extend(tail);
        let mut seen = std::collections::BTreeSet::new();
        head.retain(|i| seen.insert(i.clone()));
        ids = head;
    }
    Ok(ids)
}

/// Curated recent models when discovery finds nothing.
///
/// Keep short; live/CLI/catalog should usually win.
pub fn recent_models_for_provider(provider: &str) -> &'static [&'static str] {
    match provider {
        "openai" => &[
            "gpt-4.1",
            "gpt-4.1-mini",
            "gpt-4o",
            "gpt-4o-mini",
            "o3",
            "o4-mini",
        ],
        "anthropic" => &[
            "claude-opus-4-20250514",
            "claude-sonnet-4-20250514",
            "claude-3-5-haiku-latest",
        ],
        "google" | "antigravity" => &[
            "gemini-2.5-pro",
            "gemini-2.5-flash",
            "gemini-2.0-flash",
            "gemini-3.5-flash",
            "gemini-3.1-pro",
        ],
        "xai" => &["grok-4", "grok-4.5", "grok-3", "grok-3-mini"],
        "openrouter" => &[
            "openai/gpt-4o",
            "anthropic/claude-sonnet-4",
            "google/gemini-2.5-pro",
        ],
        "ollama" => &["llama3.2", "llama3.1", "mistral", "qwen2.5", "codellama"],
        "groq" => &[
            "llama-3.3-70b-versatile",
            "llama-3.1-8b-instant",
            "mixtral-8x7b-32768",
        ],
        "mistral" => &["mistral-large-latest", "mistral-small-latest", "codestral-latest"],
        "deepseek" => &["deepseek-chat", "deepseek-reasoner"],
        "minimax" => &["MiniMax-M1", "MiniMax-Text-01"],
        "togetherai" => &["meta-llama/Meta-Llama-3.1-70B-Instruct-Turbo"],
        "lmstudio" => &["local-model"],
        _ => &[],
    }
}

/// Write AGY CLI discovery results into cache for offline re-import.
pub fn cache_discovered_models(
    gemini_home: &Path,
    models: &[(String, Option<String>)],
) -> std::io::Result<PathBuf> {
    let dir = gemini_home.join("antigravity-cli/cache");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join("discovered_models.json");
    let arr: Vec<serde_json::Value> = models
        .iter()
        .map(|(id, display)| {
            serde_json::json!({
                "id": id,
                "displayName": display,
            })
        })
        .collect();
    let body = serde_json::json!({
        "updated_at_unix": now_unix(),
        "source": "agy models",
        "models": arr,
    });
    std::fs::write(
        &path,
        serde_json::to_vec_pretty(&body).unwrap_or_default(),
    )?;
    Ok(path)
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

fn which_ok(bin: &str) -> bool {
    let Ok(path) = std::env::var("PATH") else {
        return false;
    };
    for dir in std::env::split_paths(&path) {
        if dir.join(bin).is_file() {
            return true;
        }
    }
    false
}

fn opencode_config_candidates(cwd: Option<&Path>) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(h) = home() {
        out.push(h.join(".config/opencode/opencode.json"));
        out.push(h.join(".config/opencode/opencode.jsonc"));
    }
    if let Some(c) = cwd {
        out.push(c.join("opencode.json"));
        out.push(c.join("opencode.jsonc"));
    }
    out
}

fn strip_jsonc_line_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for line in text.lines() {
        let t = line.trim_start();
        if t.starts_with("//") {
            out.push('\n');
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

fn models_from_opencode_json(
    v: &serde_json::Value,
    provider: &str,
    base_url: Option<String>,
) -> Vec<DiscoveredModel> {
    let mut out = Vec::new();
    if let Some(m) = v.get("model").and_then(|x| x.as_str()) {
        // model is often "provider/id"
        if let Some((p, id)) = m.split_once('/') {
            if p == provider || provider == "opencode" {
                out.push(DiscoveredModel {
                    id: if provider == "opencode" {
                        m.to_string()
                    } else {
                        id.to_string()
                    },
                    display_name: None,
                    provider_id: provider.into(),
                    source: ModelListSource::ConfigFile,
                    base_url: base_url.clone(),
                });
            }
        } else if provider == "opencode" {
            out.push(DiscoveredModel {
                id: m.to_string(),
                display_name: None,
                provider_id: provider.into(),
                source: ModelListSource::ConfigFile,
                base_url: base_url.clone(),
            });
        }
    }
    if let Some(providers) = v.get("provider").and_then(|p| p.as_object()) {
        for (pid, pval) in providers {
            if provider != "opencode" && pid != provider {
                continue;
            }
            if let Some(models) = pval.get("models").and_then(|m| m.as_object()) {
                for mid in models.keys() {
                    out.push(DiscoveredModel {
                        id: if provider == "opencode" {
                            format!("{pid}/{mid}")
                        } else {
                            mid.clone()
                        },
                        display_name: None,
                        provider_id: provider.into(),
                        source: ModelListSource::ConfigFile,
                        base_url: base_url.clone(),
                    });
                }
            }
        }
    }
    out
}

fn models_from_grok_config_toml(text: &str, provider: &str) -> Vec<String> {
    // Match lines like: model = "google/gemini-2.5-pro" or [model.foo] with provider
    let mut out = Vec::new();
    for line in text.lines() {
        let t = line.trim();
        if let Some(rest) = t
            .strip_prefix("model")
            .and_then(|r| r.trim_start().strip_prefix('='))
        {
            let val = rest.trim().trim_matches('"').trim_matches('\'');
            if val.is_empty() {
                continue;
            }
            if let Some((p, id)) = val.split_once('/') {
                if p == provider || provider == "opencode" {
                    out.push(if provider == "opencode" {
                        val.to_string()
                    } else {
                        id.to_string()
                    });
                }
            }
        }
    }
    out
}

fn collect_model_strings_in_json(v: &serde_json::Value) -> Vec<String> {
    let mut out = Vec::new();
    fn walk(v: &serde_json::Value, out: &mut Vec<String>) {
        match v {
            serde_json::Value::Object(map) => {
                for (k, val) in map {
                    let lk = k.to_ascii_lowercase();
                    if lk.contains("model") {
                        if let Some(s) = val.as_str() {
                            if looks_like_model_id(s) {
                                out.push(s.to_string());
                            }
                        } else if let Some(arr) = val.as_array() {
                            for item in arr {
                                if let Some(s) = item.as_str() {
                                    if looks_like_model_id(s) {
                                        out.push(s.to_string());
                                    }
                                } else if let Some(id) =
                                    item.get("id").and_then(|x| x.as_str())
                                {
                                    out.push(id.to_string());
                                }
                            }
                        }
                    }
                    walk(val, out);
                }
            }
            serde_json::Value::Array(arr) => {
                for item in arr {
                    walk(item, out);
                }
            }
            _ => {}
        }
    }
    walk(v, &mut out);
    out
}

fn looks_like_model_id(s: &str) -> bool {
    let s = s.trim();
    if s.len() < 5 || s.len() > 96 {
        return false;
    }
    // Reject paths, URLs, and shell noise from chat history scrapes.
    if s.starts_with('/') || s.starts_with('.') || s.contains("://") || s.contains("..") {
        return false;
    }
    if s.contains(' ') {
        return false;
    }
    // Must look like a product model slug, not a random path segment.
    let lower = s.to_ascii_lowercase();
    let has_vendor_prefix = lower.starts_with("gpt")
        || lower.starts_with("o1")
        || lower.starts_with("o3")
        || lower.starts_with("o4")
        || lower.starts_with("claude")
        || lower.starts_with("gemini")
        || lower.starts_with("gemma")
        || lower.starts_with("grok")
        || lower.starts_with("llama")
        || lower.starts_with("mistral")
        || lower.starts_with("codestral")
        || lower.starts_with("deepseek")
        || lower.starts_with("qwen")
        || lower.starts_with("minimax")
        || lower.starts_with("chatgpt")
        || lower.starts_with("gpt-oss")
        || lower.starts_with("openai/")
        || lower.starts_with("anthropic/")
        || lower.starts_with("google/")
        || lower.starts_with("meta-llama/")
        || lower.starts_with("x-ai/")
        || lower.contains("gpt-oss");
    if !has_vendor_prefix {
        return false;
    }
    // Prefer ids with a digit or known separator (filters bare words like "logout").
    let has_structure = s.chars().any(|c| c.is_ascii_digit())
        || s.contains('-')
        || s.contains('.')
        || s.contains('/')
        || s.contains('@');
    has_structure
}

fn scrape_model_ids_from_text(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | '/' | '@') {
            cur.push(ch);
        } else if !cur.is_empty() {
            if looks_like_model_id(&cur) {
                out.push(cur.clone());
            }
            cur.clear();
        }
    }
    if looks_like_model_id(&cur) {
        out.push(cur);
    }
    let mut seen = std::collections::BTreeSet::new();
    out.retain(|id| seen.insert(id.to_ascii_lowercase()));
    out
}

/// Discover models for every item in a install report that has a base_url or known provider.
pub fn discover_models_for_found(
    items: &[super::discover::FoundItem],
    offline: bool,
) -> Vec<ModelDiscoveryReport> {
    let mut out = Vec::new();
    let mut seen_providers = std::collections::BTreeSet::new();
    for it in items {
        let Some(pid) = it.provider_id.as_deref() else {
            continue;
        };
        if !seen_providers.insert(pid.to_string()) {
            continue;
        }
        let mut req = ModelDiscoveryRequest::for_provider(pid);
        if let Some(url) = &it.base_url {
            req.base_url = Some(url.clone());
        }
        if offline {
            req = req.offline_only();
        }
        // Prefer env key when present for this provider
        if it.credential_present {
            if let Some(key) = env_key_for_provider(pid) {
                req.api_key = Some(key);
            }
        }
        out.push(discover_models(&req));
    }
    out
}

fn env_key_for_provider(provider: &str) -> Option<String> {
    let vars: &[&str] = match provider {
        "openai" => &["OPENAI_API_KEY"],
        "anthropic" => &["ANTHROPIC_API_KEY", "ANTHROPIC_AUTH_TOKEN"],
        "google" | "antigravity" => &["GEMINI_API_KEY", "GOOGLE_API_KEY"],
        "xai" => &["XAI_API_KEY", "GROK_CODE_XAI_API_KEY"],
        "openrouter" => &["OPENROUTER_API_KEY"],
        "groq" => &["GROQ_API_KEY"],
        "mistral" => &["MISTRAL_API_KEY"],
        "deepseek" => &["DEEPSEEK_API_KEY"],
        "minimax" => &["MINIMAX_API_KEY"],
        "togetherai" => &["TOGETHER_API_KEY"],
        _ => return None,
    };
    for v in vars {
        if let Ok(val) = std::env::var(v) {
            if !val.is_empty() {
                return Some(val);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_agy_models_all_lines() {
        let text = "\
Gemini 3.5 Flash (Medium)
Gemini 3.5 Flash (High)
Gemini 3.1 Pro (Low)
Claude Sonnet 4.6 (Thinking)
GPT-OSS 120B (Medium)
";
        let rows = parse_agy_models_output(text);
        assert_eq!(rows.len(), 5);
        assert!(rows.iter().any(|(id, d)| {
            id.contains("gemini-3.5-flash") && d.as_deref() == Some("Gemini 3.5 Flash (Medium)")
        }));
        assert!(rows.iter().any(|(id, _)| id.contains("claude-sonnet")));
        assert!(rows.iter().any(|(id, _)| id.contains("gpt-oss")));
    }

    #[test]
    fn display_name_slug() {
        assert_eq!(
            display_name_to_model_id("Gemini 3.5 Flash (Medium)"),
            "gemini-3.5-flash-medium"
        );
    }

    #[test]
    fn extract_openai_and_ollama_shapes() {
        let oai = serde_json::json!({"data":[{"id":"gpt-4o"},{"id":"o3"}]});
        assert_eq!(extract_model_ids(&oai), vec!["gpt-4o", "o3"]);
        let ol = serde_json::json!({"models":[{"name":"llama3.2:latest"}]});
        assert_eq!(extract_model_ids(&ol), vec!["llama3.2:latest"]);
    }

    #[test]
    fn recent_fallback_when_offline_empty() {
        let req = ModelDiscoveryRequest::for_provider("openai").offline_only();
        // force skip catalog so we hit recent (catalog usually has openai)
        let mut req = req;
        req.skip_catalog = true;
        let rep = discover_models(&req);
        assert!(!rep.models.is_empty());
        assert_eq!(rep.primary_source, ModelListSource::RecentFallback);
        assert!(rep.ids().iter().any(|i| i.contains("gpt")));
    }

    #[test]
    fn catalog_or_recent_for_google_offline() {
        let req = ModelDiscoveryRequest::for_provider("google").offline_only();
        let rep = discover_models(&req);
        assert!(!rep.models.is_empty(), "notes={:?}", rep.notes);
        // May be catalog, config, history, or recent depending on host files.
        assert!(
            matches!(
                rep.primary_source,
                ModelListSource::Catalog
                    | ModelListSource::RecentFallback
                    | ModelListSource::ConfigFile
                    | ModelListSource::History
                    | ModelListSource::ProviderCli
            ),
            "primary={:?}",
            rep.primary_source
        );
    }

    #[test]
    fn opencode_config_models_discovered() {
        let dir = std::env::temp_dir().join(format!("md-oc-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(
            dir.join("opencode.json"),
            r#"{
              "model": "ollama/llama3.2",
              "provider": {
                "ollama": {
                  "models": { "llama3.2": {}, "codellama": {} }
                }
              }
            }"#,
        )
        .unwrap();
        let mut req = ModelDiscoveryRequest::for_provider("ollama").offline_only();
        req.skip_catalog = true;
        req.skip_recent_fallback = true;
        req.cwd = Some(dir.clone());
        let rep = discover_models(&req);
        assert!(
            rep.ids().iter().any(|i| i.contains("llama")),
            "ids={:?} notes={:?}",
            rep.ids(),
            rep.notes
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn merge_prefers_live_over_fallback() {
        let mut map = BTreeMap::new();
        insert_model(
            &mut map,
            DiscoveredModel {
                id: "gpt-4o".into(),
                display_name: None,
                provider_id: "openai".into(),
                source: ModelListSource::RecentFallback,
                base_url: None,
            },
        );
        insert_model(
            &mut map,
            DiscoveredModel {
                id: "gpt-4o".into(),
                display_name: Some("GPT-4o".into()),
                provider_id: "openai".into(),
                source: ModelListSource::LiveEndpoint,
                base_url: Some("https://api.openai.com/v1".into()),
            },
        );
        assert_eq!(map["gpt-4o"].source, ModelListSource::LiveEndpoint);
        assert_eq!(map["gpt-4o"].display_name.as_deref(), Some("GPT-4o"));
    }

    #[test]
    fn normalize_agy_aliases() {
        assert_eq!(normalize_provider_id("agy"), "antigravity");
        assert_eq!(normalize_provider_id("GEMINI"), "antigravity");
        assert_eq!(normalize_provider_id("OpenAI"), "openai");
    }

    #[test]
    fn summary_includes_source() {
        let rep = ModelDiscoveryReport {
            provider_id: "x".into(),
            base_url: None,
            models: vec![DiscoveredModel {
                id: "m1".into(),
                display_name: Some("M One".into()),
                provider_id: "x".into(),
                source: ModelListSource::Catalog,
                base_url: None,
            }],
            primary_source: ModelListSource::Catalog,
            sources_tried: vec!["catalog".into()],
            notes: vec![],
        };
        let t = rep.summary_text();
        assert!(t.contains("m1"));
        assert!(t.contains("catalog"));
    }

    #[test]
    fn cache_roundtrip() {
        let dir = std::env::temp_dir().join(format!("agy-cache-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = cache_discovered_models(
            &dir,
            &[
                ("gemini-3.5-flash-medium".into(), Some("Gemini 3.5 Flash (Medium)".into())),
            ],
        )
        .unwrap();
        assert!(path.is_file());
        let mut req = ModelDiscoveryRequest::for_provider("antigravity").offline_only();
        req.skip_catalog = true;
        req.skip_recent_fallback = true;
        req.gemini_home = Some(dir.clone());
        let rep = discover_models(&req);
        assert!(
            rep.ids().iter().any(|i| i.contains("gemini-3.5")),
            "{:?}",
            rep.ids()
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn recent_covers_common_providers() {
        for p in [
            "openai", "anthropic", "google", "xai", "ollama", "groq", "mistral",
        ] {
            assert!(
                !recent_models_for_provider(p).is_empty(),
                "missing recent for {p}"
            );
        }
    }

    #[test]
    fn history_scrape_rejects_urls_and_paths() {
        let junk = r#"
            check //wallhaven.cc/user/foo and /home/mlapointe/git/wf-shell
            and Be/Haiku plus logout/poweroff then gemini-3.5-flash-medium
            and claude-sonnet-4.6-thinking and gpt-oss-120b-medium
        "#;
        let ids = scrape_model_ids_from_text(junk);
        assert!(ids.iter().all(|i| !i.contains("wallhaven")));
        assert!(ids.iter().all(|i| !i.starts_with('/')));
        assert!(ids.iter().any(|i| i.contains("gemini-3.5")));
        assert!(ids.iter().any(|i| i.contains("claude")));
        assert!(ids.iter().any(|i| i.contains("gpt-oss")));
    }
}
