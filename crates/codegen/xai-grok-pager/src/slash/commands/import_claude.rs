//! Back-compat module path for the Claude import slash command.
//!
//! The canonical command is [`super::import_menu::ImportMenuCommand`]
//! (`/import-menu`). `/import-claude` is an alias on that command.

pub use super::import_menu::ImportMenuCommand as ImportClaudeCommand;
