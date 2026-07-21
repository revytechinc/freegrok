//! Unified remote/local import findings (pre-UI, post-dedup).

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Product that contributed a raw finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceProduct {
    Grok,
    OpenCode,
    Claude,
    Cursor,
    Codex,
    Junie,
    SharedMcp,
    LocalInference,
    Unknown,
}

impl SourceProduct {
    pub fn label(self) -> &'static str {
        match self {
            Self::Grok => "Grok",
            Self::OpenCode => "OpenCode",
            Self::Claude => "Claude",
            Self::Cursor => "Cursor",
            Self::Codex => "Codex",
            Self::Junie => "Junie",
            Self::SharedMcp => "MCP",
            Self::LocalInference => "Local inference",
            Self::Unknown => "Unknown",
        }
    }

    /// Preference for canonical source (lower = preferred).
    pub fn preference_rank(self) -> u8 {
        match self {
            Self::Grok => 0,
            Self::OpenCode => 1,
            Self::Claude => 2,
            Self::Cursor => 3,
            Self::Codex => 4,
            Self::Junie => 5,
            Self::SharedMcp => 6,
            Self::LocalInference => 7,
            Self::Unknown => 8,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FindingKind {
    ModelProvider,
    Account,
    McpServer,
    Skill,
    Rule,
    AgentsMd,
    Hook,
    EnvVar,
    PathEntry,
    Permission,
    LocalEndpoint,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LocalDiff {
    Missing,
    Same,
    Different,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecretPolicy {
    Never,
    OptIn,
    RequiredForApply,
}

/// One raw (pre-dedup) or collapsed finding.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteFinding {
    pub id: String,
    pub kind: FindingKind,
    pub source_tool: SourceProduct,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote_path: Option<PathBuf>,
    pub display_name: String,
    pub summary: String,
    /// Stable hash of normalized content (not secrets).
    pub content_hash: String,
    /// Bucket key for dedup (see `dedup` module).
    pub equivalence_key: String,
    pub exists_locally: bool,
    pub local_diff: LocalDiff,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_id: Option<String>,
    pub secret_policy: SecretPolicy,
    /// Products that contributed to this bucket (after dedup).
    #[serde(default)]
    pub seen_in_products: Vec<SourceProduct>,
    pub canonical_source: SourceProduct,
    /// If this row was collapsed into another id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duplicate_of: Option<String>,
    /// Optional structured payload (base_url, model ids, mcp command, …).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DedupStats {
    pub raw: usize,
    pub collapsed: usize,
    pub buckets: usize,
}

impl DedupStats {
    pub fn saved(&self) -> usize {
        self.raw.saturating_sub(self.collapsed)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FindReport {
    pub host: Option<String>,
    pub remote_os: Option<String>,
    pub findings: Vec<RemoteFinding>,
    pub dedup: DedupStats,
    #[serde(default)]
    pub notes: Vec<String>,
}

impl FindReport {
    pub fn summary_text(&self) -> String {
        let mut lines = Vec::new();
        if let Some(h) = &self.host {
            lines.push(format!("Host: {h}"));
        }
        if let Some(os) = &self.remote_os {
            lines.push(format!("OS:   {os}"));
        }
        lines.push(format!(
            "dedup: raw={} collapsed={} (saved {} duplicate rows)",
            self.dedup.raw,
            self.dedup.collapsed,
            self.dedup.saved()
        ));
        lines.push(format!("findings: {}", self.findings.len()));
        for f in &self.findings {
            if f.duplicate_of.is_some() {
                continue;
            }
            let flag = match f.local_diff {
                LocalDiff::Missing => "NEW",
                LocalDiff::Same => "EXISTS",
                LocalDiff::Different => "CONFLICT",
            };
            let products: Vec<_> = f.seen_in_products.iter().map(|p| p.label()).collect();
            let also = if products.len() > 1 {
                format!("  also: {}", products.join(", "))
            } else {
                String::new()
            };
            lines.push(format!(
                "  [{flag}] {} — {}{also}",
                f.display_name, f.summary, also = if also.is_empty() { "" } else { &also }
            ));
        }
        for n in &self.notes {
            lines.push(format!("note: {n}"));
        }
        lines.join("\n")
    }

    /// Elect all actionable (NEW or CONFLICT) non-duplicate findings.
    pub fn elect_import_all_actionable(&self) -> Vec<String> {
        self.findings
            .iter()
            .filter(|f| f.duplicate_of.is_none())
            .filter(|f| matches!(f.local_diff, LocalDiff::Missing | LocalDiff::Different))
            .map(|f| f.id.clone())
            .collect()
    }
}
