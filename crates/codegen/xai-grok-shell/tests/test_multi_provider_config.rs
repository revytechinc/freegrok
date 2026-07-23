//! Integration: multi-provider catalog config (OpenAI-compat gateway +
//! Anthropic-compat MiniMax-style + local OpenAI-compat) loads offline.
//!
//! **Portable / Joe-safe:** no CloudBSD hosts, no live Ollama, no paid APIs.
//! Wire validation uses [`MockInferenceServer`] on loopback only.

use serial_test::serial;
use xai_grok_shell::agent::auth_method::{LEGACY_XAI_API_KEY_ENV_VAR, XAI_API_KEY_ENV_VAR};
use xai_grok_shell::agent::config::{resolve_model_list, Config};
use xai_grok_shell::cli_models::AuthStatus;
use xai_grok_shell::providers::{validate_endpoint, HelloPolicy, ValidationLevel, ValidationStatus};
use xai_grok_shell::sampling::{ApiBackend, AuthScheme};
use xai_grok_test_support::{EnvGuard, MockInferenceServer, MockModelEntry};

/// Synthetic multi-provider catalog — hostnames are RFC 2606 / test-only.
const MULTI_PROVIDER_TOML: &str = r#"
[cli]
installer = "internal"

[ui]
yolo = false

[model.minimax-direct-m3]
model = "MiniMax-M3"
name = "MiniMax M3 (direct)"
base_url = "https://minimax.example.test/anthropic/v1"
api_backend = "messages"
auth_scheme = "x_api_key"
env_key = "MINIMAX_API_KEY"
context_window = 204800
extra_headers = { "anthropic-version" = "2023-06-01" }

[model.minimax-direct-m2-5]
model = "MiniMax-M2.5"
name = "MiniMax M2.5 (direct)"
base_url = "https://minimax.example.test/anthropic/v1"
api_backend = "messages"
auth_scheme = "x_api_key"
env_key = "MINIMAX_API_KEY"
context_window = 204800
extra_headers = { "anthropic-version" = "2023-06-01" }

[model.gateway-minimaxm3]
model = "minimaxm3"
name = "minimaxm3 (OpenAI-compat gateway)"
base_url = "https://llm-gateway.example.test/v1"
env_key = "LLM_GATEWAY_API_KEY"

[model.gateway-ollama-qwen]
model = "ollama-qwen3.5-cloud"
name = "ollama-qwen3.5-cloud (gateway)"
base_url = "https://llm-gateway.example.test/v1"
env_key = "LLM_GATEWAY_API_KEY"

[model.gateway-glm-colon]
model = "glm-5.2:cloud"
name = "glm with colon in model id (gateway)"
base_url = "https://llm-gateway.example.test/v1"
env_key = "LLM_GATEWAY_API_KEY"

[model.local-openai-compat]
model = "local-coder"
name = "Local OpenAI-compat (no key)"
base_url = "http://127.0.0.1:9/v1"

[model.local-thinking]
model = "thinking-cloud"
name = "Local thinking model id"
base_url = "http://127.0.0.1:9/v1"
"#;

fn isolate_auth_sources() -> (tempfile::TempDir, [EnvGuard; 7]) {
    let dir = tempfile::tempdir().unwrap();
    let auth_path = dir.path().join("no-auth.json");
    let guards = [
        EnvGuard::unset(XAI_API_KEY_ENV_VAR),
        EnvGuard::unset(LEGACY_XAI_API_KEY_ENV_VAR),
        EnvGuard::unset("GROK_AUTH"),
        EnvGuard::set("GROK_AUTH_PATH", auth_path.to_str().unwrap()),
        EnvGuard::unset("GROK_DEPLOYMENT_KEY"),
        EnvGuard::unset("GROK_WS_ORIGIN"),
        EnvGuard::unset("GROK_DISABLE_API_KEY_AUTH"),
    ];
    (dir, guards)
}

fn config_from_toml(toml_str: &str) -> Config {
    let raw: toml::Value = toml::from_str(toml_str).expect("toml parse");
    Config::new_from_toml_cfg(&raw).expect("config load")
}

#[test]
fn multi_provider_toml_is_valid_and_has_no_secrets() {
    let _: toml::Value = toml::from_str(MULTI_PROVIDER_TOML).unwrap();
    assert!(
        !MULTI_PROVIDER_TOML.contains("sk-"),
        "fixture must not embed API key material"
    );
    assert!(
        !MULTI_PROVIDER_TOML.contains("cloudbsd"),
        "fixtures must not hardcode operator infra hostnames"
    );
    assert!(MULTI_PROVIDER_TOML.contains("env_key"));
    assert!(MULTI_PROVIDER_TOML.contains("MINIMAX_API_KEY"));
    assert!(MULTI_PROVIDER_TOML.contains("LLM_GATEWAY_API_KEY"));
    assert!(MULTI_PROVIDER_TOML.contains("example.test"));
}

#[test]
fn multi_provider_catalog_loads_all_entries() {
    let cfg = config_from_toml(MULTI_PROVIDER_TOML);
    assert!(
        cfg.config_warnings.is_empty(),
        "warnings: {:?}",
        cfg.config_warnings
    );
    for key in [
        "minimax-direct-m3",
        "minimax-direct-m2-5",
        "gateway-minimaxm3",
        "gateway-ollama-qwen",
        "gateway-glm-colon",
        "local-openai-compat",
        "local-thinking",
    ] {
        assert!(
            cfg.config_models.contains_key(key),
            "missing model entry {key}"
        );
    }
    assert_eq!(cfg.config_models.len(), 7);

    let mm = cfg.config_models.get("minimax-direct-m3").unwrap();
    assert_eq!(mm.api_backend, Some(ApiBackend::Messages));
    assert_eq!(mm.auth_scheme, Some(AuthScheme::XApiKey));
    assert_eq!(
        mm.env_key.as_ref().and_then(|k| k.primary()),
        Some("MINIMAX_API_KEY")
    );
    assert_eq!(
        mm.extra_headers
            .get("anthropic-version")
            .map(|s| s.as_str()),
        Some("2023-06-01")
    );

    let gw = cfg.config_models.get("gateway-minimaxm3").unwrap();
    assert_eq!(
        gw.base_url.as_deref(),
        Some("https://llm-gateway.example.test/v1")
    );
    assert_eq!(
        gw.env_key.as_ref().and_then(|k| k.primary()),
        Some("LLM_GATEWAY_API_KEY")
    );

    let colon = cfg.config_models.get("gateway-glm-colon").unwrap();
    assert_eq!(colon.model.as_deref(), Some("glm-5.2:cloud"));

    let local = cfg.config_models.get("local-openai-compat").unwrap();
    assert!(local.env_key.is_none());
    assert_eq!(local.base_url.as_deref(), Some("http://127.0.0.1:9/v1"));
}

#[test]
fn multi_provider_resolve_model_list_includes_custom_entries() {
    let cfg = config_from_toml(MULTI_PROVIDER_TOML);
    let resolved = resolve_model_list(&cfg, None);
    assert!(
        resolved.contains_key("minimax-direct-m3"),
        "resolved keys sample: {:?}",
        resolved.keys().take(20).collect::<Vec<_>>()
    );
    assert!(resolved.contains_key("gateway-minimaxm3"));
    assert!(resolved.contains_key("local-openai-compat"));
    let entry = resolved.get("minimax-direct-m3").unwrap();
    assert_eq!(entry.info.model, "MiniMax-M3");
    assert_eq!(entry.info.api_backend, ApiBackend::Messages);
    assert_eq!(
        entry.info.base_url,
        "https://minimax.example.test/anthropic/v1"
    );
}

#[test]
#[serial]
fn multi_provider_env_key_auth_status() {
    let (_dir, _g) = isolate_auth_sources();
    let _mm_unset = EnvGuard::unset("MINIMAX_API_KEY");
    let _gw_unset = EnvGuard::unset("LLM_GATEWAY_API_KEY");

    let cfg = config_from_toml(MULTI_PROVIDER_TOML);

    assert_eq!(
        AuthStatus::resolve(&cfg),
        AuthStatus::NotAuthenticated,
        "without env keys, multi-provider BYOK must not claim auth"
    );

    {
        let _mm = EnvGuard::set("MINIMAX_API_KEY", "test-minimax-token");
        match AuthStatus::resolve(&cfg) {
            AuthStatus::ModelCredentials(key) => {
                assert!(
                    key.contains("minimax"),
                    "expected minimax catalog key, got {key}"
                );
            }
            other => panic!("expected ModelCredentials with MINIMAX_API_KEY set, got {other:?}"),
        }
    }

    {
        let _mm = EnvGuard::unset("MINIMAX_API_KEY");
        let _gw = EnvGuard::set("LLM_GATEWAY_API_KEY", "test-gateway-token");
        match AuthStatus::resolve(&cfg) {
            AuthStatus::ModelCredentials(key) => {
                assert!(
                    key.contains("gateway") || key.contains("minimax") || key.contains("glm"),
                    "expected gateway-backed catalog key, got {key}"
                );
            }
            other => {
                panic!("expected ModelCredentials with LLM_GATEWAY_API_KEY set, got {other:?}")
            }
        }
    }
}

/// Loopback mock only — works offline on any machine with free ports.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mock_openai_gateway_validate_list_and_hello() {
    let server = MockInferenceServer::start_with_models(vec![
        MockModelEntry::new("minimaxm3"),
        MockModelEntry::new("ollama-qwen3.5-cloud"),
        MockModelEntry::new("thinking-cloud"),
    ])
    .await
    .expect("start mock");

    // MockInferenceServer::url() is already `http://127.0.0.1:PORT/v1`.
    let base = server.url();

    let base_for_list = base.clone();
    let list_only = tokio::task::spawn_blocking(move || {
        validate_endpoint(
            &base_for_list,
            Some("mock-token"),
            AuthScheme::Bearer,
            "chat_completions",
            &HelloPolicy {
                enabled: true,
                preferred_model: None,
                ..Default::default()
            },
        )
    })
    .await
    .unwrap();

    assert_eq!(list_only.status, ValidationStatus::Ready);
    assert_eq!(list_only.highest_level, ValidationLevel::L1ModelsList);
    assert!(
        !list_only.did_hello,
        "without --model, chat_completions skips L2"
    );
    assert!(list_only.models_found.unwrap_or(0) >= 3);

    let base_for_hello = base.clone();
    let with_model = tokio::task::spawn_blocking(move || {
        validate_endpoint(
            &base_for_hello,
            Some("mock-token"),
            AuthScheme::Bearer,
            "chat_completions",
            &HelloPolicy {
                enabled: true,
                preferred_model: Some("minimaxm3".into()),
                max_tokens: 16,
                ..Default::default()
            },
        )
    })
    .await
    .unwrap();

    assert_eq!(with_model.status, ValidationStatus::Ready);
    assert_eq!(with_model.highest_level, ValidationLevel::L2Hello);
    assert!(
        with_model.did_hello,
        "preferred_model must force L2 hello against mock"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mock_anthropic_messages_validate_hello() {
    let server = MockInferenceServer::start_with_models(vec![
        MockModelEntry::new("MiniMax-M3").with_api_backend("messages"),
    ])
    .await
    .expect("start mock");

    let base = server.url();
    let base_clone = base.clone();
    let report = tokio::task::spawn_blocking(move || {
        validate_endpoint(
            &base_clone,
            Some("mock-minimax-key"),
            AuthScheme::XApiKey,
            "messages",
            &HelloPolicy {
                enabled: true,
                preferred_model: Some("MiniMax-M3".into()),
                max_tokens: 32,
                ..Default::default()
            },
        )
    })
    .await
    .unwrap();

    assert_eq!(report.status, ValidationStatus::Ready);
    assert!(report.did_hello);
    assert_eq!(report.highest_level, ValidationLevel::L2Hello);
}
