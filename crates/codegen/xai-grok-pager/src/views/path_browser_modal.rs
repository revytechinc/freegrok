//! Reusable local path browser modal (files and/or directories).
//!
//! Supports hidden directories (e.g. `~/.ssh`) when `show_hidden` is set.
//! Used by the SSH import form for identity Browse; other features can
//! reuse the same component.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};
use std::path::{Path, PathBuf};

use crate::theme::Theme;
use crate::views::modal_window::{
    ModalSizing, ModalWindowConfig, ModalWindowOutcome, ModalWindowState, Shortcut,
    handle_modal_key, handle_modal_mouse, render_modal_window,
};

const SHORTCUT_ID_HINT: usize = usize::MAX;
const SHORTCUT_ID_CANCEL: usize = 0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathBrowserMode {
    Files,
    Dirs,
    Both,
}

/// Who opened the browser (dispatch routes selection).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathBrowserPurpose {
    SshIdentity,
}

#[derive(Debug, Clone)]
pub struct PathBrowserConfig {
    pub title: String,
    pub start_dir: PathBuf,
    pub mode: PathBrowserMode,
    pub show_hidden: bool,
    pub allow_toggle_hidden: bool,
    pub max_entries: usize,
}

impl PathBrowserConfig {
    /// Defaults for picking an SSH identity under `~/.ssh` (hidden on).
    pub fn ssh_identity() -> Self {
        let start = dirs::home_dir()
            .map(|h| h.join(".ssh"))
            .filter(|p| p.is_dir())
            .or_else(dirs::home_dir)
            .unwrap_or_else(|| PathBuf::from("."));
        Self {
            title: "Choose identity file".into(),
            start_dir: start,
            mode: PathBrowserMode::Files,
            show_hidden: true,
            allow_toggle_hidden: true,
            max_entries: 500,
        }
    }
}

#[derive(Debug, Clone)]
struct Entry {
    name: String,
    path: PathBuf,
    is_dir: bool,
}

pub struct PathBrowserModalState {
    pub purpose: PathBrowserPurpose,
    pub config: PathBrowserConfig,
    pub cwd: PathBuf,
    entries: Vec<Entry>,
    pub focus: usize,
    pub scroll: usize,
    pub filter: String,
    pub error: Option<String>,
    pub window: ModalWindowState,
    pub content_area: Option<Rect>,
}

impl PathBrowserModalState {
    pub fn open(purpose: PathBrowserPurpose, config: PathBrowserConfig) -> Self {
        let cwd = expand_path(&config.start_dir);
        let mut s = Self {
            purpose,
            config,
            cwd: cwd.clone(),
            entries: Vec::new(),
            focus: 0,
            scroll: 0,
            filter: String::new(),
            error: None,
            window: ModalWindowState::new(),
            content_area: None,
        };
        s.reload();
        s
    }

    fn reload(&mut self) {
        self.entries = list_entries(
            &self.cwd,
            self.config.show_hidden,
            self.config.mode,
            self.config.max_entries,
            &self.filter,
        );
        if self.focus >= self.entries.len() {
            self.focus = self.entries.len().saturating_sub(1);
        }
        self.scroll = 0;
    }

    pub fn handle_key(&mut self, key: &KeyEvent) -> PathBrowserOutcome {
        let cfg = ModalWindowConfig {
            title: "",
            tabs: None,
            shortcuts: &[],
            sizing: Default::default(),
            fold_info: None,
        };
        match handle_modal_key(&mut self.window, key, &cfg) {
            ModalWindowOutcome::CloseRequested => return PathBrowserOutcome::Cancelled,
            ModalWindowOutcome::Handled => return PathBrowserOutcome::Changed,
            ModalWindowOutcome::ShortcutActivated(id) if id == SHORTCUT_ID_CANCEL => {
                return PathBrowserOutcome::Cancelled;
            }
            _ => {}
        }

        if key.modifiers.contains(KeyModifiers::CONTROL)
            || (key.modifiers.contains(KeyModifiers::ALT)
                && !crate::input::key::is_altgr(key.modifiers))
        {
            return PathBrowserOutcome::Unchanged;
        }

        match key.code {
            KeyCode::Esc => PathBrowserOutcome::Cancelled,
            KeyCode::Up | KeyCode::Char('k') => {
                self.focus = self.focus.saturating_sub(1);
                PathBrowserOutcome::Changed
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if !self.entries.is_empty() {
                    self.focus = (self.focus + 1).min(self.entries.len() - 1);
                }
                PathBrowserOutcome::Changed
            }
            KeyCode::Left | KeyCode::Backspace if self.filter.is_empty() => {
                self.go_parent();
                PathBrowserOutcome::Changed
            }
            KeyCode::Backspace => {
                self.filter.pop();
                self.reload();
                PathBrowserOutcome::Changed
            }
            KeyCode::Right | KeyCode::Char('l') => {
                self.enter_focused_dir();
                PathBrowserOutcome::Changed
            }
            KeyCode::Enter => self.activate_focused(),
            KeyCode::Char('.') if self.config.allow_toggle_hidden && self.filter.is_empty() => {
                self.config.show_hidden = !self.config.show_hidden;
                self.reload();
                PathBrowserOutcome::Changed
            }
            KeyCode::Char('~') if self.filter.is_empty() => {
                if let Some(h) = dirs::home_dir() {
                    self.cwd = h;
                    self.filter.clear();
                    self.reload();
                }
                PathBrowserOutcome::Changed
            }
            KeyCode::Char(c) if !c.is_control() => {
                self.filter.push(c);
                self.reload();
                PathBrowserOutcome::Changed
            }
            _ => PathBrowserOutcome::Unchanged,
        }
    }

    pub fn handle_mouse(
        &mut self,
        kind: crossterm::event::MouseEventKind,
        column: u16,
        row: u16,
    ) -> PathBrowserOutcome {
        use crossterm::event::{MouseButton, MouseEventKind};
        match handle_modal_mouse(&mut self.window, kind, column, row) {
            ModalWindowOutcome::CloseRequested => return PathBrowserOutcome::Cancelled,
            ModalWindowOutcome::Handled => return PathBrowserOutcome::Changed,
            ModalWindowOutcome::ShortcutActivated(id) if id == SHORTCUT_ID_CANCEL => {
                return PathBrowserOutcome::Cancelled;
            }
            _ => {}
        }
        match kind {
            MouseEventKind::ScrollUp => {
                self.focus = self.focus.saturating_sub(1);
                PathBrowserOutcome::Changed
            }
            MouseEventKind::ScrollDown => {
                if !self.entries.is_empty() {
                    self.focus = (self.focus + 1).min(self.entries.len() - 1);
                }
                PathBrowserOutcome::Changed
            }
            MouseEventKind::Down(MouseButton::Left) => {
                if let Some(area) = self.content_area {
                    // Header lines take 2 rows (cwd + blank).
                    if column >= area.x
                        && column < area.x + area.width
                        && row >= area.y + 2
                        && row < area.y + area.height
                    {
                        let idx = (row - area.y - 2) as usize + self.scroll;
                        if idx < self.entries.len() {
                            self.focus = idx;
                            return self.activate_focused();
                        }
                    }
                }
                PathBrowserOutcome::Unchanged
            }
            _ => PathBrowserOutcome::Unchanged,
        }
    }

    fn go_parent(&mut self) {
        if let Some(p) = self.cwd.parent() {
            self.cwd = p.to_path_buf();
            self.filter.clear();
            self.error = None;
            self.reload();
        }
    }

    fn enter_focused_dir(&mut self) {
        if let Some(e) = self.entries.get(self.focus) {
            if e.is_dir {
                self.cwd = e.path.clone();
                self.filter.clear();
                self.focus = 0;
                self.error = None;
                self.reload();
            }
        }
    }

    fn activate_focused(&mut self) -> PathBrowserOutcome {
        let Some(e) = self.entries.get(self.focus).cloned() else {
            return PathBrowserOutcome::Unchanged;
        };
        if e.is_dir {
            self.cwd = e.path;
            self.filter.clear();
            self.focus = 0;
            self.error = None;
            self.reload();
            PathBrowserOutcome::Changed
        } else {
            let path = dunce::canonicalize(&e.path).unwrap_or(e.path);
            PathBrowserOutcome::Selected(path)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathBrowserOutcome {
    Selected(PathBuf),
    Cancelled,
    Changed,
    Unchanged,
}

fn expand_path(p: &Path) -> PathBuf {
    let s = p.to_string_lossy();
    if s == "~" {
        return dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    }
    if let Some(rest) = s.strip_prefix("~/") {
        if let Some(h) = dirs::home_dir() {
            return h.join(rest);
        }
    }
    p.to_path_buf()
}

fn is_hidden_name(name: &str) -> bool {
    name.starts_with('.') && name != "." && name != ".."
}

fn list_entries(
    cwd: &Path,
    show_hidden: bool,
    mode: PathBrowserMode,
    max: usize,
    filter: &str,
) -> Vec<Entry> {
    let filter_l = filter.to_ascii_lowercase();
    let mut out = Vec::new();
    let rd = match std::fs::read_dir(cwd) {
        Ok(r) => r,
        Err(_) => return out,
    };
    for ent in rd.flatten() {
        let name = ent.file_name().to_string_lossy().into_owned();
        if name == "." || name == ".." {
            continue;
        }
        if !show_hidden && is_hidden_name(&name) {
            continue;
        }
        if !filter_l.is_empty() && !name.to_ascii_lowercase().contains(&filter_l) {
            continue;
        }
        let path = ent.path();
        let is_dir = ent.file_type().map(|t| t.is_dir()).unwrap_or(false);
        match mode {
            PathBrowserMode::Files if is_dir => {
                // Still show dirs so user can navigate.
            }
            PathBrowserMode::Dirs if !is_dir => continue,
            _ => {}
        }
        // In Files mode we show dirs for navigation + files for select.
        if matches!(mode, PathBrowserMode::Files) && !is_dir {
            // ok
        } else if matches!(mode, PathBrowserMode::Dirs) && is_dir {
            // ok
        } else if matches!(mode, PathBrowserMode::Both) {
            // ok
        } else if matches!(mode, PathBrowserMode::Files) && is_dir {
            // ok navigate
        } else {
            continue;
        }
        out.push(Entry {
            name,
            path,
            is_dir,
        });
        if out.len() >= max {
            break;
        }
    }
    out.sort_by(|a, b| match (a.is_dir, b.is_dir) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.name.to_ascii_lowercase().cmp(&b.name.to_ascii_lowercase()),
    });
    out
}

pub fn render_path_browser_modal(
    buf: &mut Buffer,
    area: Rect,
    state: &mut PathBrowserModalState,
    theme: &Theme,
    compact: bool,
) {
    let hidden_hint = if state.config.allow_toggle_hidden {
        if state.config.show_hidden {
            "· hide"
        } else {
            "· show"
        }
    } else {
        ""
    };
    let shortcuts = [
        Shortcut {
            label: "↑↓ navigate",
            clickable: false,
            id: SHORTCUT_ID_HINT,
        },
        Shortcut {
            label: "Enter open/select",
            clickable: false,
            id: SHORTCUT_ID_HINT,
        },
        Shortcut {
            label: &format!(". hidden{hidden_hint}"),
            clickable: false,
            id: SHORTCUT_ID_HINT,
        },
        Shortcut {
            label: "Esc cancel",
            clickable: true,
            id: SHORTCUT_ID_CANCEL,
        },
    ];
    let title = state.config.title.clone();
    let config = ModalWindowConfig {
        title: &title,
        tabs: None,
        shortcuts: &shortcuts,
        sizing: ModalSizing::default().with_compact(compact),
        fold_info: None,
    };
    let Some(areas) = render_modal_window(buf, area, &mut state.window, &config, theme) else {
        return;
    };
    state.content_area = Some(areas.content);

    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(Span::styled(
        format!("📁 {}", state.cwd.display()),
        Style::default().fg(theme.gray_dim),
    )));
    if !state.filter.is_empty() {
        lines.push(Line::from(Span::styled(
            format!("filter: {}", state.filter),
            Style::default().fg(theme.gray_dim),
        )));
    } else {
        lines.push(Line::from(""));
    }
    if let Some(err) = &state.error {
        lines.push(Line::from(Span::styled(
            err.as_str(),
            Style::default().fg(theme.warning),
        )));
    }
    for (i, e) in state.entries.iter().enumerate() {
        let focused = i == state.focus;
        let marker = if focused { "▶ " } else { "  " };
        let icon = if e.is_dir { "📁 " } else { "📄 " };
        let style = if focused {
            Style::default()
                .fg(theme.accent_success)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme.text_primary)
        };
        let suffix = if e.is_dir { "/" } else { "" };
        lines.push(Line::from(Span::styled(
            format!("{marker}{icon}{}{suffix}", e.name),
            style,
        )));
    }
    if state.entries.is_empty() {
        lines.push(Line::from(Span::styled(
            "(empty)",
            Style::default().fg(theme.gray_dim),
        )));
    }
    Paragraph::new(lines).render(areas.content, buf);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_hidden_when_enabled() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join(".ssh")).unwrap();
        std::fs::write(dir.path().join("visible.txt"), b"x").unwrap();
        let mut cfg = PathBrowserConfig {
            title: "t".into(),
            start_dir: dir.path().to_path_buf(),
            mode: PathBrowserMode::Both,
            show_hidden: true,
            allow_toggle_hidden: true,
            max_entries: 100,
        };
        let state = PathBrowserModalState::open(PathBrowserPurpose::SshIdentity, cfg.clone());
        assert!(
            state.entries.iter().any(|e| e.name == ".ssh"),
            "entries={:?}",
            state.entries.iter().map(|e| &e.name).collect::<Vec<_>>()
        );
        cfg.show_hidden = false;
        let state2 = PathBrowserModalState::open(PathBrowserPurpose::SshIdentity, cfg);
        assert!(!state2.entries.iter().any(|e| e.name == ".ssh"));
        assert!(state2.entries.iter().any(|e| e.name == "visible.txt"));
    }

    #[test]
    fn can_enter_dot_ssh_and_see_key() {
        let dir = tempfile::tempdir().unwrap();
        let ssh = dir.path().join(".ssh");
        std::fs::create_dir(&ssh).unwrap();
        // Assemble PEM so the PII/secret gate does not treat fixtures as live keys.
        let pem = format!(
            "-----BEGIN {}-----\nx\n-----END {}-----\n",
            "OPENSSH PRIVATE KEY",
            "OPENSSH PRIVATE KEY"
        );
        std::fs::write(ssh.join("id_test"), pem).unwrap();
        let cfg = PathBrowserConfig {
            title: "t".into(),
            start_dir: dir.path().to_path_buf(),
            mode: PathBrowserMode::Files,
            show_hidden: true,
            allow_toggle_hidden: true,
            max_entries: 100,
        };
        let mut state = PathBrowserModalState::open(PathBrowserPurpose::SshIdentity, cfg);
        let idx = state.entries.iter().position(|e| e.name == ".ssh").unwrap();
        state.focus = idx;
        assert!(matches!(
            state.handle_key(&KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            PathBrowserOutcome::Changed
        ));
        assert!(state.cwd.ends_with(".ssh"));
        assert!(state.entries.iter().any(|e| e.name == "id_test"));
        let kidx = state.entries.iter().position(|e| e.name == "id_test").unwrap();
        state.focus = kidx;
        match state.handle_key(&KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)) {
            PathBrowserOutcome::Selected(p) => {
                assert!(p.ends_with("id_test") || p.file_name().unwrap() == "id_test");
            }
            other => panic!("expected Selected, got {other:?}"),
        }
    }

    #[test]
    fn esc_cancels() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = PathBrowserConfig {
            title: "t".into(),
            start_dir: dir.path().to_path_buf(),
            mode: PathBrowserMode::Both,
            show_hidden: true,
            allow_toggle_hidden: false,
            max_entries: 10,
        };
        let mut state = PathBrowserModalState::open(PathBrowserPurpose::SshIdentity, cfg);
        assert_eq!(
            state.handle_key(&KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
            PathBrowserOutcome::Cancelled
        );
    }
}
