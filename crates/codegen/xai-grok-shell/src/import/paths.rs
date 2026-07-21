//! Paths for sibling coding tools.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportSource {
    Claude,
    Cursor,
    OpenCode,
    Junie,
    /// Google Antigravity CLI (`agy`) under `~/.gemini`.
    Antigravity,
}

impl ImportSource {
    pub fn label(self) -> &'static str {
        match self {
            Self::Claude => "Claude Code",
            Self::Cursor => "Cursor",
            Self::OpenCode => "OpenCode",
            Self::Junie => "JetBrains Junie",
            Self::Antigravity => "Antigravity (agy)",
        }
    }
}

pub fn home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

/// `~/.gemini` — shared home for Gemini CLI + Antigravity.
pub fn gemini_home() -> Option<PathBuf> {
    home().map(|h| h.join(".gemini"))
}

pub fn antigravity_cli_dir() -> Option<PathBuf> {
    gemini_home().map(|g| g.join("antigravity-cli"))
}

pub fn antigravity_settings_path() -> Option<PathBuf> {
    antigravity_cli_dir().map(|d| d.join("settings.json"))
}

pub fn antigravity_oauth_token_path() -> Option<PathBuf> {
    antigravity_cli_dir().map(|d| d.join("antigravity-oauth-token"))
}

pub fn antigravity_mcp_config_path() -> Option<PathBuf> {
    gemini_home().map(|g| g.join("config/mcp_config.json"))
}

pub fn gemini_config_json_path() -> Option<PathBuf> {
    gemini_home().map(|g| g.join("config/config.json"))
}

/// User + builtin skill roots under Gemini/Antigravity.
pub fn gemini_skills_roots() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(g) = gemini_home() {
        out.push(g.join("config/skills"));
        out.push(g.join("antigravity-cli/builtin/skills"));
    }
    out
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn import_source_labels() {
        assert_eq!(ImportSource::Antigravity.label(), "Antigravity (agy)");
        assert_eq!(ImportSource::OpenCode.label(), "OpenCode");
        assert_eq!(ImportSource::Claude.label(), "Claude Code");
        assert_eq!(ImportSource::Cursor.label(), "Cursor");
        assert_eq!(ImportSource::Junie.label(), "JetBrains Junie");
    }

    #[test]
    fn opencode_paths_include_cwd() {
        let cwd = Path::new("/proj");
        let paths = opencode_config_paths(Some(cwd));
        assert!(paths.iter().any(|p| p.ends_with("opencode.json")));
        assert!(paths.iter().any(|p| p == &cwd.join("opencode.jsonc")));
    }

    #[test]
    fn gemini_path_helpers_when_home_set() {
        // HOME is set in normal environments; just ensure shape.
        if home().is_some() {
            let g = gemini_home().unwrap();
            assert!(g.ends_with(".gemini"));
            assert!(antigravity_settings_path()
                .unwrap()
                .ends_with("settings.json"));
            assert!(antigravity_mcp_config_path()
                .unwrap()
                .ends_with("mcp_config.json"));
            assert!(antigravity_oauth_token_path()
                .unwrap()
                .ends_with("antigravity-oauth-token"));
            assert!(!gemini_skills_roots().is_empty());
            assert!(gemini_config_json_path().unwrap().ends_with("config.json"));
            assert!(opencode_auth_path().unwrap().ends_with("auth.json"));
            let cursor = cursor_mcp_paths(Some(Path::new("/p")));
            assert!(cursor.iter().any(|p| p.ends_with(".cursor/mcp.json")));
            assert!(!junie_search_roots().is_empty());
        }
    }
}
