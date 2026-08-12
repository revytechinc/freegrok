//! Platform sandbox backends selected by [`factory::SandboxBackendFactory`].
//!
//! Keeps FreeBSD jail vs Linux/macOS nono paths from fighting inside `lib.rs`.

mod factory;

pub use factory::{SandboxBackendKind, SandboxBackendFactory};

/// Backend gated by the crate `enforce` feature (runtime apply path).
pub fn host_backend_kind() -> SandboxBackendKind {
    SandboxBackendFactory::for_host()
}

/// OS-intended backend for doctor / product labels (ignores `enforce` feature).
pub fn platform_backend_kind() -> SandboxBackendKind {
    SandboxBackendFactory::for_platform()
}
