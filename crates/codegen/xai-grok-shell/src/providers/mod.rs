//! Multi-provider discovery and connection validation.
//!
//! - [`discover`]: find local servers, env keys, sibling tools, existing models
//! - [`connection`]: L0 reach / L1 list models / L2 tiny "hello" completion
//!
//! Used by `grok providers`, `/connect`, and doctor (non-stalling offline path).

pub mod connection;
pub mod discover;

pub use connection::{
    validate_endpoint, HelloPolicy, ValidationLevel, ValidationReport, ValidationStatus,
};
pub use discover::{discover_installed, DiscoverOptions, FoundInstallReport, FoundItem, FoundKind};
