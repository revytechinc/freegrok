//! `/connect` — discover local providers and print next steps.
//!
//! Full interactive wizard lands alongside credentials store; this slash
//! surfaces discovery immediately (same data as `grok providers discover`).

use crate::slash::command::{CommandExecCtx, CommandResult, SlashCommand};
use xai_grok_shell::providers::{discover_installed, DiscoverOptions};

pub struct ConnectCommand;

impl SlashCommand for ConnectCommand {
    fn name(&self) -> &str {
        "connect"
    }

    fn aliases(&self) -> &[&str] {
        &["providers"]
    }

    fn description(&self) -> &str {
        "Discover LLM providers on this machine (local / env / tools)"
    }

    fn usage(&self) -> &str {
        "/connect"
    }

    fn run(&self, _ctx: &mut CommandExecCtx, args: &str) -> CommandResult {
        let _ = args;
        let report = discover_installed(&DiscoverOptions::default());
        let mut msg = report.summary_text();
        msg.push_str("\n\n");
        msg.push_str("CLI:\n");
        msg.push_str("  grok providers discover\n");
        msg.push_str("  grok providers validate --base-url http://127.0.0.1:11434/v1\n");
        msg.push_str("  grok providers catalog\n");
        msg.push_str("\nConfig: add [model.*] blocks (see docs user-guide 11-custom-models).\n");
        CommandResult::Message(msg)
    }
}
