//! Multi-source config import (OpenCode, Claude, Cursor, Junie).
//!
//! Claude already has a dedicated interactive path (`claude_import`).
//! This module adds scanners that produce a common [`ImportPlanLite`] for
//! provider/model materialization and future unified `/import` UI.

pub mod opencode;
pub mod paths;

pub use opencode::{scan_opencode, OpenCodeImport};
pub use paths::ImportSource;
