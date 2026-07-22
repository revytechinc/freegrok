//! Red/green tests for the Claude import dispatch chain.
//!
//! Import scan/apply must never run on the UI/dispatch thread. These tests
//! drive Action → Effect → TaskResult with mock plans (and one effect-level
//! scan against a mock `.claude/` tree) so the freeze regression is covered.

use super::*;
use std::path::Path;
use xai_grok_shell::claude_import::{ImportPlan, ImportableItem, PathKind};
use xai_grok_workspace::permission::types::{PatternMode, PermissionRule, RuleAction, ToolFilter};

fn mock_plan() -> ImportPlan {
    ImportPlan {
        global_items: vec![],
        project_items: vec![
            ImportableItem::Permission(PermissionRule {
                action: RuleAction::Allow,
                tool: ToolFilter::Bash,
                pattern: Some("npm test".into()),
                pattern_mode: PatternMode::Glob,
            }),
            ImportableItem::EnvVar {
                key: "MOCK_IMPORT_VAR".into(),
                value: "from-claude".into(),
            },
            ImportableItem::PathEntry {
                kind: PathKind::Skill,
                path: "/tmp/mock/.claude/skills".into(),
            },
        ],
    }
}

#[test]
fn open_import_menu_always_shows_source_picker() {
    let mut app = test_app();
    let effects = dispatch(Action::OpenImportMenu, &mut app);
    assert!(effects.is_empty());
    assert!(
        app.import_menu_modal.is_some(),
        "import menu must open even with no Claude configs"
    );
    assert!(app.import_claude_modal.is_none());
}

#[test]
fn import_menu_select_claude_dispatches_scan_effect_not_checklist() {
    let mut app = test_app();
    app.has_claude_import = true;
    app.cwd = PathBuf::from("/tmp/mock-project");
    app.import_menu_modal = Some(crate::views::import_menu_modal::ImportMenuModalState::new());

    let effects = dispatch(
        Action::ImportMenuSelect(crate::views::import_menu_modal::ImportSource::Claude),
        &mut app,
    );

    assert!(app.import_menu_modal.is_none(), "source menu closes on select");
    assert!(
        app.import_claude_scanning,
        "scan must be marked in-flight so the UI can show progress"
    );
    assert!(
        app.import_claude_modal.is_none(),
        "checklist must not open synchronously — that freezes the TUI on large trees"
    );
    assert!(
        matches!(
            effects.as_slice(),
            [Effect::ScanClaudeImport { cwd }] if cwd == Path::new("/tmp/mock-project")
        ),
        "expected ScanClaudeImport effect, got {effects:?}"
    );
    assert!(
        app.import_report_modal.is_none(),
        "scanning must not dump a report modal"
    );
}

#[test]
fn import_settings_dispatches_scan_effect_not_modal() {
    let mut app = test_app();
    app.has_claude_import = true;
    app.cwd = PathBuf::from("/tmp/mock-project");

    let effects = dispatch(Action::ImportClaudeSettings, &mut app);

    assert!(
        app.import_claude_scanning,
        "scan must be marked in-flight so the UI can show progress"
    );
    assert!(
        app.import_claude_modal.is_none(),
        "modal must not open synchronously — that freezes the TUI on large trees"
    );
    assert!(
        matches!(
            effects.as_slice(),
            [Effect::ScanClaudeImport { cwd }] if cwd == Path::new("/tmp/mock-project")
        ),
        "expected ScanClaudeImport effect, got {effects:?}"
    );
}

#[test]
fn import_settings_while_scanning_is_noop() {
    let mut app = test_app();
    app.import_claude_scanning = true;

    let effects = dispatch(Action::ImportClaudeSettings, &mut app);
    assert!(effects.is_empty(), "re-entrant import must not spawn a second scan");
}

#[test]
fn scanned_plan_opens_modal() {
    let mut app = test_app();
    app.import_claude_scanning = true;
    app.cwd = PathBuf::from("/tmp/mock-project");
    let plan = mock_plan();
    let total = plan.total_items();

    let effects = dispatch(
        Action::TaskComplete(TaskResult::ClaudeImportScanned {
            plan,
            cwd: PathBuf::from("/tmp/mock-project"),
        }),
        &mut app,
    );

    assert!(effects.is_empty());
    assert!(!app.import_claude_scanning);
    let modal = app
        .import_claude_modal
        .as_ref()
        .expect("modal must open after scan completes");
    assert_eq!(modal.total_count(), total);
    assert_eq!(modal.cwd, PathBuf::from("/tmp/mock-project"));
    assert!(
        !app.startup_warnings
            .iter()
            .any(|w| w.message.contains("Scanning Claude")),
        "scanning status must clear when modal opens"
    );
}

#[test]
fn scanned_empty_plan_reopens_source_menu() {
    let mut app = test_app();
    app.import_claude_scanning = true;
    app.has_claude_import = true;

    let _ = dispatch(
        Action::TaskComplete(TaskResult::ClaudeImportScanned {
            plan: ImportPlan::default(),
            cwd: app.cwd.clone(),
        }),
        &mut app,
    );

    assert!(!app.import_claude_scanning);
    assert!(app.import_claude_modal.is_none());
    assert!(
        app.import_menu_modal.is_some(),
        "empty Claude scan must return user to the source menu, not a blank screen"
    );
    assert!(
        app.import_report_modal.is_none(),
        "empty Claude scan must not dump multi-line into a report; toast + menu only"
    );
    assert!(!app.has_claude_import);
}

#[test]
fn confirm_dispatches_apply_effect() {
    let mut app = test_app();
    let plan = mock_plan();
    let total = plan.total_items();
    app.import_claude_modal = Some(
        crate::views::import_claude_modal::ImportClaudeModalState::new(
            plan,
            PathBuf::from("/tmp/mock-project"),
        ),
    );

    let effects = dispatch(Action::ImportClaudeConfirm, &mut app);

    assert!(app.import_claude_modal.is_none(), "modal closes on confirm");
    assert!(app.import_claude_applying);
    assert!(
        matches!(
            effects.as_slice(),
            [Effect::ApplyClaudeImport {
                total_in_modal,
                cwd,
                plan,
            }] if *total_in_modal == total
                && cwd == Path::new("/tmp/mock-project")
                && plan.project_items.len() == total
        ),
        "expected ApplyClaudeImport, got {effects:?}"
    );
}

#[test]
fn confirm_with_nothing_selected_finishes_sync() {
    let mut app = test_app();
    let plan = mock_plan();
    let mut modal = crate::views::import_claude_modal::ImportClaudeModalState::new(
        plan,
        PathBuf::from("/tmp/mock-project"),
    );
    for sel in modal.selected.iter_mut() {
        *sel = false;
    }
    app.import_claude_modal = Some(modal);
    app.has_claude_import = true;

    let effects = dispatch(Action::ImportClaudeConfirm, &mut app);

    assert!(effects.is_empty(), "no apply work when nothing selected");
    assert!(!app.import_claude_applying);
    assert!(!app.has_claude_import);
}

#[test]
fn applied_success_clears_busy_and_records_summary() {
    let mut app = test_app();
    app.import_claude_applying = true;
    app.has_claude_import = true;

    let effects = dispatch(
        Action::TaskComplete(TaskResult::ClaudeImportApplied {
            summary: "Imported 3 of 3 setting(s).".into(),
            error: None,
            cwd: PathBuf::from("/tmp/mock-project"),
        }),
        &mut app,
    );

    assert!(effects.is_empty());
    assert!(!app.import_claude_applying);
    assert!(!app.has_claude_import);
    // Short one-liner → toast only (no report modal clutter).
    assert!(app.import_report_modal.is_none());
}

#[test]
fn applied_multiline_summary_opens_report_modal() {
    let mut app = test_app();
    app.import_claude_applying = true;

    let _ = dispatch(
        Action::TaskComplete(TaskResult::ClaudeImportApplied {
            summary: "Found Claude settings to import:\n\nImported 2 of 3 setting(s).\n  Updated: /tmp/x".into(),
            error: None,
            cwd: PathBuf::from("/tmp/mock-project"),
        }),
        &mut app,
    );

    let report = app
        .import_report_modal
        .as_ref()
        .expect("multi-line apply summary must open report modal");
    assert!(report.title.contains("Claude"));
    assert!(report.lines.iter().any(|l| l.contains("Imported 2 of 3")));
}

#[test]
fn applied_error_opens_report_modal() {
    let mut app = test_app();
    app.import_claude_applying = true;

    let _ = dispatch(
        Action::TaskComplete(TaskResult::ClaudeImportApplied {
            summary: String::new(),
            error: Some("disk full".into()),
            cwd: PathBuf::from("/tmp/mock-project"),
        }),
        &mut app,
    );

    assert!(!app.import_claude_applying);
    let report = app
        .import_report_modal
        .as_ref()
        .expect("apply error must open report modal");
    assert!(
        report.lines.iter().any(|l| l.contains("disk full")),
        "error body missing: {:?}",
        report.lines
    );
}

#[test]
fn ssh_menu_opens_form_with_autodiscovered_user() {
    let mut app = test_app();
    let effects = dispatch(
        Action::ImportMenuSelect(crate::views::import_menu_modal::ImportSource::Ssh),
        &mut app,
    );
    assert!(effects.is_empty());
    let form = app
        .ssh_import_form
        .as_ref()
        .expect("SSH source must open form modal");
    // Login is optional and starts empty; local user is a soft hint only.
    assert!(
        form.user.is_empty(),
        "Login as must not be prefilled with local user"
    );
    assert!(
        !form.hints.summary_lines().is_empty(),
        "form must show local discoveries"
    );
}

#[test]
fn ssh_form_submit_opens_report_not_scrollback() {
    let mut app = test_app();
    app.ssh_import_form = Some(crate::views::ssh_import_form_modal::SshImportFormModalState::new());
    let values = crate::views::ssh_import_form_modal::SshImportFormValues {
        destination: "mybox".into(),
        user: Some("deploy".into()),
        port: Some(22),
        identity: None,
    };
    let effects = dispatch(Action::SshImportFormSubmit(values), &mut app);
    assert!(effects.is_empty());
    assert!(app.ssh_import_form.is_none());
    let report = app
        .import_report_modal
        .as_ref()
        .expect("submit must open report modal");
    assert!(
        report.lines.iter().any(|l| l.contains("mybox") || l.contains("deploy")),
        "report should mention host/user: {:?}",
        report.lines
    );
}

#[test]
fn opencode_select_opens_report_modal_not_banner() {
    let mut app = test_app();
    app.cwd = PathBuf::from("/tmp/no-opencode-here");
    let effects = dispatch(
        Action::ImportMenuSelect(crate::views::import_menu_modal::ImportSource::OpenCode),
        &mut app,
    );
    assert!(effects.is_empty());
    assert!(app.import_menu_modal.is_none());
    let report = app
        .import_report_modal
        .as_ref()
        .expect("OpenCode scan must open import report modal");
    assert!(
        report.title.contains("OpenCode"),
        "title={:?}",
        report.title
    );
    assert!(
        !report.lines.is_empty(),
        "report body must not be empty"
    );
    // Must not pollute welcome banner with multi-line dump.
    assert!(
        app.startup_warnings.is_empty()
            || app
                .startup_warnings
                .iter()
                .all(|w| !w.message.contains("OpenCode import scan")),
        "OpenCode dump must not land in startup_warnings: {:?}",
        app.startup_warnings
    );
}

#[test]
fn cancel_closes_modal_without_apply() {
    let mut app = test_app();
    app.import_claude_modal = Some(
        crate::views::import_claude_modal::ImportClaudeModalState::new(
            mock_plan(),
            PathBuf::from("/tmp/mock-project"),
        ),
    );

    let effects = dispatch(Action::ImportClaudeCancel, &mut app);
    assert!(effects.is_empty());
    assert!(app.import_claude_modal.is_none());
    assert!(!app.import_claude_applying);
}

#[tokio::test]
async fn scan_effect_runs_off_thread_against_mock_claude_tree() {
    use crate::app::effects::{SessionFlags, execute};
    use std::path::Path;
    use tokio::task::JoinSet;

    let root = tempfile::tempdir().unwrap();
    git2::Repository::init(root.path()).unwrap();
    let claude = root.path().join(".claude");
    std::fs::create_dir_all(&claude).unwrap();
    std::fs::write(
        claude.join("settings.json"),
        r#"{ "permissions": { "allow": ["Bash(npm test)"] }, "env": { "E2E": "1" } }"#,
    )
    .unwrap();

    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let (progress_tx, _progress_rx) = tokio::sync::mpsc::unbounded_channel();
    let mut tasks = JoinSet::new();
    execute(
        Effect::ScanClaudeImport {
            cwd: root.path().to_path_buf(),
        },
        &mut tasks,
        &tx,
        Path::new("."),
        &SessionFlags::default(),
        &progress_tx,
    );

    match tasks.join_next().await.expect("task").expect("no panic") {
        TaskResult::ClaudeImportScanned { plan, cwd } => {
            assert_eq!(cwd, root.path());
            assert!(
                !plan.project_items.is_empty(),
                "effect scan must find mock project items, got {plan:?}"
            );
            assert!(
                plan.project_items.iter().any(|i| matches!(
                    i,
                    ImportableItem::EnvVar { key, value }
                        if key == "E2E" && value == "1"
                )),
                "mock env missing from effect scan: {:?}",
                plan.project_items
            );
        }
        other => panic!("expected ClaudeImportScanned, got {other:?}"),
    }
}

#[tokio::test]
async fn apply_effect_writes_project_config_from_mock_plan() {
    use crate::app::effects::{SessionFlags, execute};
    use std::path::Path;
    use tokio::task::JoinSet;

    let root = tempfile::tempdir().unwrap();
    git2::Repository::init(root.path()).unwrap();

    let plan = ImportPlan {
        global_items: vec![],
        project_items: vec![
            ImportableItem::EnvVar {
                key: "APPLY_E2E".into(),
                value: "ok".into(),
            },
            ImportableItem::Permission(PermissionRule {
                action: RuleAction::Allow,
                tool: ToolFilter::Bash,
                pattern: Some("true".into()),
                pattern_mode: PatternMode::Glob,
            }),
        ],
    };
    let total = plan.total_items();

    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let (progress_tx, _progress_rx) = tokio::sync::mpsc::unbounded_channel();
    let mut tasks = JoinSet::new();
    execute(
        Effect::ApplyClaudeImport {
            plan,
            cwd: root.path().to_path_buf(),
            total_in_modal: total,
        },
        &mut tasks,
        &tx,
        Path::new("."),
        &SessionFlags::default(),
        &progress_tx,
    );

    match tasks.join_next().await.expect("task").expect("no panic") {
        TaskResult::ClaudeImportApplied {
            summary,
            error,
            cwd,
        } => {
            assert_eq!(cwd, root.path());
            assert!(error.is_none(), "apply must succeed: {error:?}");
            assert!(
                summary.contains("Imported") && summary.contains(&total.to_string()),
                "summary should report counts: {summary}"
            );
            let config = std::fs::read_to_string(root.path().join(".grok/config.toml"))
                .expect("project config written by apply effect");
            assert!(
                config.contains("APPLY_E2E") && config.contains("ok"),
                "apply effect must write env into project config:\n{config}"
            );
        }
        other => panic!("expected ClaudeImportApplied, got {other:?}"),
    }
}
