//! OS-aware remote path catalogs for config import probes.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteOs {
    Linux,
    MacOs,
    FreeBsd,
    OpenBsd,
    NetBsd,
    Windows,
    Unknown,
}

impl RemoteOs {
    pub fn detect_from_uname(uname_s: &str) -> Self {
        let s = uname_s.trim().to_ascii_lowercase();
        if s.contains("darwin") || s.contains("macos") {
            Self::MacOs
        } else if s.contains("freebsd") {
            Self::FreeBsd
        } else if s.contains("openbsd") {
            Self::OpenBsd
        } else if s.contains("netbsd") {
            Self::NetBsd
        } else if s.contains("linux") {
            Self::Linux
        } else if s.contains("mingw") || s.contains("msys") || s.contains("cygwin") || s.contains("windows")
        {
            Self::Windows
        } else {
            Self::Unknown
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Linux => "Linux",
            Self::MacOs => "macOS",
            Self::FreeBsd => "FreeBSD",
            Self::OpenBsd => "OpenBSD",
            Self::NetBsd => "NetBSD",
            Self::Windows => "Windows",
            Self::Unknown => "Unknown",
        }
    }
}

/// A path pattern relative to a root (home, xdg config, appdata, …).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogPath {
    pub id: String,
    pub product: super::findings::SourceProduct,
    /// Path relative to the chosen root, using `/` separators.
    pub rel: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedPath {
    pub id: String,
    pub product: super::findings::SourceProduct,
    pub absolute: PathBuf,
    pub description: String,
}

/// Build absolute paths for a POSIX-like home layout.
pub fn resolve_posix_catalog(home: &Path, os: RemoteOs) -> Vec<ResolvedPath> {
    let xdg = home.join(".config");
    let mut out = Vec::new();

    let push = |out: &mut Vec<ResolvedPath>, id: &str, product, abs: PathBuf, desc: &str| {
        out.push(ResolvedPath {
            id: id.into(),
            product,
            absolute: abs,
            description: desc.into(),
        });
    };

    use super::findings::SourceProduct as P;

    // Grok
    push(
        &mut out,
        "grok.config",
        P::Grok,
        home.join(".grok/config.toml"),
        "Grok config",
    );
    push(
        &mut out,
        "grok.credentials",
        P::Grok,
        home.join(".grok/credentials.json"),
        "Grok credentials store",
    );
    push(
        &mut out,
        "grok.skills",
        P::Grok,
        home.join(".grok/skills"),
        "Grok skills tree",
    );
    push(
        &mut out,
        "grok.accounts",
        P::Grok,
        home.join(".grok/accounts"),
        "Grok accounts index",
    );

    // OpenCode
    push(
        &mut out,
        "opencode.config",
        P::OpenCode,
        xdg.join("opencode/opencode.json"),
        "OpenCode config",
    );
    push(
        &mut out,
        "opencode.configc",
        P::OpenCode,
        xdg.join("opencode/opencode.jsonc"),
        "OpenCode config (jsonc)",
    );
    push(
        &mut out,
        "opencode.auth",
        P::OpenCode,
        home.join(".local/share/opencode/auth.json"),
        "OpenCode auth",
    );

    // Claude
    push(
        &mut out,
        "claude.dir",
        P::Claude,
        home.join(".claude"),
        "Claude Code home",
    );
    push(
        &mut out,
        "claude.json",
        P::Claude,
        home.join(".claude.json"),
        "Claude MCP/settings json",
    );

    // Cursor
    push(
        &mut out,
        "cursor.dir",
        P::Cursor,
        home.join(".cursor"),
        "Cursor home",
    );
    push(
        &mut out,
        "cursor.mcp",
        P::Cursor,
        home.join(".cursor/mcp.json"),
        "Cursor MCP",
    );

    // Shared MCP
    push(
        &mut out,
        "mcp.home",
        P::SharedMcp,
        home.join(".mcp.json"),
        "Home MCP config",
    );

    // Agents / skills roots
    push(
        &mut out,
        "agents.skills",
        P::Unknown,
        home.join(".agents"),
        "Agents skills root",
    );

    // Local inference
    push(
        &mut out,
        "ollama.home",
        P::LocalInference,
        home.join(".ollama"),
        "Ollama data dir",
    );

    // macOS Application Support extras
    if matches!(os, RemoteOs::MacOs) {
        let asup = home.join("Library/Application Support");
        push(
            &mut out,
            "jetbrains.mac",
            P::Junie,
            asup.join("JetBrains"),
            "JetBrains (macOS)",
        );
    } else {
        push(
            &mut out,
            "jetbrains.xdg",
            P::Junie,
            xdg.join("JetBrains"),
            "JetBrains (XDG)",
        );
    }

    out
}

/// Windows-style roots under USERPROFILE / APPDATA / LOCALAPPDATA.
pub fn resolve_windows_catalog(
    userprofile: &Path,
    appdata: Option<&Path>,
    localappdata: Option<&Path>,
) -> Vec<ResolvedPath> {
    use super::findings::SourceProduct as P;
    let mut out = Vec::new();
    let push = |out: &mut Vec<ResolvedPath>, id: &str, product, abs: PathBuf, desc: &str| {
        out.push(ResolvedPath {
            id: id.into(),
            product,
            absolute: abs,
            description: desc.into(),
        });
    };

    push(
        &mut out,
        "grok.config",
        P::Grok,
        userprofile.join(".grok\\config.toml"),
        "Grok config",
    );
    push(
        &mut out,
        "grok.skills",
        P::Grok,
        userprofile.join(".grok\\skills"),
        "Grok skills",
    );
    push(
        &mut out,
        "claude.dir",
        P::Claude,
        userprofile.join(".claude"),
        "Claude Code home",
    );
    push(
        &mut out,
        "cursor.dir",
        P::Cursor,
        userprofile.join(".cursor"),
        "Cursor home",
    );
    push(
        &mut out,
        "cursor.mcp",
        P::Cursor,
        userprofile.join(".cursor\\mcp.json"),
        "Cursor MCP",
    );
    push(
        &mut out,
        "mcp.home",
        P::SharedMcp,
        userprofile.join(".mcp.json"),
        "Home MCP",
    );
    push(
        &mut out,
        "ollama.home",
        P::LocalInference,
        userprofile.join(".ollama"),
        "Ollama",
    );

    if let Some(ad) = appdata {
        push(
            &mut out,
            "opencode.appdata",
            P::OpenCode,
            ad.join("opencode"),
            "OpenCode (AppData)",
        );
        push(
            &mut out,
            "jetbrains.appdata",
            P::Junie,
            ad.join("JetBrains"),
            "JetBrains (AppData)",
        );
    }
    if let Some(la) = localappdata {
        push(
            &mut out,
            "opencode.local",
            P::OpenCode,
            la.join("opencode"),
            "OpenCode (LocalAppData)",
        );
    }

    out
}

/// Project-relative paths (cwd on remote).
pub fn project_catalog(cwd: &Path) -> Vec<ResolvedPath> {
    use super::findings::SourceProduct as P;
    vec![
        ResolvedPath {
            id: "project.mcp".into(),
            product: P::SharedMcp,
            absolute: cwd.join(".mcp.json"),
            description: "Project MCP".into(),
        },
        ResolvedPath {
            id: "project.opencode".into(),
            product: P::OpenCode,
            absolute: cwd.join("opencode.json"),
            description: "Project OpenCode".into(),
        },
        ResolvedPath {
            id: "project.opencode.c".into(),
            product: P::OpenCode,
            absolute: cwd.join("opencode.jsonc"),
            description: "Project OpenCode jsonc".into(),
        },
        ResolvedPath {
            id: "project.claude".into(),
            product: P::Claude,
            absolute: cwd.join(".claude"),
            description: "Project Claude".into(),
        },
        ResolvedPath {
            id: "project.cursor".into(),
            product: P::Cursor,
            absolute: cwd.join(".cursor"),
            description: "Project Cursor".into(),
        },
        ResolvedPath {
            id: "project.grok".into(),
            product: P::Grok,
            absolute: cwd.join(".grok"),
            description: "Project Grok".into(),
        },
        ResolvedPath {
            id: "project.agents_md".into(),
            product: P::Unknown,
            absolute: cwd.join("AGENTS.md"),
            description: "AGENTS.md".into(),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uname_detect() {
        assert_eq!(RemoteOs::detect_from_uname("FreeBSD"), RemoteOs::FreeBsd);
        assert_eq!(RemoteOs::detect_from_uname("Darwin"), RemoteOs::MacOs);
        assert_eq!(RemoteOs::detect_from_uname("Linux"), RemoteOs::Linux);
    }

    #[test]
    fn posix_catalog_includes_ollama_and_mcp() {
        let home = PathBuf::from("/home/alice");
        let paths = resolve_posix_catalog(&home, RemoteOs::Linux);
        assert!(paths.iter().any(|p| p.id == "ollama.home"));
        assert!(paths.iter().any(|p| p.id == "cursor.mcp"));
        assert!(paths.iter().any(|p| p.id == "opencode.config"));
    }
}
