//! Scrollable report modal for import discovery / apply results.
//!
//! Multi-line import output (OpenCode scan, Cursor paths, Claude apply
//! summary, …) must live here — never dumped into the welcome banner or a
//! full-screen toast blob.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget, Wrap};

use crate::theme::Theme;
use crate::views::modal_window::{
    ModalSizing, ModalWindowConfig, ModalWindowOutcome, ModalWindowState, Shortcut,
    handle_modal_key, handle_modal_mouse, render_modal_window,
};

const SHORTCUT_ID_HINT: usize = usize::MAX;
const SHORTCUT_ID_CLOSE: usize = 0;

/// Read-only multi-line import report.
pub struct ImportReportModalState {
    pub title: String,
    pub lines: Vec<String>,
    pub scroll: u16,
    pub window: ModalWindowState,
}

impl ImportReportModalState {
    pub fn new(title: impl Into<String>, body: impl AsRef<str>) -> Self {
        let lines: Vec<String> = body
            .as_ref()
            .lines()
            .map(|l| l.to_string())
            .collect();
        Self {
            title: title.into(),
            lines,
            scroll: 0,
            window: ModalWindowState::new(),
        }
    }

    pub fn handle_key(&mut self, key: &KeyEvent) -> ImportReportOutcome {
        let config = ModalWindowConfig {
            title: "",
            tabs: None,
            shortcuts: &[],
            sizing: Default::default(),
            fold_info: None,
        };
        match handle_modal_key(&mut self.window, key, &config) {
            ModalWindowOutcome::CloseRequested => return ImportReportOutcome::Closed,
            ModalWindowOutcome::Handled => return ImportReportOutcome::Changed,
            ModalWindowOutcome::ShortcutActivated(id) if id == SHORTCUT_ID_CLOSE => {
                return ImportReportOutcome::Closed;
            }
            _ => {}
        }

        if (key.modifiers.contains(KeyModifiers::CONTROL)
            || key.modifiers.contains(KeyModifiers::ALT))
            && !crate::input::key::is_altgr(key.modifiers)
        {
            return ImportReportOutcome::Unchanged;
        }

        match key.code {
            KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q') => ImportReportOutcome::Closed,
            KeyCode::Up | KeyCode::Char('k') => {
                self.scroll = self.scroll.saturating_sub(1);
                ImportReportOutcome::Changed
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.scroll = self.scroll.saturating_add(1).min(self.max_scroll());
                ImportReportOutcome::Changed
            }
            KeyCode::PageUp => {
                self.scroll = self.scroll.saturating_sub(8);
                ImportReportOutcome::Changed
            }
            KeyCode::PageDown => {
                self.scroll = self.scroll.saturating_add(8).min(self.max_scroll());
                ImportReportOutcome::Changed
            }
            KeyCode::Home => {
                self.scroll = 0;
                ImportReportOutcome::Changed
            }
            KeyCode::End => {
                self.scroll = self.max_scroll();
                ImportReportOutcome::Changed
            }
            _ => ImportReportOutcome::Unchanged,
        }
    }

    pub fn handle_mouse(
        &mut self,
        kind: crossterm::event::MouseEventKind,
        column: u16,
        row: u16,
    ) -> ImportReportOutcome {
        use crossterm::event::MouseEventKind;

        match handle_modal_mouse(&mut self.window, kind, column, row) {
            ModalWindowOutcome::CloseRequested => return ImportReportOutcome::Closed,
            ModalWindowOutcome::Handled => return ImportReportOutcome::Changed,
            ModalWindowOutcome::ShortcutActivated(id) if id == SHORTCUT_ID_CLOSE => {
                return ImportReportOutcome::Closed;
            }
            _ => {}
        }

        match kind {
            MouseEventKind::ScrollUp => {
                self.scroll = self.scroll.saturating_sub(1);
                ImportReportOutcome::Changed
            }
            MouseEventKind::ScrollDown => {
                self.scroll = self.scroll.saturating_add(1).min(self.max_scroll());
                ImportReportOutcome::Changed
            }
            _ => ImportReportOutcome::Unchanged,
        }
    }

    fn max_scroll(&self) -> u16 {
        // Keep a little room so the last lines stay visible.
        self.lines.len().saturating_sub(4) as u16
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportReportOutcome {
    Closed,
    Changed,
    Unchanged,
}

pub fn render_import_report_modal(
    buf: &mut Buffer,
    area: Rect,
    state: &mut ImportReportModalState,
    theme: &Theme,
    compact: bool,
) {
    let shortcuts = [
        Shortcut {
            label: "\u{2191}\u{2193} scroll",
            clickable: false,
            id: SHORTCUT_ID_HINT,
        },
        Shortcut {
            label: "Enter / Esc close",
            clickable: true,
            id: SHORTCUT_ID_CLOSE,
        },
    ];
    // Title is owned by state — render_modal_window needs a short-lived str.
    let title = state.title.clone();
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

    let content_style = Style::default().fg(theme.text_primary);
    let body: Vec<Line> = state
        .lines
        .iter()
        .map(|l| Line::from(Span::styled(l.as_str(), content_style)))
        .collect();
    Paragraph::new(body)
        .wrap(Wrap { trim: false })
        .scroll((state.scroll, 0))
        .render(areas.content, buf);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn esc_and_enter_close() {
        let mut m = ImportReportModalState::new("OpenCode", "line one\nline two");
        assert_eq!(m.handle_key(&key(KeyCode::Esc)), ImportReportOutcome::Closed);
        let mut m = ImportReportModalState::new("OpenCode", "line one");
        assert_eq!(
            m.handle_key(&key(KeyCode::Enter)),
            ImportReportOutcome::Closed
        );
    }

    #[test]
    fn scroll_clamps() {
        let mut m = ImportReportModalState::new(
            "Report",
            "a\nb\nc\nd\ne\nf\ng\nh\ni\nj",
        );
        assert_eq!(m.handle_key(&key(KeyCode::Down)), ImportReportOutcome::Changed);
        assert!(m.scroll >= 1);
        m.scroll = 999;
        assert_eq!(m.handle_key(&key(KeyCode::Down)), ImportReportOutcome::Changed);
        assert_eq!(m.scroll, m.max_scroll());
    }
}
