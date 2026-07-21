//! Curated TUI regression suite for isolated product runs.
//!
//! Run via `scripts/tui-regression.sh` (or `make tui-regression`), which sets a
//! synthetic `HOME` / `GROK_HOME` and points `PAGER_BINARY` at a built pager.
//! These tests stay `#[ignore]` so ordinary `cargo test` stays fast.
//!
//! Tiers (env `TUI_TIER`, default `smoke`):
//! - **smoke** — boot, mock turn, resize, slash, shortcuts (CI / installed reg)
//! - **standard** — smoke + trust, hints, selection, compact, vim palette
//! - **full** — standard + heavier UI (inline edit, paste chips, mermaid, …)
//!
//! Override the exact YAML list with `TUI_SCENARIO_FILES="a.yaml b.yaml"`.
//! Stop on first failure with `TUI_FAIL_FAST=1`.

use std::path::PathBuf;
use std::time::Instant;

use xai_grok_pager_pty_harness::{
    ScriptedRunConfig, ScriptedRunStatus, ScriptedScenario, ScriptedScenarioRunner, pager_binary,
};

fn scenario_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("scenarios")
        .join(name)
}

/// Core smoke — default for installed regression (stable, fast-ish).
/// Prefer scenarios that assert structural UI (sentinels, gates), not
/// copy-brittle help prose that churns with product copy.
const TIER_SMOKE: &[&str] = &[
    "welcome.yaml",
    "mock_response.yaml",
    "bracket_prompt_input.yaml",
    "slash_resize_storm.yaml",
    "goal_slash_presession.yaml",
    "goal_slash_presession_disabled.yaml",
    "release_notes_scroll.yaml",
    "plan_nudge_opt_out_no_show.yaml",
];

/// Standard product surface.
const TIER_STANDARD: &[&str] = &[
    "welcome.yaml",
    "mock_response.yaml",
    "bracket_prompt_input.yaml",
    "slash_resize_storm.yaml",
    "goal_slash_presession.yaml",
    "goal_slash_presession_disabled.yaml",
    "release_notes_scroll.yaml",
    "plan_nudge_opt_out_no_show.yaml",
    "folder_trust_prompt.yaml",
    "vim_modal_command_palette.yaml",
    "copy_selection.yaml",
    "undo_tip_clear_shows.yaml",
    "undo_tip_short_draft_no_show.yaml",
    "plan_nudge_shows.yaml",
    "auto_compact_resize.yaml",
    "small_screen_tip_band.yaml",
    "small_screen_tip_no_show_tall.yaml",
    "ansi_execute_output.yaml",
    "path_space_hyperlink.yaml",
    // Copy-sensitive; kept in standard so smoke stays green when help text moves.
    "shortcuts_help_detail.yaml",
];

/// Heavier scenarios appended for `TUI_TIER=full`.
const TIER_FULL_EXTRA: &[&str] = &[
    "dashboard_model_list_click.yaml",
    "tool_header_path_selection.yaml",
    "paste_chip_double_click.yaml",
    "paste_chip_repaste.yaml",
    "table_cell_selection.yaml",
    "input_modalities.yaml",
    "mermaid-affordances.yaml",
    "mid_text_skill_token_echo.yaml",
    "inline_edit_resubmit.yaml",
    "inline_edit_unchanged_exit.yaml",
    "inline_edit_dismiss_returns_to_editor.yaml",
    "undo_tip_ctrl_c_clear_shows.yaml",
    "undo_tip_short_terminal_no_show.yaml",
    "small_screen_tip_no_show_tiny.yaml",
    "image_chip_preview_path_free.yaml",
    "image_normalize_corrupt_dropped.yaml",
];

fn scenario_files_for_tier(tier: &str) -> Vec<&'static str> {
    match tier {
        "smoke" | "" => TIER_SMOKE.to_vec(),
        "standard" | "std" => TIER_STANDARD.to_vec(),
        "full" => {
            let mut v = TIER_STANDARD.to_vec();
            v.extend_from_slice(TIER_FULL_EXTRA);
            v
        }
        other => panic!(
            "unknown TUI_TIER={other:?}; use smoke | standard | full (or TUI_SCENARIO_FILES=…)"
        ),
    }
}

fn selected_scenarios() -> Vec<String> {
    if let Ok(raw) = std::env::var("TUI_SCENARIO_FILES") {
        let files: Vec<String> = raw
            .split_whitespace()
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .collect();
        if !files.is_empty() {
            return files;
        }
    }
    let tier = std::env::var("TUI_TIER").unwrap_or_else(|_| "smoke".into());
    scenario_files_for_tier(tier.trim())
        .into_iter()
        .map(str::to_string)
        .collect()
}

async fn run_one(name: &str) -> Result<(), String> {
    let path = scenario_path(name);
    if !path.is_file() {
        return Err(format!("scenario file missing: {}", path.display()));
    }
    let scenario = ScriptedScenario::from_file(&path).map_err(|e| format!("load {name}: {e}"))?;
    let artifact_dir = std::env::var_os("CARGO_TARGET_TMPDIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("tui-regression-suite")
        .join(name.trim_end_matches(".yaml"));
    let _ = std::fs::create_dir_all(&artifact_dir);
    let binary = pager_binary().map_err(|e| format!("pager binary: {e}"))?;
    let runner = ScriptedScenarioRunner::new(ScriptedRunConfig::new(binary, artifact_dir));
    let started = Instant::now();
    let report = runner
        .run(&scenario)
        .await
        .map_err(|e| format!("run {name}: {e}"))?;
    let ms = started.elapsed().as_millis();
    eprintln!(
        "tui_regression: {name} status={:?} bugs={} steps={} {ms}ms",
        report.status,
        report.bugs.len(),
        report.steps.len(),
    );
    if report.status != ScriptedRunStatus::Passed {
        return Err(format!(
            "scenario {name} status={:?} bugs={:#?}",
            report.status, report.bugs
        ));
    }
    if !report.bugs.is_empty() {
        return Err(format!("scenario {name} bugs: {:#?}", report.bugs));
    }
    Ok(())
}

/// Single entry point used by `scripts/tui-regression.sh`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "isolated product TUI suite; run via make tui-regression / scripts/tui-regression.sh"]
async fn tui_regression_curated() {
    let files = selected_scenarios();
    assert!(
        !files.is_empty(),
        "no scenarios selected (TUI_TIER / TUI_SCENARIO_FILES)"
    );
    eprintln!(
        "tui_regression_curated: {} scenario(s) tier={:?} files={:?}",
        files.len(),
        std::env::var("TUI_TIER").ok(),
        files
    );
    let fail_fast = std::env::var("TUI_FAIL_FAST").ok().as_deref() == Some("1");
    let mut failed: Vec<String> = Vec::new();
    for name in &files {
        match run_one(name).await {
            Ok(()) => {}
            Err(msg) => {
                eprintln!("tui_regression FAIL {name}: {msg}");
                failed.push(format!("{name}: {msg}"));
                if fail_fast {
                    break;
                }
            }
        }
    }
    assert!(
        failed.is_empty(),
        "TUI regression failures ({}):\n  {}",
        failed.len(),
        failed.join("\n  ")
    );
}

/// Parse check for every scenario the suite might run (no PTY).
#[test]
fn tui_regression_scenario_files_parse() {
    let mut names: Vec<&str> = TIER_SMOKE.to_vec();
    names.extend_from_slice(TIER_STANDARD);
    names.extend_from_slice(TIER_FULL_EXTRA);
    names.sort_unstable();
    names.dedup();
    for name in names {
        let path = scenario_path(name);
        let scenario =
            ScriptedScenario::from_file(&path).unwrap_or_else(|e| panic!("parse {name}: {e}"));
        assert!(!scenario.steps.is_empty(), "{name} has no steps");
    }
}
