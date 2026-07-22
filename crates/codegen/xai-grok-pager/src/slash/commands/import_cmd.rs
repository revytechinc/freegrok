//! `/import` — multi-source import scan (OpenCode, Antigravity, Claude, …).
//!
//! Results always open the scrollable import-report modal (or the interactive
//! source menu). Never dump multi-line text into scrollback as a system block.

use crate::app::actions::Action;
use crate::slash::command::{CommandExecCtx, CommandResult, SlashCommand};
use xai_grok_shell::import::{scan_antigravity, scan_opencode};
use xai_grok_shell::providers::{discover_installed, DiscoverOptions};

pub struct ImportCommand;

impl SlashCommand for ImportCommand {
    fn name(&self) -> &str {
        "import"
    }

    fn description(&self) -> &str {
        "Import menu / scan sibling tools (opencode | antigravity | cursor | junie | claude)"
    }

    fn usage(&self) -> &str {
        "/import [opencode|antigravity|agy|gemini|cursor|junie|claude]"
    }

    fn takes_args(&self) -> bool {
        true
    }

    fn run(&self, _ctx: &mut CommandExecCtx, args: &str) -> CommandResult {
        let src = args.trim().to_ascii_lowercase();
        match src.as_str() {
            "" | "all" | "menu" => CommandResult::Action(Action::OpenImportMenu),
            "opencode" | "oc" => {
                let oc = scan_opencode(std::env::current_dir().ok().as_deref());
                CommandResult::Action(Action::ShowImportReport {
                    title: "OpenCode import".into(),
                    body: oc.summary_text(),
                })
            }
            "antigravity" | "agy" | "gemini" => {
                let agy = scan_antigravity();
                CommandResult::Action(Action::ShowImportReport {
                    title: "Antigravity import".into(),
                    body: agy.summary_text(),
                })
            }
            "claude" => CommandResult::Action(Action::OpenImportMenu),
            "ssh" | "remote" | "host" => CommandResult::Action(Action::OpenSshImportForm),
            "cursor" => {
                let d = discover_installed(&DiscoverOptions {
                    probe_local: false,
                    probe_env: false,
                    probe_binaries: false,
                    probe_sibling: true,
                    ..Default::default()
                });
                let cursor: Vec<_> = d.items.iter().filter(|i| i.id.contains("cursor")).collect();
                let body = if cursor.is_empty() {
                    "No Cursor config dirs found (~/.cursor).\n\n\
                     Skills/MCP may still load via compat if configured elsewhere."
                        .to_string()
                } else {
                    let mut msg = String::from("Cursor paths detected:\n");
                    for i in cursor {
                        msg.push_str(&format!(
                            "\n  • {}\n    {}",
                            i.label,
                            i.detail.as_deref().unwrap_or("(no detail)")
                        ));
                    }
                    msg.push_str(
                        "\n\nMCP/rules are live-discovered; model keys use env + [model.*].",
                    );
                    msg
                };
                CommandResult::Action(Action::ShowImportReport {
                    title: "Cursor import".into(),
                    body,
                })
            }
            "junie" | "jetbrains" => {
                let roots = xai_grok_shell::import::paths::junie_search_roots();
                let found: Vec<_> = roots.into_iter().filter(|p| p.exists()).collect();
                let body = if found.is_empty() {
                    "JetBrains / Junie config roots not found on this host.\n\n\
                     Prefer OpenAI-compatible env + [model.*] when available."
                        .to_string()
                } else {
                    let mut msg = String::from("JetBrains roots present:\n");
                    for p in found {
                        msg.push_str(&format!("\n  • {}", p.display()));
                    }
                    msg.push_str(
                        "\n\nJunie model import is best-effort; prefer OpenAI-compatible env + [model.*].",
                    );
                    msg
                };
                CommandResult::Action(Action::ShowImportReport {
                    title: "JetBrains / Junie import".into(),
                    body,
                })
            }
            other => CommandResult::Error(format!(
                "unknown import source {other:?}; try /import-menu or \
                 opencode|antigravity|agy|cursor|junie|claude|ssh"
            )),
        }
    }
}
