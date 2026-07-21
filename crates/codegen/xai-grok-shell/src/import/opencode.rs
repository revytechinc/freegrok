//! Parse OpenCode `opencode.json` / `opencode.jsonc` into Grok model hints.

use super::paths::{opencode_auth_path, opencode_config_paths};
use serde_json::Value;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct OpenCodeModelHint {
    pub provider_id: String,
    pub model_id: String,
    pub base_url: Option<String>,
    pub npm: Option<String>,
    /// Suggested Grok catalog key `provider/model`.
    pub grok_key: String,
}

#[derive(Debug, Clone, Default)]
pub struct OpenCodeImport {
    pub config_paths_found: Vec<PathBuf>,
    pub auth_path: Option<PathBuf>,
    pub default_model: Option<String>,
    pub models: Vec<OpenCodeModelHint>,
    pub has_auth_file: bool,
    pub notes: Vec<String>,
}

/// Strip JSONC-style `//` line comments for a best-effort parse.
fn strip_jsonc_line_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("//") {
            out.push('\n');
            continue;
        }
        // naive: cut // outside strings (good enough for opencode configs)
        if let Some(idx) = find_unquoted_line_comment(line) {
            out.push_str(&line[..idx]);
        } else {
            out.push_str(line);
        }
        out.push('\n');
    }
    out
}

fn find_unquoted_line_comment(line: &str) -> Option<usize> {
    let mut in_str = false;
    let mut escape = false;
    let bytes = line.as_bytes();
    let mut i = 0;
    while i + 1 < bytes.len() {
        let c = bytes[i] as char;
        if escape {
            escape = false;
            i += 1;
            continue;
        }
        if in_str {
            if c == '\\' {
                escape = true;
            } else if c == '"' {
                in_str = false;
            }
            i += 1;
            continue;
        }
        if c == '"' {
            in_str = true;
            i += 1;
            continue;
        }
        if c == '/' && bytes[i + 1] as char == '/' {
            return Some(i);
        }
        i += 1;
    }
    None
}

pub fn scan_opencode(cwd: Option<&Path>) -> OpenCodeImport {
    let mut out = OpenCodeImport::default();
    if let Some(p) = opencode_auth_path() {
        if p.is_file() {
            out.auth_path = Some(p.clone());
            out.has_auth_file = true;
        }
    }

    for path in opencode_config_paths(cwd) {
        if !path.is_file() {
            continue;
        }
        out.config_paths_found.push(path.clone());
        let Ok(raw) = std::fs::read_to_string(&path) else {
            out.notes
                .push(format!("could not read {}", path.display()));
            continue;
        };
        let cleaned = strip_jsonc_line_comments(&raw);
        let Ok(v) = serde_json::from_str::<Value>(&cleaned) else {
            out.notes
                .push(format!("JSON parse failed: {}", path.display()));
            continue;
        };
        if let Some(m) = v.get("model").and_then(|x| x.as_str()) {
            out.default_model = Some(m.to_string());
        }
        if let Some(providers) = v.get("provider").and_then(|p| p.as_object()) {
            for (pid, pval) in providers {
                let base_url = pval
                    .pointer("/options/baseURL")
                    .or_else(|| pval.pointer("/options/baseUrl"))
                    .and_then(|x| x.as_str())
                    .map(str::to_string);
                let npm = pval.get("npm").and_then(|x| x.as_str()).map(str::to_string);
                if let Some(models) = pval.get("models").and_then(|m| m.as_object()) {
                    for mid in models.keys() {
                        out.models.push(OpenCodeModelHint {
                            provider_id: pid.clone(),
                            model_id: mid.clone(),
                            base_url: base_url.clone(),
                            npm: npm.clone(),
                            grok_key: format!("{pid}/{mid}"),
                        });
                    }
                } else if let Some(def) = &out.default_model {
                    // provider declared without models list
                    if def.starts_with(&format!("{pid}/")) {
                        let model_id = def[pid.len() + 1..].to_string();
                        out.models.push(OpenCodeModelHint {
                            provider_id: pid.clone(),
                            model_id,
                            base_url: base_url.clone(),
                            npm: npm.clone(),
                            grok_key: def.clone(),
                        });
                    }
                }
            }
        }
    }

    if out.config_paths_found.is_empty() {
        out.notes
            .push("No opencode.json found in ~/.config/opencode or project root".into());
    }
    out
}

impl OpenCodeImport {
    pub fn summary_text(&self) -> String {
        let mut lines = vec!["OpenCode import scan:".to_string()];
        for p in &self.config_paths_found {
            lines.push(format!("  config: {}", p.display()));
        }
        if let Some(a) = &self.auth_path {
            lines.push(format!("  auth:   {} (present)", a.display()));
        }
        if let Some(m) = &self.default_model {
            lines.push(format!("  default model: {m}"));
        }
        lines.push(format!("  model hints: {}", self.models.len()));
        for h in self.models.iter().take(12) {
            let base = h.base_url.as_deref().unwrap_or("(default base)");
            lines.push(format!("    • {} @ {base}", h.grok_key));
        }
        if self.models.len() > 12 {
            lines.push(format!("    … +{} more", self.models.len() - 12));
        }
        for n in &self.notes {
            lines.push(format!("  note: {n}"));
        }
        lines.push(
            "\nNext: map hints into [model.*] (see 11-custom-models) and run:\n  grok providers validate --base-url …"
                .into(),
        );
        lines.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn parse_minimal_opencode_json() {
        let dir = std::env::temp_dir().join(format!("oc-import-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("opencode.json");
        let mut f = std::fs::File::create(&path).unwrap();
        write!(
            f,
            r#"{{
  "model": "ollama/llama3.2",
  "provider": {{
    "ollama": {{
      "npm": "@ai-sdk/openai-compatible",
      "options": {{ "baseURL": "http://127.0.0.1:11434/v1" }},
      "models": {{ "llama3.2": {{ "name": "Llama" }} }}
    }}
  }}
}}"#
        )
        .unwrap();
        let imp = scan_opencode(Some(&dir));
        assert!(imp.models.iter().any(|m| m.grok_key == "ollama/llama3.2"));
        assert_eq!(imp.default_model.as_deref(), Some("ollama/llama3.2"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn strip_jsonc_comments() {
        let s = strip_jsonc_line_comments("// hi\n{\"a\": 1} // tail\n");
        assert!(serde_json::from_str::<Value>(&s).is_ok());
    }
}
