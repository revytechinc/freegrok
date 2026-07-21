//! Collapse multi-product findings that describe the same real resource.
//!
//! Example: Ollama appears in OpenCode, Cursor, and Grok configs → one bucket.

use super::findings::{
    DedupStats, FindingKind, LocalDiff, RemoteFinding, SourceProduct,
};
use std::collections::BTreeMap;

/// Normalize a base URL for equivalence (localhost ↔ 127.0.0.1, strip slash).
pub fn normalize_base_url(url: &str) -> String {
    let mut u = url.trim().trim_end_matches('/').to_ascii_lowercase();
    u = u.replace("localhost", "127.0.0.1");
    // drop default ports noise later if needed
    u
}

pub fn equivalence_endpoint(base_url: &str) -> String {
    format!("endpoint:{}", normalize_base_url(base_url))
}

pub fn equivalence_model(base_url: &str, model_id: &str) -> String {
    format!(
        "model:{}:{}",
        normalize_base_url(base_url),
        model_id.trim().to_ascii_lowercase()
    )
}

pub fn equivalence_mcp_stdio(command: &str, args: &[String]) -> String {
    let base = std::path::Path::new(command)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(command)
        .to_ascii_lowercase();
    let args_norm: Vec<String> = args
        .iter()
        .map(|a| {
            // Collapse home-prefixed paths to a placeholder for stability
            if a.starts_with('/') || a.starts_with("C:\\") || a.starts_with("c:\\") {
                if a.contains("node_modules") {
                    return a
                        .rsplit(['/', '\\'])
                        .take(3)
                        .collect::<Vec<_>>()
                        .into_iter()
                        .rev()
                        .collect::<Vec<_>>()
                        .join("/");
                }
            }
            a.to_ascii_lowercase()
        })
        .collect();
    format!("mcp:stdio:{base}:{}", args_norm.join("\u{1f}"))
}

pub fn equivalence_mcp_url(url: &str) -> String {
    format!("mcp:url:{}", normalize_base_url(url))
}

pub fn equivalence_skill(name: &str) -> String {
    format!("skill:{}", name.trim().to_ascii_lowercase())
}

pub fn equivalence_env(key: &str) -> String {
    format!("env:{}", key.trim())
}

pub fn equivalence_account(provider: &str, fingerprint: &str) -> String {
    format!(
        "account:{}:{}",
        provider.trim().to_ascii_lowercase(),
        fingerprint
    )
}

/// Known local inference ports → product-agnostic endpoint keys.
pub fn is_local_inference_url(url: &str) -> bool {
    let n = normalize_base_url(url);
    n.contains("127.0.0.1:11434")
        || n.contains("127.0.0.1:1234")
        || n.contains("127.0.0.1:8080")
        || n.contains("127.0.0.1:1337")
}

/// Collapse raw findings by `equivalence_key`.
///
/// Returns only primary (non-duplicate) findings plus stats. Duplicates are
/// marked with `duplicate_of` and omitted from the returned vec (stats still
/// count raw).
pub fn collapse_findings(raw: Vec<RemoteFinding>) -> (Vec<RemoteFinding>, DedupStats) {
    let raw_len = raw.len();
    let mut buckets: BTreeMap<String, Vec<RemoteFinding>> = BTreeMap::new();
    for f in raw {
        buckets.entry(f.equivalence_key.clone()).or_default().push(f);
    }

    let mut out = Vec::new();
    for (_key, mut group) in buckets {
        if group.is_empty() {
            continue;
        }
        // Prefer richer / preferred product as canonical
        group.sort_by_key(|f| {
            (
                f.source_tool.preference_rank(),
                // Prefer non-empty payload / longer summary
                usize::MAX - f.summary.len(),
            )
        });
        let mut primary = group.remove(0);
        let mut products: Vec<SourceProduct> = vec![primary.source_tool];
        let mut hashes: Vec<String> = vec![primary.content_hash.clone()];
        let mut paths: Vec<String> = primary
            .remote_path
            .as_ref()
            .map(|p| p.display().to_string())
            .into_iter()
            .collect();

        for other in group {
            if !products.contains(&other.source_tool) {
                products.push(other.source_tool);
            }
            if !hashes.contains(&other.content_hash) {
                hashes.push(other.content_hash.clone());
            }
            if let Some(p) = &other.remote_path {
                let s = p.display().to_string();
                if !paths.contains(&s) {
                    paths.push(s);
                }
            }
            // Merge model id lists in payload if both are objects with models arrays
            merge_payload(&mut primary.payload, other.payload.as_ref());
        }

        products.sort_by_key(|p| p.preference_rank());
        primary.seen_in_products = products.clone();
        primary.canonical_source = primary.source_tool;
        primary.duplicate_of = None;

        if hashes.len() > 1 {
            primary.local_diff = LocalDiff::Different;
            primary.summary = format!(
                "{} (variants from {} products)",
                primary.summary,
                products.len()
            );
        }

        if products.len() > 1 {
            let labels: Vec<_> = products.iter().map(|p| p.label()).collect();
            if !primary.summary.contains("also:") {
                primary.summary = format!("{} · also in {}", primary.summary, labels.join("/"));
            }
        }

        // EXISTS vs NEW: if any says exists_locally with Same, keep; if mixed, Different
        // (already handled for multi-hash). For single hash with exists:
        // leave primary.exists_locally as-is from canonical; if any in group had
        // exists_locally true and same hash, mark Same.
        if hashes.len() == 1 && primary.exists_locally {
            primary.local_diff = LocalDiff::Same;
        }

        out.push(primary);
    }

    let collapsed = out.len();
    let stats = DedupStats {
        raw: raw_len,
        collapsed,
        buckets: collapsed,
    };
    (out, stats)
}

fn merge_payload(into: &mut Option<serde_json::Value>, other: Option<&serde_json::Value>) {
    let Some(other) = other else { return };
    let Some(into_v) = into.as_mut() else {
        *into = Some(other.clone());
        return;
    };
    let (Some(into_obj), Some(other_obj)) = (into_v.as_object_mut(), other.as_object()) else {
        return;
    };
    // Union "models" arrays by string id
    if let Some(other_models) = other_obj.get("models").and_then(|m| m.as_array()) {
        let entry = into_obj
            .entry("models")
            .or_insert_with(|| serde_json::Value::Array(vec![]));
        if let Some(arr) = entry.as_array_mut() {
            for m in other_models {
                if !arr.contains(m) {
                    arr.push(m.clone());
                }
            }
        }
    }
    for (k, v) in other_obj {
        if k == "models" {
            continue;
        }
        into_obj.entry(k.clone()).or_insert_with(|| v.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import::findings::{FindingKind, LocalDiff, SecretPolicy};

    fn finding(
        id: &str,
        product: SourceProduct,
        key: &str,
        hash: &str,
        name: &str,
    ) -> RemoteFinding {
        RemoteFinding {
            id: id.into(),
            kind: FindingKind::LocalEndpoint,
            source_tool: product,
            remote_path: None,
            display_name: name.into(),
            summary: format!("from {:?}", product),
            content_hash: hash.into(),
            equivalence_key: key.into(),
            exists_locally: false,
            local_diff: LocalDiff::Missing,
            account_id: None,
            secret_policy: SecretPolicy::Never,
            seen_in_products: vec![product],
            canonical_source: product,
            duplicate_of: None,
            payload: Some(serde_json::json!({
                "base_url": "http://127.0.0.1:11434/v1",
                "models": ["llama3"]
            })),
        }
    }

    #[test]
    fn localhost_and_127_share_endpoint_key() {
        assert_eq!(
            equivalence_endpoint("http://localhost:11434/v1/"),
            equivalence_endpoint("http://127.0.0.1:11434/v1")
        );
    }

    #[test]
    fn ollama_in_three_products_collapses_to_one() {
        let key = equivalence_endpoint("http://127.0.0.1:11434/v1");
        let raw = vec![
            finding("1", SourceProduct::OpenCode, &key, "h1", "Ollama"),
            finding("2", SourceProduct::Cursor, &key, "h1", "Ollama"),
            finding("3", SourceProduct::Grok, &key, "h1", "Ollama"),
        ];
        let (out, stats) = collapse_findings(raw);
        assert_eq!(stats.raw, 3);
        assert_eq!(stats.collapsed, 1);
        assert_eq!(stats.saved(), 2);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].seen_in_products.len(), 3);
        // Grok preferred as canonical when present (sorted by rank, Grok first after sort by rank)
        assert_eq!(out[0].canonical_source, SourceProduct::Grok);
    }

    #[test]
    fn different_hashes_mark_conflict() {
        let key = equivalence_endpoint("http://127.0.0.1:11434/v1");
        let raw = vec![
            finding("1", SourceProduct::OpenCode, &key, "hash-a", "Ollama"),
            finding("2", SourceProduct::Cursor, &key, "hash-b", "Ollama"),
        ];
        let (out, _) = collapse_findings(raw);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].local_diff, LocalDiff::Different);
    }

    #[test]
    fn model_lists_union_on_merge() {
        let key = equivalence_endpoint("http://127.0.0.1:11434/v1");
        let mut a = finding("1", SourceProduct::OpenCode, &key, "h1", "Ollama");
        a.payload = Some(serde_json::json!({"models": ["llama3"]}));
        let mut b = finding("2", SourceProduct::Cursor, &key, "h1", "Ollama");
        b.payload = Some(serde_json::json!({"models": ["codellama", "llama3"]}));
        let (out, _) = collapse_findings(vec![a, b]);
        let models = out[0]
            .payload
            .as_ref()
            .unwrap()
            .get("models")
            .unwrap()
            .as_array()
            .unwrap();
        assert_eq!(models.len(), 2);
    }
}
