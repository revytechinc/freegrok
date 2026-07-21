//! Paths for sibling coding tools.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportSource {
    Claude,
    Cursor,
    OpenCode,
    Junie,
}

impl ImportSource {
    pub fn label(self) -> &'static str {
        match self {
            Self::Claude => "Claude Code",
            Self::Cursor => "Cursor",
            Self::OpenCode => "OpenCode",
            Self::Junie => "JetBrains Junie",
        }
    }
}

pub fn home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

/// Global OpenCode config candidates.
pub fn opencode_config_paths(cwd: Option<&Path>) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(h) = home() {
        out.push(h.join(".config/opencode/opencode.json"));
        out.push(h.join(".config/opencode/opencode.jsonc"));
    }
    if let Some(c) = cwd {
        out.push(c.join("opencode.json"));
        out.push(c.join("opencode.jsonc"));
    }
    out
}

pub fn opencode_auth_path() -> Option<PathBuf> {
    home().map(|h| h.join(".local/share/opencode/auth.json"))
}

pub fn cursor_mcp_paths(cwd: Option<&Path>) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(h) = home() {
        out.push(h.join(".cursor/mcp.json"));
    }
    if let Some(c) = cwd {
        out.push(c.join(".cursor/mcp.json"));
    }
    out
}

/// Best-effort JetBrains config roots (may not exist).
pub fn junie_search_roots() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(h) = home() {
        out.push(h.join(".config/JetBrains"));
        out.push(h.join("Library/Application Support/JetBrains"));
        out.push(h.join(".local/share/JetBrains"));
    }
    out
}
