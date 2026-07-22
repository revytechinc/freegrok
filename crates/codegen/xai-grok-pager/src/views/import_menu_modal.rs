//! Source-picker modal for `/import-menu` (alias `/import-claude`).
//!
//! Always opens — even when no Claude tree exists — so the user sees a real
//! menu. Selecting a source either launches the Claude checklist scanner or
//! surfaces a text discovery summary for the other tools.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};

use crate::theme::Theme;
use crate::views::modal_window::{
    ModalSizing, ModalWindowConfig, ModalWindowOutcome, ModalWindowState, Shortcut,
    handle_modal_key, handle_modal_mouse, render_modal_window,
};

const SHORTCUT_ID_HINT: usize = usize::MAX;
const SHORTCUT_ID_CONFIRM: usize = 0;
const SHORTCUT_ID_CANCEL: usize = 1;

/// Import source shown in the menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportSource {
    /// Interactive Claude settings checklist (async scan).
    Claude,
    /// OpenCode discovery summary.
    OpenCode,
    /// Antigravity / Gemini discovery summary.
    Antigravity,
    /// Cursor path discovery.
    Cursor,
    /// JetBrains / Junie roots.
    Junie,
    /// SSH host form (user, identity, local key discoveries).
    Ssh,
}

impl ImportSource {
    pub fn label(self) -> &'static str {
        match self {
            Self::Claude => "Claude settings",
            Self::OpenCode => "OpenCode",
            Self::Antigravity => "Antigravity / Gemini",
            Self::Cursor => "Cursor",
            Self::Junie => "JetBrains / Junie",
            Self::Ssh => "Import config from another host",
        }
    }

    pub fn detail(self) -> &'static str {
        match self {
            Self::Claude => "permissions, env, MCP, hooks, skill/rule paths",
            Self::OpenCode => "scan config (text summary)",
            Self::Antigravity => "scan config (text summary)",
            Self::Cursor => "detect ~/.cursor paths",
            Self::Junie => "detect JetBrains config roots",
            Self::Ssh => "SSH; scan Claude/OpenCode/… on a remote machine",
        }
    }

    fn all() -> &'static [ImportSource] {
        &[
            Self::Claude,
            Self::OpenCode,
            Self::Antigravity,
            Self::Cursor,
            Self::Junie,
            Self::Ssh,
        ]
    }
}

/// State for the import source menu.
pub struct ImportMenuModalState {
    pub focus: usize,
    pub window: ModalWindowState,
    pub content_area: Option<Rect>,
}

impl ImportMenuModalState {
    pub fn new() -> Self {
        Self {
            focus: 0,
            window: ModalWindowState::new(),
            content_area: None,
        }
    }

    pub fn focused_source(&self) -> ImportSource {
        ImportSource::all()[self.focus.min(ImportSource::all().len() - 1)]
    }

    pub fn handle_key(&mut self, key: &KeyEvent) -> ImportMenuOutcome {
        let config = ModalWindowConfig {
            title: "",
            tabs: None,
            shortcuts: &[],
            sizing: Default::default(),
            fold_info: None,
        };
        match handle_modal_key(&mut self.window, key, &config) {
            ModalWindowOutcome::CloseRequested => return ImportMenuOutcome::Cancelled,
            ModalWindowOutcome::Handled => return ImportMenuOutcome::Changed,
            ModalWindowOutcome::ShortcutActivated(id) => {
                return self.dispatch_shortcut(id);
            }
            _ => {}
        }

        if (key.modifiers.contains(KeyModifiers::CONTROL)
            || key.modifiers.contains(KeyModifiers::ALT))
            && !crate::input::key::is_altgr(key.modifiers)
        {
            return ImportMenuOutcome::Unchanged;
        }

        match key.code {
            KeyCode::Esc => ImportMenuOutcome::Cancelled,
            KeyCode::Enter => ImportMenuOutcome::Selected(self.focused_source()),
            KeyCode::Up | KeyCode::Char('k') => {
                if self.focus > 0 {
                    self.focus -= 1;
                }
                ImportMenuOutcome::Changed
            }
            KeyCode::Down | KeyCode::Char('j') => {
                let max = ImportSource::all().len().saturating_sub(1);
                if self.focus < max {
                    self.focus += 1;
                }
                ImportMenuOutcome::Changed
            }
            KeyCode::Char('1') => self.select_index(0),
            KeyCode::Char('2') => self.select_index(1),
            KeyCode::Char('3') => self.select_index(2),
            KeyCode::Char('4') => self.select_index(3),
            KeyCode::Char('5') => self.select_index(4),
            KeyCode::Char('6') => self.select_index(5),
            _ => ImportMenuOutcome::Unchanged,
        }
    }

    pub fn handle_mouse(
        &mut self,
        kind: crossterm::event::MouseEventKind,
        column: u16,
        row: u16,
    ) -> ImportMenuOutcome {
        use crossterm::event::{MouseButton, MouseEventKind};

        match handle_modal_mouse(&mut self.window, kind, column, row) {
            ModalWindowOutcome::CloseRequested => return ImportMenuOutcome::Cancelled,
            ModalWindowOutcome::Handled => return ImportMenuOutcome::Changed,
            ModalWindowOutcome::ShortcutActivated(id) => {
                return self.dispatch_shortcut(id);
            }
            _ => {}
        }

        if let MouseEventKind::Down(MouseButton::Left) = kind
            && let Some(area) = self.content_area
            && column >= area.x
            && column < area.x + area.width
            && row >= area.y
            && row < area.y + area.height
        {
            // Row 0 is the intro line; sources start at row 1.
            let idx = (row - area.y) as usize;
            if idx >= 1 {
                let src_idx = idx - 1;
                if src_idx < ImportSource::all().len() {
                    self.focus = src_idx;
                    return ImportMenuOutcome::Selected(self.focused_source());
                }
            }
        }
        ImportMenuOutcome::Unchanged
    }

    fn dispatch_shortcut(&mut self, id: usize) -> ImportMenuOutcome {
        match id {
            SHORTCUT_ID_CONFIRM => ImportMenuOutcome::Selected(self.focused_source()),
            SHORTCUT_ID_CANCEL => ImportMenuOutcome::Cancelled,
            _ => ImportMenuOutcome::Unchanged,
        }
    }

    fn select_index(&mut self, idx: usize) -> ImportMenuOutcome {
        if idx < ImportSource::all().len() {
            self.focus = idx;
            ImportMenuOutcome::Selected(self.focused_source())
        } else {
            ImportMenuOutcome::Unchanged
        }
    }
}

impl Default for ImportMenuModalState {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportMenuOutcome {
    Cancelled,
    Selected(ImportSource),
    Changed,
    Unchanged,
}

pub fn render_import_menu_modal(
    buf: &mut Buffer,
    area: Rect,
    state: &mut ImportMenuModalState,
    theme: &Theme,
    compact: bool,
) {
    let shortcuts = [
        Shortcut {
            label: "\u{2191}\u{2193} navigate",
            clickable: false,
            id: SHORTCUT_ID_HINT,
        },
        Shortcut {
            label: "1-6 jump",
            clickable: false,
            id: SHORTCUT_ID_HINT,
        },
        Shortcut {
            label: "Enter open",
            clickable: true,
            id: SHORTCUT_ID_CONFIRM,
        },
        Shortcut {
            label: "Esc cancel",
            clickable: true,
            id: SHORTCUT_ID_CANCEL,
        },
    ];
    let config = ModalWindowConfig {
        title: "Import settings",
        tabs: None,
        shortcuts: &shortcuts,
        sizing: ModalSizing::default().with_compact(compact),
        fold_info: None,
    };

    let Some(areas) = render_modal_window(buf, area, &mut state.window, &config, theme) else {
        return;
    };
    let content_area = areas.content;
    state.content_area = Some(content_area);

    let sources = ImportSource::all();
    let mut lines: Vec<Line> = Vec::with_capacity(sources.len() + 1);
    lines.push(Line::from(Span::styled(
        "Choose a source to import from:",
        Style::default().fg(theme.gray_dim),
    )));

    for (i, src) in sources.iter().enumerate() {
        let focused = i == state.focus;
        let marker = if focused { "\u{25b6} " } else { "  " };
        let num = format!("{}. ", i + 1);
        let style = if focused {
            Style::default()
                .fg(theme.accent_success)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme.text_primary)
        };
        let detail_style = Style::default().fg(theme.gray_dim);
        lines.push(Line::from(vec![
            Span::styled(format!("{marker}{num}{}", src.label()), style),
            Span::styled(format!("  — {}", src.detail()), detail_style),
        ]));
    }

    let para = Paragraph::new(lines);
    para.render(content_area, buf);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn enter_selects_focused_claude_by_default() {
        let mut m = ImportMenuModalState::new();
        assert_eq!(
            m.handle_key(&key(KeyCode::Enter)),
            ImportMenuOutcome::Selected(ImportSource::Claude)
        );
    }

    #[test]
    fn down_moves_focus_then_enter_selects_opencode() {
        let mut m = ImportMenuModalState::new();
        assert_eq!(m.handle_key(&key(KeyCode::Down)), ImportMenuOutcome::Changed);
        assert_eq!(
            m.handle_key(&key(KeyCode::Enter)),
            ImportMenuOutcome::Selected(ImportSource::OpenCode)
        );
    }

    #[test]
    fn digit_jumps_and_selects() {
        let mut m = ImportMenuModalState::new();
        assert_eq!(
            m.handle_key(&key(KeyCode::Char('3'))),
            ImportMenuOutcome::Selected(ImportSource::Antigravity)
        );
    }

    #[test]
    fn esc_cancels() {
        let mut m = ImportMenuModalState::new();
        assert_eq!(
            m.handle_key(&key(KeyCode::Esc)),
            ImportMenuOutcome::Cancelled
        );
    }
}
