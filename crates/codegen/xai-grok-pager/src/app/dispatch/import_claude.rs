//! Import menu + Claude settings checklist + import report dispatchers.

use crate::app::actions::Effect;
use crate::app::app_view::AppView;
use crate::views::import_menu_modal::ImportSource;
use crate::views::import_report_modal::ImportReportModalState;

/// Open the multi-source import menu (always visible; no scan yet).
pub(super) fn dispatch_open_import_menu(app: &mut AppView) -> Vec<Effect> {
    if app.import_claude_scanning || app.import_claude_applying {
        notify_import_status(app, "Import already in progress…");
        return vec![];
    }
    app.import_claude_modal = None;
    app.import_report_modal = None;
    app.ssh_import_form = None;
    app.path_browser = None;
    app.import_menu_modal = Some(crate::views::import_menu_modal::ImportMenuModalState::new());
    vec![]
}

pub(super) fn dispatch_import_menu_cancel(app: &mut AppView) -> Vec<Effect> {
    app.import_menu_modal = None;
    vec![]
}

pub(super) fn dispatch_import_report_close(app: &mut AppView) -> Vec<Effect> {
    app.import_report_modal = None;
    vec![]
}

pub(super) fn dispatch_show_import_report(
    app: &mut AppView,
    title: String,
    body: String,
) -> Vec<Effect> {
    show_import_report(app, title, body);
    vec![]
}

/// Handle a source pick from the import menu.
pub(super) fn dispatch_import_menu_select(
    app: &mut AppView,
    source: ImportSource,
) -> Vec<Effect> {
    app.import_menu_modal = None;
    match source {
        ImportSource::Ssh => dispatch_open_ssh_import_form(app),
        ImportSource::Claude => dispatch_import_claude(app),
        ImportSource::OpenCode => {
            let oc = xai_grok_shell::import::scan_opencode(Some(app.cwd.as_path()));
            show_import_report(app, "OpenCode import", oc.summary_text());
            vec![]
        }
        ImportSource::Antigravity => {
            let agy = xai_grok_shell::import::scan_antigravity();
            show_import_report(app, "Antigravity import", agy.summary_text());
            vec![]
        }
        ImportSource::Cursor => {
            let d = xai_grok_shell::providers::discover_installed(
                &xai_grok_shell::providers::DiscoverOptions {
                    probe_local: false,
                    probe_env: false,
                    probe_binaries: false,
                    probe_sibling: true,
                    ..Default::default()
                },
            );
            let cursor: Vec<_> = d.items.iter().filter(|i| i.id.contains("cursor")).collect();
            let msg = if cursor.is_empty() {
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
            show_import_report(app, "Cursor import", msg);
            vec![]
        }
        ImportSource::Junie => {
            let roots = xai_grok_shell::import::paths::junie_search_roots();
            let found: Vec<_> = roots.into_iter().filter(|p| p.exists()).collect();
            let msg = if found.is_empty() {
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
            show_import_report(app, "JetBrains / Junie import", msg);
            vec![]
        }
    }
}

/// Short, single-line status (toast). Never for multi-line discovery dumps.
fn notify_import_status(app: &mut AppView, message: &str) {
    app.show_toast(message);
}

pub(super) fn dispatch_open_ssh_import_form(app: &mut AppView) -> Vec<Effect> {
    app.import_menu_modal = None;
    app.import_report_modal = None;
    app.path_browser = None;
    app.ssh_import_form =
        Some(crate::views::ssh_import_form_modal::SshImportFormModalState::new());
    vec![]
}

pub(super) fn dispatch_ssh_import_form_cancel(app: &mut AppView) -> Vec<Effect> {
    app.ssh_import_form = None;
    app.path_browser = None;
    vec![]
}

pub(super) fn dispatch_ssh_import_browse_identity(app: &mut AppView) -> Vec<Effect> {
    use crate::views::path_browser_modal::{PathBrowserConfig, PathBrowserModalState, PathBrowserPurpose};
    app.path_browser = Some(PathBrowserModalState::open(
        PathBrowserPurpose::SshIdentity,
        PathBrowserConfig::ssh_identity(),
    ));
    vec![]
}

pub(super) fn dispatch_path_browser_cancel(app: &mut AppView) -> Vec<Effect> {
    app.path_browser = None;
    vec![]
}

pub(super) fn dispatch_path_browser_selected(
    app: &mut AppView,
    purpose: crate::views::path_browser_modal::PathBrowserPurpose,
    path: std::path::PathBuf,
) -> Vec<Effect> {
    app.path_browser = None;
    match purpose {
        crate::views::path_browser_modal::PathBrowserPurpose::SshIdentity => {
            if let Some(form) = app.ssh_import_form.as_mut() {
                if let Err(reason) = form.set_identity_from_browse(path) {
                    notify_import_status(app, &format!("Identity rejected: {reason}"));
                } else {
                    notify_import_status(app, "Identity file set");
                }
            }
        }
    }
    vec![]
}

/// Phase 1 submit: validate + show a modal summary of what would be used
/// (destination, user, identity). Full remote scan is a later phase.
pub(super) fn dispatch_ssh_import_form_submit(
    app: &mut AppView,
    values: crate::views::ssh_import_form_modal::SshImportFormValues,
) -> Vec<Effect> {
    use xai_grok_shell::import::{auth_diagnostics, SshAuth, SshSession};

    app.ssh_import_form = None;
    app.path_browser = None;

    let mut session = SshSession::new(values.destination.clone());
    // Only pass -l when Login was explicit; empty → Host user@… / ssh config / default.
    if let Some(u) = &values.user {
        session = session.user(u.clone());
    }
    if let Some(p) = values.port {
        session = session.port(p);
    }
    if let Some(id) = &values.identity {
        session = session.identity(id.clone());
    }

    let mut body = String::new();
    body.push_str("Import from remote host (ready to scan — remote scan is next):\n\n");
    body.push_str(&format!("  Host:     {}\n", values.destination));
    body.push_str(&format!(
        "  Login:    {}\n",
        values
            .user
            .as_deref()
            .unwrap_or("(from Host user@… / ssh config / OpenSSH default)")
    ));
    body.push_str(&format!(
        "  Port:     {}\n",
        values
            .port
            .map(|p| p.to_string())
            .unwrap_or_else(|| "(from ~/.ssh/config / default)".into())
    ));
    body.push_str(&format!(
        "  Identity: {}\n\n",
        values
            .identity
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "(agent / config IdentityFile)".into())
    ));
    body.push_str("OpenSSH diagnostics:\n");
    body.push_str(&auth_diagnostics(&session.target, &session.auth));
    body.push_str(
        "\n\nNext: remote path scan will use these settings. \
         Run `grok-build config ssh-probe <host> --ping` to verify auth from the CLI.",
    );

    // Drop any password material if it were ever set (phase 1 has none).
    let _ = SshAuth::keys_and_config();

    show_import_report(app, "Import config from another host", body);
    vec![]
}

/// Multi-line import results go into a scrollable modal — never the welcome
/// banner or a toast blob that reflows the whole TUI.
fn show_import_report(app: &mut AppView, title: impl Into<String>, body: impl AsRef<str>) {
    app.import_menu_modal = None;
    app.import_claude_modal = None;
    app.import_report_modal = Some(ImportReportModalState::new(title, body));
}

/// Scan Claude settings and open the checklist modal.
///
/// Scanning walks `.claude/` trees and can block for a long time on large
/// home/project dirs, so the actual scan runs as [`Effect::ScanClaudeImport`]
/// on a blocking worker.
pub(super) fn dispatch_import_claude(app: &mut AppView) -> Vec<Effect> {
    if app.import_claude_scanning || app.import_claude_applying {
        notify_import_status(app, "Claude import already in progress…");
        return vec![];
    }
    app.import_menu_modal = None;
    app.import_claude_modal = None;
    app.import_report_modal = None;
    app.import_claude_scanning = true;
    let cwd = app.cwd.clone();
    notify_import_status(app, "Scanning Claude settings…");
    vec![Effect::ScanClaudeImport { cwd }]
}

/// Apply a completed scan: open the checklist, or report empty + reopen menu.
pub(super) fn handle_claude_import_scanned(
    app: &mut AppView,
    plan: xai_grok_shell::claude_import::ImportPlan,
    cwd: std::path::PathBuf,
) -> Vec<Effect> {
    app.import_claude_scanning = false;

    if plan.is_empty() {
        xai_grok_shell::claude_import_state::mark_dismissed(&cwd);
        if let Err(e) = xai_grok_shell::claude_import::mark_claude_imported() {
            tracing::warn!(error = %e, "Failed to write Claude import marker");
        }
        app.has_claude_import = false;
        notify_import_status(
            app,
            "No Claude settings found (no ~/.claude or project .claude)",
        );
        app.import_menu_modal = Some(crate::views::import_menu_modal::ImportMenuModalState::new());
        return vec![];
    }

    app.import_report_modal = None;
    app.import_claude_modal =
        Some(crate::views::import_claude_modal::ImportClaudeModalState::new(plan, cwd));
    vec![]
}

/// Apply the user's selection from the checklist and close it.
pub(super) fn dispatch_import_claude_confirm(app: &mut AppView) -> Vec<Effect> {
    if app.import_claude_applying || app.import_claude_scanning {
        return vec![];
    }
    let Some(modal) = app.import_claude_modal.take() else {
        return vec![];
    };
    let cwd = modal.cwd.clone();
    let total_in_modal = modal.total_count();
    let filtered = modal.filtered_plan();
    let selected_count = filtered.global_items.len() + filtered.project_items.len();

    if selected_count == 0 {
        finish_claude_import_success(app, &cwd, "No items selected.".to_string());
        return vec![];
    }

    app.import_claude_applying = true;
    notify_import_status(app, "Importing Claude settings…");
    vec![Effect::ApplyClaudeImport {
        plan: filtered,
        cwd,
        total_in_modal,
    }]
}

/// Finish apply after the background worker completes.
pub(super) fn handle_claude_import_applied(
    app: &mut AppView,
    summary: String,
    error: Option<String>,
    cwd: std::path::PathBuf,
) -> Vec<Effect> {
    app.import_claude_applying = false;

    if let Some(err) = error {
        show_import_report(
            app,
            "Claude import failed",
            format!("Failed to import Claude settings:\n\n{err}"),
        );
        return vec![];
    }

    finish_claude_import_success(app, &cwd, summary);
    vec![]
}

fn finish_claude_import_success(app: &mut AppView, cwd: &std::path::Path, summary: String) {
    xai_grok_shell::claude_import_state::mark_imported(cwd);
    if let Err(e) = xai_grok_shell::claude_import::mark_claude_imported() {
        tracing::warn!(error = %e, "Failed to write Claude import marker");
    }
    app.has_claude_import = false;
    // Multi-line apply summary → report modal; one-liner → toast.
    if summary.lines().count() > 1 || summary.len() > 80 {
        show_import_report(app, "Claude import complete", summary);
    } else {
        notify_import_status(app, &summary);
    }
}

/// Cancel the Claude checklist without applying.
pub(super) fn dispatch_import_claude_cancel(app: &mut AppView) -> Vec<Effect> {
    app.import_claude_modal = None;
    vec![]
}

/// Hide the welcome import menu row (hash dismiss).
pub(super) fn dispatch_dismiss_claude_import(app: &mut AppView) -> Vec<Effect> {
    let cwd = app.cwd.clone();
    xai_grok_shell::claude_import_state::mark_dismissed(&cwd);
    if let Err(e) = xai_grok_shell::claude_import::mark_claude_imported() {
        tracing::warn!(error = %e, "Failed to write Claude import marker on dismiss");
    }
    app.has_claude_import = false;
    app.welcome_menu_index = None;
    vec![]
}
