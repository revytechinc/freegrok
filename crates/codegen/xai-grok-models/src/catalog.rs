//! models.dev provider/model catalog.
//!
//! Load order for [`load_catalog`]:
//! 1. User cache (`~/.grok/cache/models_dev.json` or gzip sibling)
//! 2. Shipped gzip embedded in this crate (`catalog/models_dev.json.gz`)
//!
//! Network refresh is optional: fetch bytes, then [`try_update_from_bytes`] —
//! the on-disk cache is replaced **only** when parse + validate succeed.

use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;
use serde::{Deserialize, Serialize};

/// Embedded gzip of https://models.dev/api.json (shipped offline fallback).
pub const SHIPPED_MODELS_DEV_GZ: &[u8] =
    include_bytes!("../catalog/models_dev.json.gz");

/// Canonical models.dev catalog URL.
pub const MODELS_DEV_URL: &str = "https://models.dev/api.json";

/// Relative path under share dir: `share/grok-build/models_dev.json.gz`.
pub const INSTALL_CATALOG_REL: &str = "share/grok-build/models_dev.json.gz";

/// Wire protocol used by Grok's sampler for a catalog provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CatalogApiBackend {
    /// OpenAI Chat Completions (`/v1/chat/completions`).
    ChatCompletions,
    /// OpenAI Responses (`/v1/responses`).
    Responses,
    /// Anthropic Messages (`/v1/messages`).
    Messages,
}

/// How the provider authenticates on the wire (when known).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CatalogAuthScheme {
    Bearer,
    XApiKey,
}

/// One models.dev provider entry (tolerant parse).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelsDevProvider {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub env: Vec<String>,
    #[serde(default)]
    pub npm: String,
    #[serde(default)]
    pub api: Option<String>,
    #[serde(default)]
    pub doc: Option<String>,
    #[serde(default)]
    pub models: BTreeMap<String, ModelsDevModel>,
}

/// One model under a provider (tolerant parse; extra fields ignored).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelsDevModel {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub tool_call: bool,
    #[serde(default)]
    pub reasoning: bool,
    #[serde(default)]
    pub temperature: Option<bool>,
    #[serde(default)]
    pub open_weights: Option<bool>,
    #[serde(default)]
    pub family: Option<String>,
    #[serde(default)]
    pub limit: Option<ModelsDevLimit>,
    #[serde(default)]
    pub cost: Option<ModelsDevCost>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelsDevLimit {
    #[serde(default)]
    pub context: Option<u64>,
    #[serde(default)]
    pub input: Option<u64>,
    #[serde(default)]
    pub output: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelsDevCost {
    #[serde(default)]
    pub input: Option<f64>,
    #[serde(default)]
    pub output: Option<f64>,
}

/// Full catalog: provider id → provider.
pub type ModelsDevCatalog = BTreeMap<String, ModelsDevProvider>;

/// Where a loaded catalog came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatalogSource {
    UserCache,
    Shipped,
    /// Parsed from caller-supplied bytes (e.g. after a successful fetch).
    Bytes,
    Path,
}

/// Result of loading a catalog.
#[derive(Debug, Clone)]
pub struct LoadedCatalog {
    pub catalog: ModelsDevCatalog,
    pub source: CatalogSource,
    pub provider_count: usize,
    pub model_count: usize,
}

/// Validation / parse errors (never panic on bad remote JSON).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CatalogError {
    Empty,
    InvalidJson(String),
    InvalidShape(String),
    Io(String),
    Decompress(String),
}

impl std::fmt::Display for CatalogError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => write!(f, "catalog is empty"),
            Self::InvalidJson(e) => write!(f, "invalid JSON: {e}"),
            Self::InvalidShape(e) => write!(f, "invalid catalog shape: {e}"),
            Self::Io(e) => write!(f, "I/O error: {e}"),
            Self::Decompress(e) => write!(f, "decompress error: {e}"),
        }
    }
}

impl std::error::Error for CatalogError {}

/// Map models.dev `npm` package id → Grok sampler backend + auth hint.
pub fn map_npm_to_backend(npm: &str) -> (CatalogApiBackend, CatalogAuthScheme) {
    let n = npm.to_ascii_lowercase();
    if n.contains("anthropic") {
        return (CatalogApiBackend::Messages, CatalogAuthScheme::XApiKey);
    }
    if n.contains("xai") {
        // First-party Grok models often use Responses; third-party xAI-compatible
        // OpenAI paths still default to chat completions when not xAI session.
        return (CatalogApiBackend::Responses, CatalogAuthScheme::Bearer);
    }
    // openai, openai-compatible, openrouter, azure, groq, etc.
    (CatalogApiBackend::ChatCompletions, CatalogAuthScheme::Bearer)
}

/// Known default base URLs when models.dev omits `api`.
pub fn default_base_url(provider_id: &str) -> Option<&'static str> {
    match provider_id {
        "openai" => Some("https://api.openai.com/v1"),
        "anthropic" => Some("https://api.anthropic.com/v1"),
        "xai" => Some("https://api.x.ai/v1"),
        "google" => Some("https://generativelanguage.googleapis.com/v1beta/openai"),
        "groq" => Some("https://api.groq.com/openai/v1"),
        "mistral" => Some("https://api.mistral.ai/v1"),
        "openrouter" => Some("https://openrouter.ai/api/v1"),
        "togetherai" | "together" => Some("https://api.together.xyz/v1"),
        "deepseek" => Some("https://api.deepseek.com/v1"),
        "ollama" => Some("http://127.0.0.1:11434/v1"),
        _ => None,
    }
}

/// Effective base URL for a provider (catalog `api` or known default).
pub fn provider_base_url(provider: &ModelsDevProvider) -> Option<String> {
    if let Some(api) = provider.api.as_ref().filter(|s| !s.is_empty()) {
        return Some(api.clone());
    }
    default_base_url(&provider.id).map(str::to_string)
}

/// Canonical Grok catalog key: `provider_id/model_id`.
pub fn catalog_model_key(provider_id: &str, model_id: &str) -> String {
    format!("{provider_id}/{model_id}")
}

/// Parse UTF-8 JSON catalog bytes.
pub fn parse_catalog_json(bytes: &[u8]) -> Result<ModelsDevCatalog, CatalogError> {
    let text = std::str::from_utf8(bytes)
        .map_err(|e| CatalogError::InvalidJson(e.to_string()))?;
    let catalog: ModelsDevCatalog = serde_json::from_str(text)
        .map_err(|e| CatalogError::InvalidJson(e.to_string()))?;
    validate_catalog(&catalog)?;
    Ok(catalog)
}

/// Parse gzip-compressed JSON.
pub fn parse_catalog_gz(bytes: &[u8]) -> Result<ModelsDevCatalog, CatalogError> {
    let mut decoder = GzDecoder::new(bytes);
    let mut raw = Vec::new();
    decoder
        .read_to_end(&mut raw)
        .map_err(|e| CatalogError::Decompress(e.to_string()))?;
    parse_catalog_json(&raw)
}

/// Auto-detect gzip (magic) vs plain JSON.
pub fn parse_catalog_auto(bytes: &[u8]) -> Result<ModelsDevCatalog, CatalogError> {
    if bytes.len() >= 2 && bytes[0] == 0x1f && bytes[1] == 0x8b {
        parse_catalog_gz(bytes)
    } else {
        parse_catalog_json(bytes)
    }
}

/// Structural validation after parse.
pub fn validate_catalog(catalog: &ModelsDevCatalog) -> Result<(), CatalogError> {
    if catalog.is_empty() {
        return Err(CatalogError::Empty);
    }
    let mut models = 0usize;
    for (key, provider) in catalog {
        if provider.id.is_empty() {
            return Err(CatalogError::InvalidShape(format!(
                "provider key {key:?} has empty id"
            )));
        }
        if provider.name.is_empty() {
            return Err(CatalogError::InvalidShape(format!(
                "provider {} has empty name",
                provider.id
            )));
        }
        if provider.npm.is_empty() {
            return Err(CatalogError::InvalidShape(format!(
                "provider {} has empty npm",
                provider.id
            )));
        }
        for (mid, model) in &provider.models {
            if model.id.is_empty() {
                return Err(CatalogError::InvalidShape(format!(
                    "provider {} model key {mid:?} has empty id",
                    provider.id
                )));
            }
            models += 1;
        }
    }
    if models == 0 {
        return Err(CatalogError::InvalidShape(
            "catalog has providers but zero models".into(),
        ));
    }
    Ok(())
}

fn count_models(catalog: &ModelsDevCatalog) -> usize {
    catalog.values().map(|p| p.models.len()).sum()
}

fn loaded(catalog: ModelsDevCatalog, source: CatalogSource) -> LoadedCatalog {
    let provider_count = catalog.len();
    let model_count = count_models(&catalog);
    LoadedCatalog {
        catalog,
        source,
        provider_count,
        model_count,
    }
}

/// Load the shipped embedded catalog (always available offline).
pub fn load_shipped_catalog() -> Result<LoadedCatalog, CatalogError> {
    let catalog = parse_catalog_gz(SHIPPED_MODELS_DEV_GZ)?;
    Ok(loaded(catalog, CatalogSource::Shipped))
}

/// Load from an arbitrary path (json or json.gz).
pub fn load_catalog_from_path(path: &Path) -> Result<LoadedCatalog, CatalogError> {
    let bytes = fs::read(path).map_err(|e| CatalogError::Io(e.to_string()))?;
    let catalog = parse_catalog_auto(&bytes)?;
    Ok(loaded(catalog, CatalogSource::Path))
}

/// Default user cache path: `~/.grok/cache/models_dev.json.gz` (prefer gzip).
pub fn default_user_cache_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join(".grok/cache/models_dev.json.gz"))
}

/// Plain JSON sibling (also accepted on load).
pub fn default_user_cache_path_json() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join(".grok/cache/models_dev.json"))
}

/// Load: user cache (gz then json) → shipped embed.
pub fn load_catalog() -> Result<LoadedCatalog, CatalogError> {
    if let Some(path) = default_user_cache_path() {
        if path.is_file() {
            match load_catalog_from_path(&path) {
                Ok(mut loaded) => {
                    loaded.source = CatalogSource::UserCache;
                    return Ok(loaded);
                }
                Err(_) => {
                    // Fall through; bad cache must not brick the product.
                }
            }
        }
    }
    if let Some(path) = default_user_cache_path_json() {
        if path.is_file() {
            match load_catalog_from_path(&path) {
                Ok(mut loaded) => {
                    loaded.source = CatalogSource::UserCache;
                    return Ok(loaded);
                }
                Err(_) => {}
            }
        }
    }
    load_shipped_catalog()
}

/// Parse/validate caller-fetched bytes. On success, optionally write user cache.
///
/// Returns `Err` without modifying the cache when validation fails.
pub fn try_update_from_bytes(
    bytes: &[u8],
    write_cache: bool,
) -> Result<LoadedCatalog, CatalogError> {
    let catalog = parse_catalog_auto(bytes)?;
    let result = loaded(catalog, CatalogSource::Bytes);
    if write_cache {
        if let Some(path) = default_user_cache_path() {
            // Best-effort; parse already succeeded.
            let _ = write_catalog_gz(&path, &result.catalog);
        }
    }
    Ok(result)
}

/// Atomically write catalog as gzip to `path`.
pub fn write_catalog_gz(path: &Path, catalog: &ModelsDevCatalog) -> Result<(), CatalogError> {
    validate_catalog(catalog)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| CatalogError::Io(e.to_string()))?;
    }
    let json = serde_json::to_vec(catalog).map_err(|e| CatalogError::InvalidJson(e.to_string()))?;
    let tmp = path.with_extension("json.gz.tmp");
    {
        let file = fs::File::create(&tmp).map_err(|e| CatalogError::Io(e.to_string()))?;
        let mut enc = GzEncoder::new(file, Compression::default());
        enc.write_all(&json)
            .map_err(|e| CatalogError::Io(e.to_string()))?;
        enc.finish()
            .map_err(|e| CatalogError::Io(e.to_string()))?;
    }
    fs::rename(&tmp, path).map_err(|e| CatalogError::Io(e.to_string()))?;
    Ok(())
}

/// Optional HTTP fetch of models.dev. Does **not** write cache; call
/// [`try_update_from_bytes`] with the body after success.
///
/// Uses a short timeout so FreeBSD doctor / offline hosts do not stall.
#[cfg(feature = "fetch")]
pub fn fetch_models_dev(timeout_secs: u64) -> Result<Vec<u8>, CatalogError> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(timeout_secs.max(1)))
        .user_agent("grok-build-models-catalog/0.1")
        .build()
        .map_err(|e| CatalogError::Io(e.to_string()))?;
    let resp = client
        .get(MODELS_DEV_URL)
        .send()
        .map_err(|e| CatalogError::Io(e.to_string()))?;
    if !resp.status().is_success() {
        return Err(CatalogError::Io(format!(
            "HTTP {} from models.dev",
            resp.status()
        )));
    }
    resp.bytes()
        .map(|b| b.to_vec())
        .map_err(|e| CatalogError::Io(e.to_string()))
}

/// Fetch models.dev, validate, and update user cache only on success.
#[cfg(feature = "fetch")]
pub fn refresh_user_cache(timeout_secs: u64) -> Result<LoadedCatalog, CatalogError> {
    let bytes = fetch_models_dev(timeout_secs)?;
    try_update_from_bytes(&bytes, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = include_str!("../catalog/models_dev_fixture.json");

    #[test]
    fn fixture_parses_and_validates() {
        let catalog = parse_catalog_json(FIXTURE.as_bytes()).expect("fixture");
        assert!(catalog.len() >= 3);
        assert!(catalog.contains_key("openai"));
        assert!(catalog.contains_key("anthropic"));
        assert!(catalog.contains_key("minimax"));
        let mm = catalog.get("minimax").unwrap();
        assert!(
            mm.api
                .as_deref()
                .unwrap_or("")
                .contains("minimax"),
            "minimax api={:?}",
            mm.api
        );
        let (backend, auth) = map_npm_to_backend(&mm.npm);
        assert_eq!(backend, CatalogApiBackend::Messages);
        assert_eq!(auth, CatalogAuthScheme::XApiKey);
    }

    #[test]
    fn shipped_gz_parses() {
        let loaded = load_shipped_catalog().expect("shipped");
        assert_eq!(loaded.source, CatalogSource::Shipped);
        assert!(loaded.provider_count >= 100, "providers={}", loaded.provider_count);
        assert!(loaded.model_count >= 1000, "models={}", loaded.model_count);
        assert!(loaded.catalog.contains_key("openai"));
    }

    #[test]
    fn empty_rejected() {
        let err = parse_catalog_json(b"{}").unwrap_err();
        assert_eq!(err, CatalogError::Empty);
    }

    #[test]
    fn invalid_json_rejected() {
        assert!(parse_catalog_json(b"not-json").is_err());
    }

    #[test]
    fn bad_bytes_do_not_look_like_success() {
        assert!(try_update_from_bytes(b"{}", false).is_err());
        assert!(try_update_from_bytes(b"[1,2,3]", false).is_err());
    }

    #[test]
    fn npm_mapping() {
        assert_eq!(
            map_npm_to_backend("@ai-sdk/openai-compatible"),
            (
                CatalogApiBackend::ChatCompletions,
                CatalogAuthScheme::Bearer
            )
        );
        assert_eq!(
            map_npm_to_backend("@ai-sdk/anthropic"),
            (CatalogApiBackend::Messages, CatalogAuthScheme::XApiKey)
        );
        assert_eq!(
            map_npm_to_backend("@ai-sdk/xai"),
            (CatalogApiBackend::Responses, CatalogAuthScheme::Bearer)
        );
    }

    #[test]
    fn write_and_reload_gz_roundtrip() {
        let catalog = parse_catalog_json(FIXTURE.as_bytes()).unwrap();
        let dir = tempfile_dir();
        let path = dir.join("models_dev.json.gz");
        write_catalog_gz(&path, &catalog).unwrap();
        let again = load_catalog_from_path(&path).unwrap();
        assert_eq!(again.catalog.len(), catalog.len());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn catalog_model_key_format() {
        assert_eq!(
            catalog_model_key("minimax", "MiniMax-M2.5"),
            "minimax/MiniMax-M2.5"
        );
    }

    fn tempfile_dir() -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!(
            "xai-grok-models-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&p).unwrap();
        p
    }
}
