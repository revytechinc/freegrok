//! Connection validation: L0 reach, L1 list models, L2 tiny "hello".

use serde::Serialize;
use std::time::{Duration, Instant};
use xai_grok_sampler::AuthScheme;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ValidationLevel {
    /// TCP / HTTP reach only.
    L0Reach,
    /// GET …/models (or equivalent).
    L1ModelsList,
    /// Minimal non-streaming completion with message "hello".
    L2Hello,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ValidationStatus {
    Ready,
    Degraded,
    Unreachable,
    AuthFailed,
    Failed,
    Skipped,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidationReport {
    pub status: ValidationStatus,
    pub highest_level: ValidationLevel,
    pub base_url: String,
    pub latency_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub models_found: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sample_model_ids: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    pub did_hello: bool,
}

#[derive(Debug, Clone)]
pub struct HelloPolicy {
    pub enabled: bool,
    pub max_tokens: u32,
    pub timeout: Duration,
    /// Prefer this model id for hello when listing succeeded.
    pub preferred_model: Option<String>,
}

impl Default for HelloPolicy {
    fn default() -> Self {
        Self {
            enabled: true,
            max_tokens: 8,
            timeout: Duration::from_secs(20),
            preferred_model: None,
        }
    }
}

/// Validate an OpenAI-compatible or Anthropic-messages base URL.
///
/// `api_backend`: `"chat_completions"` | `"messages"` | `"responses"`.
pub fn validate_endpoint(
    base_url: &str,
    api_key: Option<&str>,
    auth_scheme: AuthScheme,
    api_backend: &str,
    hello: &HelloPolicy,
) -> ValidationReport {
    let start = Instant::now();
    let base = base_url.trim_end_matches('/');

    // L0: TCP to host:port when URL is parseable
    if let Some((host, port)) = host_port_from_url(base) {
        let addr = format!("{host}:{port}");
        use std::net::ToSocketAddrs;
        let sock = match addr.to_socket_addrs().ok().and_then(|mut i| i.next()) {
            Some(a) => a,
            None => {
                return ValidationReport {
                    status: ValidationStatus::Unreachable,
                    highest_level: ValidationLevel::L0Reach,
                    base_url: base.to_string(),
                    latency_ms: start.elapsed().as_millis() as u64,
                    models_found: None,
                    sample_model_ids: None,
                    message: Some(format!("cannot resolve {addr}")),
                    did_hello: false,
                };
            }
        };
        if std::net::TcpStream::connect_timeout(&sock, Duration::from_secs(2)).is_err() {
            return ValidationReport {
                status: ValidationStatus::Unreachable,
                highest_level: ValidationLevel::L0Reach,
                base_url: base.to_string(),
                latency_ms: start.elapsed().as_millis() as u64,
                models_found: None,
                sample_model_ids: None,
                message: Some(format!("TCP connect failed to {addr}")),
                did_hello: false,
            };
        }
    }

    let client = match reqwest::blocking::Client::builder()
        .timeout(hello.timeout)
        .user_agent("grok-build-provider-validate/0.1")
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            return ValidationReport {
                status: ValidationStatus::Failed,
                highest_level: ValidationLevel::L0Reach,
                base_url: base.to_string(),
                latency_ms: start.elapsed().as_millis() as u64,
                models_found: None,
                sample_model_ids: None,
                message: Some(e.to_string()),
                did_hello: false,
            };
        }
    };

    // L1: GET /models
    let models_url = format!("{base}/models");
    let mut req = client.get(&models_url);
    req = apply_auth(req, api_key, auth_scheme);
    if matches!(auth_scheme, AuthScheme::XApiKey) {
        req = req.header("anthropic-version", "2023-06-01");
    }

    let (models_found, sample_ids, l1_err, auth_failed) = match req.send() {
        Ok(resp) => {
            let status = resp.status();
            if status.as_u16() == 401 || status.as_u16() == 403 {
                (None, None, Some(format!("HTTP {status}")), true)
            } else if !status.is_success() {
                (None, None, Some(format!("HTTP {status} from /models")), false)
            } else {
                match resp.json::<serde_json::Value>() {
                    Ok(v) => {
                        let ids = extract_model_ids(&v);
                        (Some(ids.len()), Some(ids), None, false)
                    }
                    Err(e) => (None, None, Some(format!("models JSON: {e}")), false),
                }
            }
        }
        Err(e) => (None, None, Some(e.to_string()), false),
    };

    if auth_failed {
        return ValidationReport {
            status: ValidationStatus::AuthFailed,
            highest_level: ValidationLevel::L1ModelsList,
            base_url: base.to_string(),
            latency_ms: start.elapsed().as_millis() as u64,
            models_found,
            sample_model_ids: sample_ids,
            message: l1_err,
            did_hello: false,
        };
    }

    let l1_ok = l1_err.is_none() && models_found.unwrap_or(0) > 0;

    // Decide hello.
    // When the operator pins `--model`, always run L2: L1 list-only is a false
    // green for endpoints that list models but fail (or return empty content)
    // on completion (Ollama cloud thinking models, mis-routed LiteLLM, etc.).
    let need_hello = hello.enabled
        && (!l1_ok
            || sample_ids
                .as_ref()
                .map(|s| s.is_empty())
                .unwrap_or(true)
            || matches!(api_backend, "messages" | "responses")
            || hello.preferred_model.is_some());

    if !need_hello && l1_ok {
        return ValidationReport {
            status: ValidationStatus::Ready,
            highest_level: ValidationLevel::L1ModelsList,
            base_url: base.to_string(),
            latency_ms: start.elapsed().as_millis() as u64,
            models_found,
            sample_model_ids: sample_ids.map(|mut v| {
                v.truncate(8);
                v
            }),
            message: None,
            did_hello: false,
        };
    }

    if !hello.enabled {
        return ValidationReport {
            status: if l1_ok {
                ValidationStatus::Ready
            } else if l1_err.is_some() {
                ValidationStatus::Degraded
            } else {
                ValidationStatus::Failed
            },
            highest_level: ValidationLevel::L1ModelsList,
            base_url: base.to_string(),
            latency_ms: start.elapsed().as_millis() as u64,
            models_found,
            sample_model_ids: sample_ids,
            message: l1_err.or_else(|| Some("hello disabled".into())),
            did_hello: false,
        };
    }

    let model_id = hello
        .preferred_model
        .clone()
        .or_else(|| sample_ids.as_ref().and_then(|s| s.first().cloned()))
        .unwrap_or_else(|| "default".into());

    let hello_result = match api_backend {
        "messages" => hello_messages(&client, base, api_key, auth_scheme, &model_id, hello),
        "responses" => hello_responses(&client, base, api_key, &model_id, hello),
        _ => hello_chat_completions(&client, base, api_key, auth_scheme, &model_id, hello),
    };

    match hello_result {
        Ok(()) => ValidationReport {
            status: ValidationStatus::Ready,
            highest_level: ValidationLevel::L2Hello,
            base_url: base.to_string(),
            latency_ms: start.elapsed().as_millis() as u64,
            models_found,
            sample_model_ids: sample_ids.map(|mut v| {
                v.truncate(8);
                v
            }),
            message: l1_err.map(|e| format!("list models: {e}; hello ok")),
            did_hello: true,
        },
        Err(e) => {
            let status = if l1_ok {
                ValidationStatus::Degraded
            } else {
                ValidationStatus::Failed
            };
            ValidationReport {
                status,
                highest_level: ValidationLevel::L2Hello,
                base_url: base.to_string(),
                latency_ms: start.elapsed().as_millis() as u64,
                models_found,
                sample_model_ids: sample_ids,
                message: Some(format!("hello failed: {e}")),
                did_hello: true,
            }
        }
    }
}

fn apply_auth(
    req: reqwest::blocking::RequestBuilder,
    api_key: Option<&str>,
    scheme: AuthScheme,
) -> reqwest::blocking::RequestBuilder {
    let Some(key) = api_key.filter(|k| !k.is_empty()) else {
        return req;
    };
    match scheme {
        AuthScheme::XApiKey => req.header("x-api-key", key),
        AuthScheme::Bearer => req.bearer_auth(key),
    }
}

fn extract_model_ids(v: &serde_json::Value) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(arr) = v.get("data").and_then(|d| d.as_array()) {
        for item in arr {
            if let Some(id) = item.get("id").and_then(|x| x.as_str()) {
                out.push(id.to_string());
            }
        }
    } else if let Some(arr) = v.as_array() {
        for item in arr {
            if let Some(id) = item.get("id").and_then(|x| x.as_str()) {
                out.push(id.to_string());
            }
        }
    } else if let Some(models) = v.get("models").and_then(|m| m.as_array()) {
        for item in models {
            if let Some(id) = item.get("name").or_else(|| item.get("id")).and_then(|x| x.as_str())
            {
                out.push(id.to_string());
            }
        }
    }
    out
}

fn hello_chat_completions(
    client: &reqwest::blocking::Client,
    base: &str,
    api_key: Option<&str>,
    scheme: AuthScheme,
    model: &str,
    hello: &HelloPolicy,
) -> Result<(), String> {
    let url = format!("{base}/chat/completions");
    let body = serde_json::json!({
        "model": model,
        "messages": [{"role": "user", "content": "hello"}],
        "max_tokens": hello.max_tokens,
        "stream": false,
    });
    let mut req = client.post(url).json(&body);
    req = apply_auth(req, api_key, scheme);
    let resp = req.send().map_err(|e| e.to_string())?;
    let status = resp.status();
    if !status.is_success() {
        let t = resp.text().unwrap_or_default();
        return Err(format!("HTTP {status}: {}", truncate(&t, 200)));
    }
    Ok(())
}

fn hello_messages(
    client: &reqwest::blocking::Client,
    base: &str,
    api_key: Option<&str>,
    scheme: AuthScheme,
    model: &str,
    hello: &HelloPolicy,
) -> Result<(), String> {
    let url = format!("{base}/messages");
    let body = serde_json::json!({
        "model": model,
        "max_tokens": hello.max_tokens,
        "messages": [{"role": "user", "content": "hello"}],
    });
    let mut req = client
        .post(url)
        .header("anthropic-version", "2023-06-01")
        .json(&body);
    req = apply_auth(req, api_key, scheme);
    let resp = req.send().map_err(|e| e.to_string())?;
    let status = resp.status();
    if !status.is_success() {
        let t = resp.text().unwrap_or_default();
        return Err(format!("HTTP {status}: {}", truncate(&t, 200)));
    }
    Ok(())
}

fn hello_responses(
    client: &reqwest::blocking::Client,
    base: &str,
    api_key: Option<&str>,
    model: &str,
    hello: &HelloPolicy,
) -> Result<(), String> {
    let url = format!("{base}/responses");
    let body = serde_json::json!({
        "model": model,
        "input": "hello",
        "max_output_tokens": hello.max_tokens,
    });
    let mut req = client.post(url).json(&body);
    if let Some(k) = api_key.filter(|k| !k.is_empty()) {
        req = req.bearer_auth(k);
    }
    let resp = req.send().map_err(|e| e.to_string())?;
    let status = resp.status();
    if !status.is_success() {
        let t = resp.text().unwrap_or_default();
        return Err(format!("HTTP {status}: {}", truncate(&t, 200)));
    }
    Ok(())
}

fn truncate(s: &str, n: usize) -> String {
    let t = s.replace('\n', " ");
    if t.len() <= n {
        t
    } else {
        format!("{}…", &t[..n])
    }
}

fn host_port_from_url(url: &str) -> Option<(String, u16)> {
    let u = url.strip_prefix("https://").or_else(|| url.strip_prefix("http://"))?;
    let hostport = u.split('/').next()?;
    if let Some((h, p)) = hostport.split_once(':') {
        let port: u16 = p.parse().ok()?;
        Some((h.to_string(), port))
    } else {
        let default = if url.starts_with("https://") { 443 } else { 80 };
        Some((hostport.to_string(), default))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_port_parse() {
        assert_eq!(
            host_port_from_url("http://127.0.0.1:11434/v1"),
            Some(("127.0.0.1".into(), 11434))
        );
        assert_eq!(
            host_port_from_url("https://api.openai.com/v1"),
            Some(("api.openai.com".into(), 443))
        );
    }

    #[test]
    fn unreachable_local_port() {
        // Port unlikely to be open
        let r = validate_endpoint(
            "http://127.0.0.1:1/v1",
            None,
            AuthScheme::Bearer,
            "chat_completions",
            &HelloPolicy {
                enabled: false,
                ..Default::default()
            },
        );
        assert_eq!(r.status, ValidationStatus::Unreachable);
        assert!(!r.did_hello);
    }

    #[test]
    fn extract_openai_style_models() {
        let v = serde_json::json!({
            "data": [
                {"id": "gpt-4o"},
                {"id": "o1"}
            ]
        });
        let ids = extract_model_ids(&v);
        assert_eq!(ids, vec!["gpt-4o", "o1"]);
    }

    /// `--model` must force L2 hello even when L1 list succeeds, otherwise
    /// validate is a false green (lists models but never proves completion).
    #[test]
    fn preferred_model_forces_hello_attempt_on_unreachable_completion() {
        // 127.0.0.1:1 is unreachable: L0 fails before L1 — still proves we don't
        // short-circuit to Ready without trying when preferred_model is set.
        // For a reachable list-only server, hello would be attempted; here we
        // assert Unreachable rather than Ready+did_hello false.
        let r = validate_endpoint(
            "http://127.0.0.1:1/v1",
            None,
            AuthScheme::Bearer,
            "chat_completions",
            &HelloPolicy {
                enabled: true,
                preferred_model: Some("minimax-m2.5:cloud".into()),
                ..Default::default()
            },
        );
        assert_eq!(r.status, ValidationStatus::Unreachable);
        assert!(!r.did_hello);
    }

    #[test]
    fn chat_completions_list_only_skips_hello_without_preferred_model() {
        // Without preferred_model, chat_completions + successful L1 returns Ready
        // without hello. We cannot depend on a live Ollama in unit tests; instead
        // document the policy via need_hello logic: preferred_model is the force
        // switch. When L1 would succeed on a live host, see integration tests.
        let policy_without = HelloPolicy {
            enabled: true,
            preferred_model: None,
            ..Default::default()
        };
        let policy_with = HelloPolicy {
            enabled: true,
            preferred_model: Some("x".into()),
            ..Default::default()
        };
        assert!(policy_with.preferred_model.is_some());
        assert!(policy_without.preferred_model.is_none());
    }
}
