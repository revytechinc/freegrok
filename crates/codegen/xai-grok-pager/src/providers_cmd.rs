//! CLI: `grok providers` — discover installs and validate endpoints.

use clap::{Parser, Subcommand};
use xai_grok_shell::providers::{
    discover_installed, validate_endpoint, DiscoverOptions, HelloPolicy,
};
use xai_grok_shell::sampling::AuthScheme;

#[derive(Debug, Clone, Parser)]
pub struct ProvidersArgs {
    #[command(subcommand)]
    pub command: Option<ProvidersCommand>,
}

#[derive(Debug, Clone, Subcommand)]
pub enum ProvidersCommand {
    /// Scan local servers, env keys, and sibling tool configs (default).
    Discover {
        /// JSON output.
        #[arg(long)]
        json: bool,
    },
    /// Validate a base URL (L0/L1; L2 hello unless --no-hello).
    Validate {
        /// Base URL including /v1 when required (e.g. http://127.0.0.1:11434/v1).
        #[arg(long)]
        base_url: String,
        /// API key or env var name is not expanded — pass the key value or rely on --env-key.
        #[arg(long)]
        api_key: Option<String>,
        /// Read key from this environment variable.
        #[arg(long)]
        env_key: Option<String>,
        /// chat_completions | messages | responses
        #[arg(long, default_value = "chat_completions")]
        api_backend: String,
        /// bearer | x_api_key
        #[arg(long, default_value = "bearer")]
        auth_scheme: String,
        /// Skip L2 hello completion.
        #[arg(long)]
        no_hello: bool,
        /// Model id for hello (default: first from /models).
        #[arg(long)]
        model: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Show models.dev catalog stats (offline shipped/cache).
    Catalog {
        #[arg(long)]
        json: bool,
    },
    /// Discover models for a provider (live /models, CLI, config, catalog, recent).
    ///
    /// Works for any provider id (`openai`, `ollama`, `google`, `antigravity`, …)
    /// or a raw `--base-url`. Prefer live listing; falls back to catalog then
    /// a short recent-models list when nothing else is available.
    Models {
        /// Provider id (openai, anthropic, google, ollama, antigravity/agy, …).
        #[arg(long)]
        provider: Option<String>,
        /// OpenAI-compatible base URL (overrides provider default).
        #[arg(long)]
        base_url: Option<String>,
        #[arg(long)]
        api_key: Option<String>,
        #[arg(long)]
        env_key: Option<String>,
        /// bearer | x_api_key
        #[arg(long, default_value = "bearer")]
        auth_scheme: String,
        /// Skip HTTP /models.
        #[arg(long)]
        offline: bool,
        /// Skip provider CLI (`ollama list`, `agy models`).
        #[arg(long)]
        no_cli: bool,
        /// Skip models.dev catalog.
        #[arg(long)]
        no_catalog: bool,
        /// Do not use curated recent fallback when empty.
        #[arg(long)]
        no_recent: bool,
        #[arg(long)]
        json: bool,
    },
    /// Scan OpenCode configs for model/provider hints.
    ImportOpenCode {
        /// Project directory (default: cwd).
        #[arg(long)]
        cwd: Option<std::path::PathBuf>,
    },
    /// Scan Google Antigravity (`agy`) / `~/.gemini` for MCP, skills, Gemini auth.
    ImportAntigravity {
        /// Override Gemini home (default: `~/.gemini`).
        #[arg(long)]
        gemini_home: Option<std::path::PathBuf>,
        /// JSON output (findings + summary fields).
        #[arg(long)]
        json: bool,
    },
}

pub async fn run(args: ProvidersArgs) -> anyhow::Result<()> {
    let cmd = args.command.unwrap_or(ProvidersCommand::Discover { json: false });
    match cmd {
        ProvidersCommand::Discover { json } => {
            let report = discover_installed(&DiscoverOptions::default());
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!("{}", report.summary_text());
                println!();
                println!("Tip: grok providers validate --base-url http://127.0.0.1:11434/v1");
                println!("     grok providers catalog");
            }
            Ok(())
        }
        ProvidersCommand::Validate {
            base_url,
            api_key,
            env_key,
            api_backend,
            auth_scheme,
            no_hello,
            model,
            json,
        } => {
            let key = api_key.or_else(|| {
                env_key.and_then(|n| std::env::var(n).ok()).filter(|s| !s.is_empty())
            });
            let scheme = match auth_scheme.as_str() {
                "x_api_key" | "x-api-key" => AuthScheme::XApiKey,
                _ => AuthScheme::Bearer,
            };
            let hello = HelloPolicy {
                enabled: !no_hello,
                preferred_model: model,
                ..Default::default()
            };
            // Blocking HTTP in spawn_blocking so we don't block async runtime badly.
            let base_url2 = base_url.clone();
            let api_backend2 = api_backend.clone();
            let report = tokio::task::spawn_blocking(move || {
                validate_endpoint(
                    &base_url2,
                    key.as_deref(),
                    scheme,
                    &api_backend2,
                    &hello,
                )
            })
            .await?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!("status:     {:?}", report.status);
                println!("level:      {:?}", report.highest_level);
                println!("base_url:   {}", report.base_url);
                println!("latency_ms: {}", report.latency_ms);
                println!("did_hello:  {}", report.did_hello);
                if let Some(n) = report.models_found {
                    println!("models:     {n}");
                }
                if let Some(ids) = &report.sample_model_ids {
                    println!("sample:     {}", ids.join(", "));
                }
                if let Some(m) = &report.message {
                    println!("message:    {m}");
                }
            }
            let code = match report.status {
                xai_grok_shell::providers::ValidationStatus::Ready => 0,
                xai_grok_shell::providers::ValidationStatus::Degraded => 1,
                _ => 2,
            };
            if code != 0 {
                std::process::exit(code);
            }
            Ok(())
        }
        ProvidersCommand::Catalog { json } => {
            let loaded = xai_grok_shell::models::load_catalog()
                .map_err(|e| anyhow::anyhow!("catalog load failed: {e}"))?;
            if json {
                let v = serde_json::json!({
                    "source": format!("{:?}", loaded.source),
                    "providers": loaded.provider_count,
                    "models": loaded.model_count,
                });
                println!("{}", serde_json::to_string_pretty(&v)?);
            } else {
                println!(
                    "models.dev catalog: source={:?} providers={} models={}",
                    loaded.source, loaded.provider_count, loaded.model_count
                );
                println!("URL: {}", xai_grok_shell::models::MODELS_DEV_URL);
            }
            Ok(())
        }
        ProvidersCommand::Models {
            provider,
            base_url,
            api_key,
            env_key,
            auth_scheme,
            offline,
            no_cli,
            no_catalog,
            no_recent,
            json,
        } => {
            use xai_grok_shell::providers::{discover_models, ModelDiscoveryRequest};
            use xai_grok_shell::sampling::AuthScheme;

            let provider = provider
                .or_else(|| base_url.as_ref().map(|_| "custom".to_string()))
                .unwrap_or_else(|| "openai".to_string());
            let mut req = ModelDiscoveryRequest::for_provider(&provider);
            if let Some(u) = base_url {
                req.base_url = Some(u);
            }
            let key = api_key.or_else(|| {
                env_key
                    .and_then(|n| std::env::var(n).ok())
                    .filter(|s| !s.is_empty())
            });
            if let Some(k) = key {
                req.api_key = Some(k);
            }
            req.auth_scheme = match auth_scheme.as_str() {
                "x_api_key" | "x-api-key" => AuthScheme::XApiKey,
                _ => AuthScheme::Bearer,
            };
            if offline {
                req.skip_live = true;
            }
            req.skip_cli = no_cli;
            req.skip_catalog = no_catalog;
            req.skip_recent_fallback = no_recent;
            req.cwd = std::env::current_dir().ok();

            let rep = tokio::task::spawn_blocking(move || discover_models(&req)).await?;
            // Cache agy CLI results when we got live CLI data
            if rep.provider_id == "antigravity"
                && rep
                    .models
                    .iter()
                    .any(|m| m.source == xai_grok_shell::providers::ModelListSource::ProviderCli)
            {
                if let Some(home) = std::env::var_os("HOME").map(std::path::PathBuf::from) {
                    let pairs: Vec<_> = rep
                        .models
                        .iter()
                        .filter(|m| {
                            m.source == xai_grok_shell::providers::ModelListSource::ProviderCli
                        })
                        .map(|m| (m.id.clone(), m.display_name.clone()))
                        .collect();
                    let _ = xai_grok_shell::providers::cache_discovered_models(
                        &home.join(".gemini"),
                        &pairs,
                    );
                }
            }
            if json {
                println!("{}", serde_json::to_string_pretty(&rep)?);
            } else {
                println!("{}", rep.summary_text());
            }
            Ok(())
        }
        ProvidersCommand::ImportOpenCode { cwd } => {
            let cwd = cwd.or_else(|| std::env::current_dir().ok());
            let imp = xai_grok_shell::import::scan_opencode(cwd.as_deref());
            println!("{}", imp.summary_text());
            Ok(())
        }
        ProvidersCommand::ImportAntigravity { gemini_home, json } => {
            let imp = if let Some(root) = gemini_home {
                xai_grok_shell::import::scan_antigravity_in(Some(root.as_path()), None)
            } else {
                xai_grok_shell::import::scan_antigravity()
            };
            if json {
                let v = serde_json::json!({
                    "gemini_home": imp.gemini_home,
                    "mcp_servers": imp.mcp_servers.len(),
                    "skills": imp.skills.len(),
                    "model_hints": imp.model_hints,
                    "env_api_key_present": imp.env_api_key_present,
                    "auth": format!("{:?}", imp.auth),
                    "findings": imp.findings,
                    "notes": imp.notes,
                });
                println!("{}", serde_json::to_string_pretty(&v)?);
            } else {
                println!("{}", imp.summary_text());
            }
            Ok(())
        }
    }
}
