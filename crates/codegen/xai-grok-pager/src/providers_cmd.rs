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
    /// Scan OpenCode configs for model/provider hints.
    ImportOpenCode {
        /// Project directory (default: cwd).
        #[arg(long)]
        cwd: Option<std::path::PathBuf>,
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
        ProvidersCommand::ImportOpenCode { cwd } => {
            let cwd = cwd.or_else(|| std::env::current_dir().ok());
            let imp = xai_grok_shell::import::scan_opencode(cwd.as_deref());
            println!("{}", imp.summary_text());
            Ok(())
        }
    }
}
