//! `/import` — multi-source import scan (OpenCode first; Claude via /import-claude).

use crate::slash::command::{CommandExecCtx, CommandResult, SlashCommand};
use xai_grok_shell::import::scan_opencode;
use xai_grok_shell::providers::{discover_installed, DiscoverOptions};

pub struct ImportCommand;

impl SlashCommand for ImportCommand {
    fn name(&self) -> &str {
        "import"
    }

    fn description(&self) -> &str {
        "Scan sibling tools for importable config (opencode | cursor | junie | claude)"
    }

    fn usage(&self) -> &str {
        "/import [opencode|cursor|junie|claude]"
    }

    fn run(&self, _ctx: &mut CommandExecCtx, args: &str) -> CommandResult {
        let src = args.trim().to_ascii_lowercase();
        match src.as_str() {
            "" | "all" => {
                let disc = discover_installed(&DiscoverOptions::default());
                let oc = scan_opencode(std::env::current_dir().ok().as_deref());
                let mut msg = disc.summary_text();
                msg.push_str("\n\n");
                msg.push_str(&oc.summary_text());
                msg.push_str("\n\nClaude: use /import-claude for interactive settings import.\n");
                msg.push_str("Cursor/Junie: detected paths appear under discovery when present.\n");
                CommandResult::Message(msg)
            }
            "opencode" | "oc" => {
                let oc = scan_opencode(std::env::current_dir().ok().as_deref());
                CommandResult::Message(oc.summary_text())
            }
            "claude" => CommandResult::Message(
                "Use /import-claude for the interactive Claude settings import modal.".into(),
            ),
            "cursor" => {
                let d = discover_installed(&DiscoverOptions {
                    probe_local: false,
                    probe_env: false,
                    probe_binaries: false,
                    probe_sibling: true,
                    ..Default::default()
                });
                let cursor: Vec<_> = d
                    .items
                    .iter()
                    .filter(|i| i.id.contains("cursor"))
                    .collect();
                if cursor.is_empty() {
                    CommandResult::Message(
                        "No Cursor config dirs found (~/.cursor). Skills/MCP may still load via compat."
                            .into(),
                    )
                } else {
                    let mut msg = String::from("Cursor paths:\n");
                    for i in cursor {
                        msg.push_str(&format!("  • {} — {}\n", i.label, i.detail.as_deref().unwrap_or("")));
                    }
                    msg.push_str("MCP/rules are live-discovered; model keys: use env + [model.*].\n");
                    CommandResult::Message(msg)
                }
            }
            "junie" | "jetbrains" => {
                let roots = xai_grok_shell::import::paths::junie_search_roots();
                let found: Vec<_> = roots.into_iter().filter(|p| p.exists()).collect();
                if found.is_empty() {
                    CommandResult::Message(
                        "JetBrains/Junie config roots not found on this host (best-effort scan)."
                            .into(),
                    )
                } else {
                    let mut msg = String::from("JetBrains roots present:\n");
                    for p in found {
                        msg.push_str(&format!("  • {}\n", p.display()));
                    }
                    msg.push_str(
                        "Junie model import is best-effort; prefer OpenAI-compatible env + [model.*].\n",
                    );
                    CommandResult::Message(msg)
                }
            }
            other => CommandResult::Error(format!(
                "unknown import source {other:?}; try opencode|cursor|junie|claude"
            )),
        }
    }
}
