//! Optional **live** multi-provider auth tests.
//!
//! Credentials are **never** committed. When the operator has real keys in the
//! environment (or in `~/.grok/config.toml` via `api_key`), these exercises hit
//! real services. Otherwise they skip cleanly so CI / random machines stay green.
//!
//! ```text
//! # Live LiteLLM (OpenAI-compat gateway):
//! export LITELLM_API_KEY='sk-…'          # or use config.toml api_key
//! export LITELLM_BASE_URL='https://litellm.example/v1'   # optional
//! export LITELLM_MODEL='minimaxm3'                       # optional
//!
//! # Live MiniMax (Anthropic-compat):
//! export MINIMAX_API_KEY='sk-…'
//! export MINIMAX_BASE_URL='https://api.minimax.io/anthropic/v1'  # optional
//! export MINIMAX_MODEL='MiniMax-M3'                              # optional
//!
//! cargo test -p xai-grok-shell --test test_live_provider_auth -- --ignored --nocapture
//! ```
//!
//! Also loads keys from `~/.grok/config.toml` when env is unset:
//! - first `[model.*]` with `base_url` containing `litellm` + non-empty `api_key`
//! - first `[model.*]` with `base_url` containing `minimax` + `api_backend = "messages"`

use std::path::PathBuf;
use std::time::Duration;

use xai_chat_state::AuthType;
use xai_grok_shell::agent::config::{
    resolve_credentials, resolve_model_list, Config, EndpointsConfig, ModelEntry,
};
use xai_grok_shell::providers::{validate_endpoint, HelloPolicy, ValidationStatus};
use xai_grok_shell::sampling::AuthScheme;

/// Synthetic JWT-shaped blob (must never be sent to third parties).
const FAKE_SESSION_JWT: &str = "eyJhbGciOiJSUzI1NiIsInR5cCI6IkpXVCJ9.e30.sig";

struct LiveEndpoint {
    name: String,
    base_url: String,
    api_key: String,
    model: String,
    api_backend: String,
    auth_scheme: AuthScheme,
}

fn env_nonempty(name: &str) -> Option<String> {
    std::env::var(name).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

fn operator_config_path() -> Option<PathBuf> {
    // Prefer GROK_HOME (tests / custom installs), else ~/.grok.
    if let Ok(gh) = std::env::var("GROK_HOME") {
        let p = PathBuf::from(gh).join("config.toml");
        if p.is_file() {
            return Some(p);
        }
    }
    dirs::home_dir()
        .map(|h| h.join(".grok/config.toml"))
        .filter(|p| p.is_file())
}

/// Prefer env; fall back to matching entries in the operator's config.toml.
fn load_live_from_config() -> (Option<LiveEndpoint>, Option<LiveEndpoint>) {
    let Some(path) = operator_config_path() else {
        eprintln!("live auth: no config.toml at GROK_HOME or ~/.grok");
        return (None, None);
    };
    let Ok(text) = std::fs::read_to_string(&path) else {
        eprintln!("live auth: cannot read {}", path.display());
        return (None, None);
    };
    let raw: toml::Value = match toml::from_str(&text) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("live auth: toml parse failed for {}: {e}", path.display());
            return (None, None);
        }
    };
    let cfg = match Config::new_from_toml_cfg(&raw) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("live auth: Config::new_from_toml_cfg failed: {e}");
            return (None, None);
        }
    };
    let resolved = resolve_model_list(&cfg, None);
    eprintln!(
        "live auth: loaded {} models from {}",
        resolved.len(),
        path.display()
    );

    let mut litellm = None;
    let mut minimax = None;
    for (key, entry) in &resolved {
        // Resolve with a fake session JWT so we only keep models that win via
        // api_key/env_key (third-party must not fall through to session).
        let creds = resolve_credentials(entry, Some(FAKE_SESSION_JWT));
        let Some(api_key) = creds.api_key.filter(|k| !k.starts_with("eyJ")) else {
            continue;
        };
        let base = entry.info.base_url.clone();
        let lower = base.to_ascii_lowercase();
        if litellm.is_none()
            && (lower.contains("litellm") || key.contains("litellm") || key.contains("gateway"))
            && !matches!(
                entry.info.api_backend,
                xai_grok_shell::sampling::ApiBackend::Messages
            )
        {
            litellm = Some(LiveEndpoint {
                name: key.clone(),
                base_url: base.clone(),
                api_key: api_key.clone(),
                model: entry.info.model.clone(),
                api_backend: "chat_completions".into(),
                auth_scheme: AuthScheme::Bearer,
            });
        }
        if minimax.is_none()
            && (lower.contains("minimax") || key.contains("minimax-direct"))
            && matches!(
                entry.info.api_backend,
                xai_grok_shell::sampling::ApiBackend::Messages
            )
        {
            minimax = Some(LiveEndpoint {
                name: key.clone(),
                base_url: base,
                api_key,
                model: entry.info.model.clone(),
                api_backend: "messages".into(),
                auth_scheme: AuthScheme::XApiKey,
            });
        }
    }
    (litellm, minimax)
}

fn litellm_live() -> Option<LiveEndpoint> {
    if let Some(key) = env_nonempty("LITELLM_API_KEY") {
        return Some(LiveEndpoint {
            name: "env:LITELLM".into(),
            base_url: env_nonempty("LITELLM_BASE_URL")
                .unwrap_or_else(|| "https://litellm.cloudbsd.org/v1".into()),
            api_key: key,
            model: env_nonempty("LITELLM_MODEL").unwrap_or_else(|| "minimaxm3".into()),
            api_backend: "chat_completions".into(),
            auth_scheme: AuthScheme::Bearer,
        });
    }
    load_live_from_config().0
}

fn minimax_live() -> Option<LiveEndpoint> {
    if let Some(key) = env_nonempty("MINIMAX_API_KEY") {
        return Some(LiveEndpoint {
            name: "env:MINIMAX".into(),
            base_url: env_nonempty("MINIMAX_BASE_URL")
                .unwrap_or_else(|| "https://api.minimax.io/anthropic/v1".into()),
            api_key: key,
            model: env_nonempty("MINIMAX_MODEL").unwrap_or_else(|| "MiniMax-M3".into()),
            api_backend: "messages".into(),
            auth_scheme: AuthScheme::XApiKey,
        });
    }
    load_live_from_config().1
}

fn entry_for(ep: &LiveEndpoint) -> ModelEntry {
    let endpoints = EndpointsConfig::default();
    let mut e = ModelEntry::fallback(&ep.name, &endpoints);
    e.info.model = ep.model.clone();
    e.info.base_url = ep.base_url.clone();
    e.info.auth_scheme = ep.auth_scheme;
    e.api_key = Some(ep.api_key.clone());
    e
}

fn assert_third_party_prefers_model_key(ep: &LiveEndpoint) {
    let entry = entry_for(ep);
    let creds = resolve_credentials(&entry, Some(FAKE_SESSION_JWT));
    assert_eq!(creds.auth_type, AuthType::ApiKey);
    assert_eq!(
        creds.api_key.as_deref(),
        Some(ep.api_key.as_str()),
        "model api_key must win over session JWT for {}",
        ep.base_url
    );
    assert!(
        !creds.api_key.as_deref().unwrap_or("").starts_with("eyJ"),
        "must not send JWT to third-party host"
    );
    assert!(
        creds.api_key.as_deref().unwrap_or("").starts_with("sk-")
            || creds.api_key.as_ref().is_some_and(|k| k.len() > 8),
        "expected a real virtual/api key, not empty"
    );
}

fn live_validate(ep: &LiveEndpoint) {
    let report = validate_endpoint(
        &ep.base_url,
        Some(ep.api_key.as_str()),
        ep.auth_scheme,
        &ep.api_backend,
        &HelloPolicy {
            enabled: true,
            preferred_model: Some(ep.model.clone()),
            max_tokens: 64,
            timeout: Duration::from_secs(90),
        },
    );
    assert_eq!(
        report.status,
        ValidationStatus::Ready,
        "live validate failed for {} ({}): {:?}",
        ep.name,
        ep.base_url,
        report.message
    );
    assert!(
        report.did_hello,
        "expected L2 hello for {} ({})",
        ep.name,
        ep.base_url
    );
}

/// Always runs (no secrets): third-party host never gets the session JWT.
#[test]
fn offline_third_party_never_uses_session_jwt() {
    let endpoints = EndpointsConfig::default();
    let mut entry = ModelEntry::fallback("litellm-demo", &endpoints);
    entry.info.model = "minimaxm3".into();
    entry.info.base_url = "https://litellm.example.test/v1".into();
    entry.api_key = Some("sk-from-config".into());
    let creds = resolve_credentials(&entry, Some(FAKE_SESSION_JWT));
    assert_eq!(creds.api_key.as_deref(), Some("sk-from-config"));

    entry.api_key = None;
    let creds = resolve_credentials(&entry, Some(FAKE_SESSION_JWT));
    assert!(creds.api_key.is_none(), "no JWT leak without model key");
}

#[test]
#[ignore = "live: needs LITELLM_API_KEY or config.toml litellm api_key"]
fn live_litellm_uses_config_or_env_key_not_session_jwt() {
    let Some(ep) = litellm_live() else {
        eprintln!(
            "SKIP live_litellm: set LITELLM_API_KEY or put api_key on a litellm [model.*] in ~/.grok/config.toml"
        );
        return;
    };
    eprintln!("live litellm via {} model={}", ep.name, ep.model);
    assert_third_party_prefers_model_key(&ep);
    live_validate(&ep);
}

#[test]
#[ignore = "live: needs MINIMAX_API_KEY or config.toml minimax-direct api_key"]
fn live_minimax_uses_config_or_env_key_not_session_jwt() {
    let Some(ep) = minimax_live() else {
        eprintln!(
            "SKIP live_minimax: set MINIMAX_API_KEY or put api_key on a minimax-direct [model.*] in ~/.grok/config.toml"
        );
        return;
    };
    eprintln!("live minimax via {} model={}", ep.name, ep.model);
    assert_third_party_prefers_model_key(&ep);
    live_validate(&ep);
}

#[test]
#[ignore = "live: needs both LiteLLM and MiniMax credentials"]
fn live_both_gateways() {
    let lt = litellm_live();
    let mm = minimax_live();
    if lt.is_none() && mm.is_none() {
        eprintln!(
            "SKIP live_both: set LITELLM_API_KEY / MINIMAX_API_KEY or api_key in ~/.grok/config.toml"
        );
        return;
    }
    if let Some(ep) = lt {
        assert_third_party_prefers_model_key(&ep);
        live_validate(&ep);
    }
    if let Some(ep) = mm {
        assert_third_party_prefers_model_key(&ep);
        live_validate(&ep);
    }
}

#[allow(dead_code)]
fn _home_config_path() -> PathBuf {
    dirs::home_dir().unwrap_or_default().join(".grok/config.toml")
}
