//! Multi-provider discovery and connection validation.
//!
//! - [`discover`]: find local servers, env keys, sibling tools, existing models
//! - [`connection`]: L0 reach / L1 list models / L2 tiny "hello" completion
//! - [`model_discover`]: dynamic model lists for **any** provider (live, CLI,
//!   config, catalog, recent fallback)
//!
//! Used by `grok providers`, `/connect`, and doctor (non-stalling offline path).

pub mod connection;
pub mod discover;
pub mod model_discover;

pub use connection::{
    validate_endpoint, HelloPolicy, ValidationLevel, ValidationReport, ValidationStatus,
};
pub use discover::{discover_installed, DiscoverOptions, FoundInstallReport, FoundItem, FoundKind};
pub use model_discover::{
    cache_discovered_models, discover_models, discover_models_for_found, extract_model_ids,
    list_models_http, parse_agy_models_output, recent_models_for_provider, DiscoveredModel,
    ModelDiscoveryReport, ModelDiscoveryRequest, ModelListSource,
};
