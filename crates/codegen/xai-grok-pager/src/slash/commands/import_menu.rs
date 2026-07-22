//! `/import-menu` — open the multi-source import menu.
//!
//! Always shows a source picker (Claude, OpenCode, Antigravity, Cursor,
//! Junie). Claude then opens the interactive checklist after a background
//! scan; other sources show a discovery summary.
//!
//! `/import-claude` remains an alias for muscle memory.
//! Text scan without a UI: `/import [source]`.

use crate::app::actions::Action;
use crate::slash::command::{CommandExecCtx, CommandResult, SlashCommand};

/// Open the interactive import source menu (welcome + in-session).
pub struct ImportMenuCommand;

impl SlashCommand for ImportMenuCommand {
    fn name(&self) -> &str {
        "import-menu"
    }

    /// Legacy spelling; opens the same menu as `/import-menu`.
    fn aliases(&self) -> &[&str] {
        &["import-claude"]
    }

    fn description(&self) -> &str {
        "Open the import menu (Claude checklist, OpenCode, Cursor, …)"
    }

    fn usage(&self) -> &str {
        "/import-menu"
    }

    fn run(&self, _ctx: &mut CommandExecCtx, args: &str) -> CommandResult {
        let trimmed = args.trim();
        if !trimmed.is_empty() {
            tracing::warn!("/import-menu does not accept arguments; ignoring");
        }
        CommandResult::Action(Action::OpenImportMenu)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::slash::commands::{builtin_commands, import_cmd};
    use crate::slash::registry::CommandRegistry;

    #[test]
    fn canonical_name_is_import_menu() {
        let cmd = ImportMenuCommand;
        assert_eq!(cmd.name(), "import-menu");
        assert_eq!(cmd.aliases(), &["import-claude"]);
        assert_eq!(cmd.usage(), "/import-menu");
    }

    #[test]
    fn registry_resolves_import_menu_and_legacy_alias() {
        let reg = CommandRegistry::new(builtin_commands());
        let by_menu = reg.get("import-menu").expect("/import-menu registered");
        let by_alias = reg
            .get("import-claude")
            .expect("/import-claude alias registered");
        assert_eq!(by_menu.name(), "import-menu");
        assert_eq!(by_alias.name(), "import-menu");
    }

    #[test]
    fn run_dispatches_import_action() {
        use crate::acp::model_state::ModelState;
        use crate::slash::command::CommandExecCtx;
        use std::sync::Arc;

        let models = ModelState::default();
        let bundle = crate::app::bundle::BundleState {
            has_cache: false,
            version: String::new(),
            personas: Vec::new(),
            roles: Vec::new(),
            agents: Vec::new(),
            skills: Vec::new(),
            persona_details: Vec::new(),
            role_details: Vec::new(),
        };
        let mut ctx = CommandExecCtx {
            models: &models,
            session_id: None,
            bundle_state: &bundle,
            screen_mode: crate::app::ScreenMode::Inline,
            pager_state: crate::settings::PagerLocalSnapshot::default(),
        };
        match ImportMenuCommand.run(&mut ctx, "") {
            CommandResult::Action(Action::OpenImportMenu) => {}
            other => panic!("expected OpenImportMenu action, got {other:?}"),
        }
        // Silence unused import if import_cmd is only for docs consistency.
        let _ = import_cmd::ImportCommand.name();
        let _ = Arc::new(());
    }
}
