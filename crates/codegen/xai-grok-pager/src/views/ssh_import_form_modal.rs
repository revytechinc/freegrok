//! SSH import form: destination like OpenSSH, optional login override, identity browse.
//!
//! Product model (matches `ssh` / the plan):
//! - **Host** — required. Host alias, hostname, or `user@host` (same as `ssh <destination>`).
//! - **Login as** — optional. Empty means: use user embedded in Host (`user@…`), else
//!   `User` from `~/.ssh/config`, else OpenSSH default. Never required; never a file field.
//! - **Port** — defaults to 22; clear to defer to config.
//! - **Identity file** — optional; only field that is browsable.
//!
//! Local OS username is a soft *hint* in the discoveries panel only — not a forced
//! remote login (local account often differs from the remote account).

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};
use std::path::PathBuf;
use xai_grok_shell::import::{
    discover_ssh_import_hints, validate_ssh_identity_file, SshImportHints,
};

use crate::theme::Theme;
use crate::views::modal_window::{
    ModalSizing, ModalWindowConfig, ModalWindowOutcome, ModalWindowState, Shortcut,
    handle_modal_key, handle_modal_mouse, render_modal_window,
};

const SHORTCUT_ID_HINT: usize = usize::MAX;
const SHORTCUT_ID_SCAN: usize = 0;
const SHORTCUT_ID_CANCEL: usize = 1;

/// Label drawn only on the Identity row (the sole browsable field).
const BROWSE_CHIP: &str = "[Browse]";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Field {
    Destination,
    User,
    Port,
    Identity,
}

impl Field {
    fn next(self) -> Self {
        match self {
            Self::Destination => Self::User,
            Self::User => Self::Port,
            Self::Port => Self::Identity,
            Self::Identity => Self::Destination,
        }
    }

    fn prev(self) -> Self {
        match self {
            Self::Destination => Self::Identity,
            Self::User => Self::Destination,
            Self::Port => Self::User,
            Self::Identity => Self::Port,
        }
    }
}

/// SSH import form state.
pub struct SshImportFormModalState {
    pub destination: String,
    /// Optional login override. Empty = Host `user@…` / ssh config / OpenSSH default.
    pub user: String,
    pub port: String,
    pub identity: String,
    /// Index into `hints.identity_candidates` when a shortlist key is selected;
    /// `None` if custom/browse path or empty.
    pub identity_pick: Option<usize>,
    pub hints: SshImportHints,
    field: Field,
    pub error: Option<String>,
    pub window: ModalWindowState,
    pub content_area: Option<Rect>,
    /// Screen rect of the Identity-row `[Browse]` chip (set during render).
    pub identity_browse_hit: Option<Rect>,
}

impl SshImportFormModalState {
    /// Build form with local discoveries. Login starts empty (not forced to local user).
    pub fn new() -> Self {
        let hints = discover_ssh_import_hints();
        let identity_pick = if hints.identity_candidates.len() == 1 {
            Some(0)
        } else {
            None
        };
        let identity = identity_pick
            .and_then(|i| hints.identity_candidates.get(i))
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        Self {
            destination: String::new(),
            // Empty on purpose: remote login ≠ local OS user. Soft hint is in
            // `hints.summary_lines()` only.
            user: String::new(),
            // OpenSSH default; user can clear for Host-config Port.
            port: "22".into(),
            identity,
            identity_pick,
            hints,
            field: Field::Destination,
            error: None,
            window: ModalWindowState::new(),
            content_area: None,
            identity_browse_hit: None,
        }
    }

    /// Apply a browsed identity path after validation.
    pub fn set_identity_from_browse(&mut self, path: PathBuf) -> Result<(), String> {
        match validate_ssh_identity_file(&path) {
            xai_grok_shell::import::IdentityValidation::Ok => {
                self.identity = path.display().to_string();
                self.identity_pick = None;
                self.error = None;
                Ok(())
            }
            xai_grok_shell::import::IdentityValidation::Reject { reason } => {
                self.error = Some(format!("Identity rejected: {reason}"));
                Err(reason)
            }
        }
    }

    /// Cycle to next default key (or clear).
    pub fn cycle_identity_candidate(&mut self) {
        if self.hints.identity_candidates.is_empty() {
            self.identity.clear();
            self.identity_pick = None;
            return;
        }
        let next = match self.identity_pick {
            None => 0,
            Some(i) if i + 1 < self.hints.identity_candidates.len() => i + 1,
            Some(_) => {
                // After last → clear (use agent/config only)
                self.identity_pick = None;
                self.identity.clear();
                return;
            }
        };
        self.identity_pick = Some(next);
        self.identity = self.hints.identity_candidates[next]
            .display()
            .to_string();
        self.error = None;
    }

    pub fn handle_key(&mut self, key: &KeyEvent) -> SshImportFormOutcome {
        let config = ModalWindowConfig {
            title: "",
            tabs: None,
            shortcuts: &[],
            sizing: Default::default(),
            fold_info: None,
        };
        match handle_modal_key(&mut self.window, key, &config) {
            ModalWindowOutcome::CloseRequested => return SshImportFormOutcome::Cancelled,
            ModalWindowOutcome::Handled => return SshImportFormOutcome::Changed,
            ModalWindowOutcome::ShortcutActivated(id) => {
                return self.dispatch_shortcut(id);
            }
            _ => {}
        }

        if key.modifiers.contains(KeyModifiers::CONTROL)
            || (key.modifiers.contains(KeyModifiers::ALT)
                && !crate::input::key::is_altgr(key.modifiers))
        {
            // Browse only while focused on the Identity file field.
            if self.field == Field::Identity
                && key.modifiers.contains(KeyModifiers::CONTROL)
                && key.code == KeyCode::Char('b')
            {
                return SshImportFormOutcome::BrowseIdentity;
            }
            if self.field == Field::Identity
                && key.modifiers.contains(KeyModifiers::ALT)
                && matches!(key.code, KeyCode::Char('n') | KeyCode::Char('N'))
            {
                self.cycle_identity_candidate();
                return SshImportFormOutcome::Changed;
            }
            return SshImportFormOutcome::Unchanged;
        }

        match key.code {
            KeyCode::Esc => SshImportFormOutcome::Cancelled,
            KeyCode::Tab | KeyCode::Down => {
                self.field = self.field.next();
                SshImportFormOutcome::Changed
            }
            KeyCode::BackTab | KeyCode::Up => {
                self.field = self.field.prev();
                SshImportFormOutcome::Changed
            }
            KeyCode::Enter => self.try_submit(),
            // F2 browse only on the identity field (not host/user/port).
            KeyCode::F(2) if self.field == Field::Identity => {
                SshImportFormOutcome::BrowseIdentity
            }
            KeyCode::Char(c) if !c.is_control() => {
                self.push_char(c);
                SshImportFormOutcome::Changed
            }
            KeyCode::Backspace => {
                self.pop_char();
                SshImportFormOutcome::Changed
            }
            _ => SshImportFormOutcome::Unchanged,
        }
    }

    pub fn handle_mouse(
        &mut self,
        kind: crossterm::event::MouseEventKind,
        column: u16,
        row: u16,
    ) -> SshImportFormOutcome {
        use crossterm::event::{MouseButton, MouseEventKind};

        // Click the Identity-row [Browse] chip only.
        if let MouseEventKind::Down(MouseButton::Left) = kind {
            if let Some(hit) = self.identity_browse_hit {
                if column >= hit.x
                    && column < hit.x.saturating_add(hit.width)
                    && row >= hit.y
                    && row < hit.y.saturating_add(hit.height)
                {
                    return SshImportFormOutcome::BrowseIdentity;
                }
            }
        }

        match handle_modal_mouse(&mut self.window, kind, column, row) {
            ModalWindowOutcome::CloseRequested => SshImportFormOutcome::Cancelled,
            ModalWindowOutcome::Handled => SshImportFormOutcome::Changed,
            ModalWindowOutcome::ShortcutActivated(id) => self.dispatch_shortcut(id),
            _ => SshImportFormOutcome::Unchanged,
        }
    }

    fn dispatch_shortcut(&mut self, id: usize) -> SshImportFormOutcome {
        match id {
            SHORTCUT_ID_SCAN => self.try_submit(),
            SHORTCUT_ID_CANCEL => SshImportFormOutcome::Cancelled,
            _ => SshImportFormOutcome::Unchanged,
        }
    }

    fn active_buf_mut(&mut self) -> &mut String {
        match self.field {
            Field::Destination => &mut self.destination,
            Field::User => &mut self.user,
            Field::Port => &mut self.port,
            Field::Identity => &mut self.identity,
        }
    }

    fn push_char(&mut self, c: char) {
        if self.field == Field::Identity {
            self.identity_pick = None;
        }
        self.active_buf_mut().push(c);
        self.error = None;
    }

    fn pop_char(&mut self) {
        if self.field == Field::Identity {
            self.identity_pick = None;
        }
        self.active_buf_mut().pop();
        self.error = None;
    }

    fn try_submit(&mut self) -> SshImportFormOutcome {
        let dest_raw = self.destination.trim();
        if dest_raw.is_empty() {
            self.error = Some("Host is required (alias, hostname, or user@host)".into());
            self.field = Field::Destination;
            return SshImportFormOutcome::Changed;
        }
        let port = self.port.trim();
        if !port.is_empty() && port.parse::<u16>().is_err() {
            self.error = Some("Port must be a number (1–65535) or empty".into());
            self.field = Field::Port;
            return SshImportFormOutcome::Changed;
        }
        let identity = self.identity.trim();
        if !identity.is_empty() {
            let path = expand_tilde(identity);
            match validate_ssh_identity_file(&path) {
                xai_grok_shell::import::IdentityValidation::Ok => {}
                xai_grok_shell::import::IdentityValidation::Reject { reason } => {
                    self.error = Some(format!("Identity invalid: {reason}"));
                    self.field = Field::Identity;
                    return SshImportFormOutcome::Changed;
                }
            }
        }

        let (destination, user) = resolve_ssh_endpoint(dest_raw, self.user.trim());
        SshImportFormOutcome::Submit(SshImportFormValues {
            destination,
            user,
            port: port.parse().ok(),
            identity: if identity.is_empty() {
                None
            } else {
                Some(expand_tilde(identity))
            },
        })
    }
}

impl Default for SshImportFormModalState {
    fn default() -> Self {
        Self::new()
    }
}

/// Submitted form values ready for `SshSession`.
///
/// `user` is `None` when Login was left empty so OpenSSH resolves from
/// `user@host` in destination and/or `~/.ssh/config`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SshImportFormValues {
    pub destination: String,
    pub user: Option<String>,
    pub port: Option<u16>,
    pub identity: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SshImportFormOutcome {
    Cancelled,
    /// Open path browser for identity.
    BrowseIdentity,
    Submit(SshImportFormValues),
    Changed,
    Unchanged,
}

/// Map form Host + Login into OpenSSH destination + optional `-l` user.
///
/// - Login empty → pass Host through unchanged (supports aliases and `user@host`).
/// - Login set → use it as `-l`; if Host is `someone@host`, strip the embedded
///   user so we do not double-specify conflicting logins.
pub fn resolve_ssh_endpoint(host: &str, login: &str) -> (String, Option<String>) {
    let host = host.trim();
    let login = login.trim();
    if login.is_empty() {
        return (host.to_string(), None);
    }
    let dest = match split_user_at_host(host) {
        Some((_embedded, hostname)) => hostname.to_string(),
        None => host.to_string(),
    };
    (dest, Some(login.to_string()))
}

/// Split `user@host` on the first `@`. Returns `None` for aliases / bare hostnames.
fn split_user_at_host(s: &str) -> Option<(&str, &str)> {
    let (user, host) = s.split_once('@')?;
    if user.is_empty() || host.is_empty() || host.contains('@') {
        return None;
    }
    Some((user, host))
}

fn expand_tilde(s: &str) -> PathBuf {
    if s == "~" {
        return dirs::home_dir().unwrap_or_else(|| PathBuf::from(s));
    }
    if let Some(rest) = s.strip_prefix("~/") {
        if let Some(h) = dirs::home_dir() {
            return h.join(rest);
        }
    }
    PathBuf::from(s)
}

pub fn render_ssh_import_form_modal(
    buf: &mut Buffer,
    area: Rect,
    state: &mut SshImportFormModalState,
    theme: &Theme,
    compact: bool,
) {
    // No global Browse shortcut — only on the Identity row chip below.
    let shortcuts = [
        Shortcut {
            label: "Tab fields",
            clickable: false,
            id: SHORTCUT_ID_HINT,
        },
        Shortcut {
            label: "Enter scan",
            clickable: true,
            id: SHORTCUT_ID_SCAN,
        },
        Shortcut {
            label: "Esc cancel",
            clickable: true,
            id: SHORTCUT_ID_CANCEL,
        },
    ];
    let config = ModalWindowConfig {
        title: "Import config from another host",
        tabs: None,
        shortcuts: &shortcuts,
        sizing: ModalSizing::default().with_compact(compact),
        fold_info: None,
    };
    let Some(areas) = render_modal_window(buf, area, &mut state.window, &config, theme) else {
        return;
    };
    state.content_area = Some(areas.content);
    state.identity_browse_hit = None;

    let label = |active: bool| {
        if active {
            Style::default()
                .fg(theme.accent_success)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme.gray_dim)
        }
    };
    let value_style = Style::default().fg(theme.text_primary);
    let placeholder_style = Style::default().fg(theme.gray_dim);
    let chip_style = Style::default()
        .fg(theme.accent_success)
        .add_modifier(Modifier::BOLD);

    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(Span::styled(
        "Pull config from another machine over SSH.",
        Style::default().fg(theme.gray_dim),
    )));
    lines.push(Line::from(""));

    // Host: plain text; placeholder shows OpenSSH destination shape.
    {
        let active = state.field == Field::Destination;
        let cursor = if active { "▌" } else { "" };
        if state.destination.is_empty() {
            lines.push(Line::from(vec![
                Span::styled(format!("{:14}", "Host"), label(active)),
                Span::styled(
                    format!("alias, hostname, or user@host{cursor}"),
                    placeholder_style,
                ),
            ]));
        } else {
            lines.push(Line::from(vec![
                Span::styled(format!("{:14}", "Host"), label(active)),
                Span::styled(format!("{}{cursor}", state.destination), value_style),
            ]));
        }
    }

    // Login as: optional override — empty is valid.
    {
        let active = state.field == Field::User;
        let cursor = if active { "▌" } else { "" };
        if state.user.is_empty() {
            lines.push(Line::from(vec![
                Span::styled(format!("{:14}", "Login as"), label(active)),
                Span::styled(
                    format!("(optional — Host user@… / ssh config){cursor}"),
                    placeholder_style,
                ),
            ]));
        } else {
            lines.push(Line::from(vec![
                Span::styled(format!("{:14}", "Login as"), label(active)),
                Span::styled(format!("{}{cursor}", state.user), value_style),
            ]));
        }
    }

    lines.push({
        let active = state.field == Field::Port;
        let cursor = if active { "▌" } else { "" };
        Line::from(vec![
            Span::styled(format!("{:14}", "Port"), label(active)),
            Span::styled(format!("{}{cursor}", state.port), value_style),
        ])
    });

    // Identity: only browsable field — chip lives on this row.
    let id_active = state.field == Field::Identity;
    let id_display = if state.identity.is_empty() {
        "(use agent / ssh config)"
    } else {
        state.identity.as_str()
    };
    let cursor = if id_active { "▌" } else { "" };
    let id_prefix = format!("{name:14}", name = "Identity file");
    let id_value = format!("{id_display}{cursor}  ");
    lines.push(Line::from(vec![
        Span::styled(id_prefix.clone(), label(id_active)),
        Span::styled(id_value.clone(), value_style),
        Span::styled(BROWSE_CHIP.to_string(), chip_style),
        if id_active {
            Span::styled(
                "  F2 / click".to_string(),
                Style::default().fg(theme.gray_dim),
            )
        } else {
            Span::raw("")
        },
    ]));

    if let Some(err) = &state.error {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            err.as_str(),
            Style::default().fg(theme.warning),
        )));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Detected on this machine:",
        Style::default()
            .fg(theme.text_primary)
            .add_modifier(Modifier::BOLD),
    )));
    for l in state.hints.summary_lines() {
        lines.push(Line::from(Span::styled(
            l,
            Style::default().fg(theme.gray_dim),
        )));
    }

    Paragraph::new(lines).render(areas.content, buf);

    // Hit-test for Identity-row [Browse] only.
    // Content rows: 0=intro, 1=blank, 2=Host, 3=Login, 4=Port, 5=Identity.
    let identity_row_y = areas.content.y.saturating_add(5);
    if identity_row_y < areas.content.y.saturating_add(areas.content.height) {
        use unicode_width::UnicodeWidthStr;
        let chip_start = (id_prefix.width() + id_value.width()) as u16;
        let chip_w = BROWSE_CHIP.width() as u16;
        if chip_start < areas.content.width {
            state.identity_browse_hit = Some(Rect {
                x: areas.content.x.saturating_add(chip_start),
                y: identity_row_y,
                width: chip_w.min(areas.content.width.saturating_sub(chip_start)),
                height: 1,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn new_form_does_not_force_local_user_as_remote_login() {
        let form = SshImportFormModalState::new();
        // Login must start empty — local OS user is only a soft hint in the panel.
        assert!(
            form.user.is_empty(),
            "Login as must not be prefilled with local user; got {:?}",
            form.user
        );
        // Soft hint may still appear in discoveries when OS exposes a user.
        if form.hints.default_user.is_some() {
            let summary = form.hints.summary_lines().join("\n");
            assert!(
                summary.to_ascii_lowercase().contains("login")
                    || summary.to_ascii_lowercase().contains("user"),
                "hint panel should still mention suggested login when known"
            );
        }
    }

    #[test]
    fn new_form_defaults_port_to_22() {
        let form = SshImportFormModalState::new();
        assert_eq!(form.port, "22");
    }

    #[test]
    fn submit_requires_host_only() {
        let mut form = SshImportFormModalState::new();
        form.destination.clear();
        form.user.clear();
        assert!(matches!(
            form.try_submit(),
            SshImportFormOutcome::Changed
        ));
        assert!(form.error.as_ref().unwrap().contains("Host"));

        // Host alone is enough — Login optional.
        form.destination = "mybox".into();
        form.user.clear();
        match form.try_submit() {
            SshImportFormOutcome::Submit(v) => {
                assert_eq!(v.destination, "mybox");
                assert_eq!(v.user, None);
            }
            other => panic!("expected Submit with optional login, got {other:?}"),
        }
    }

    #[test]
    fn submit_accepts_user_at_host_without_login_field() {
        let mut form = SshImportFormModalState::new();
        form.destination = "deploy@prod.example".into();
        form.user.clear();
        form.identity.clear();
        match form.try_submit() {
            SshImportFormOutcome::Submit(v) => {
                assert_eq!(v.destination, "deploy@prod.example");
                assert_eq!(v.user, None, "embedded user lives in destination");
            }
            other => panic!("expected Submit, got {other:?}"),
        }
    }

    #[test]
    fn submit_login_overrides_and_strips_embedded_user() {
        let mut form = SshImportFormModalState::new();
        form.destination = "alice@box".into();
        form.user = "bob".into();
        form.identity.clear();
        match form.try_submit() {
            SshImportFormOutcome::Submit(v) => {
                assert_eq!(v.destination, "box");
                assert_eq!(v.user.as_deref(), Some("bob"));
            }
            other => panic!("expected Submit, got {other:?}"),
        }
    }

    #[test]
    fn resolve_ssh_endpoint_unit() {
        assert_eq!(
            resolve_ssh_endpoint("mybox", ""),
            ("mybox".into(), None)
        );
        assert_eq!(
            resolve_ssh_endpoint("alice@host", ""),
            ("alice@host".into(), None)
        );
        assert_eq!(
            resolve_ssh_endpoint("alice@host", "bob"),
            ("host".into(), Some("bob".into()))
        );
        assert_eq!(
            resolve_ssh_endpoint("host", "bob"),
            ("host".into(), Some("bob".into()))
        );
    }

    #[test]
    fn submit_rejects_invalid_identity_without_panic() {
        let dir = tempfile::tempdir().unwrap();
        let mp4 = dir.path().join("x.mp4");
        let mut bytes = vec![0u8; 32];
        bytes[4..8].copy_from_slice(b"ftyp");
        fs::write(&mp4, bytes).unwrap();

        let mut form = SshImportFormModalState::new();
        form.destination = "host".into();
        form.user.clear();
        form.identity = mp4.display().to_string();
        assert!(matches!(form.try_submit(), SshImportFormOutcome::Changed));
        assert!(
            form.error.as_ref().unwrap().contains("Identity"),
            "{:?}",
            form.error
        );
    }

    #[test]
    fn set_identity_from_browse_validates() {
        let dir = tempfile::tempdir().unwrap();
        let key = dir.path().join("id_ed25519");
        // Assemble PEM so the PII/secret gate does not treat fixtures as live keys.
        let pem = format!(
            "-----BEGIN {}-----\nabc\n-----END {}-----\n",
            "OPENSSH PRIVATE KEY",
            "OPENSSH PRIVATE KEY"
        );
        fs::write(&key, pem).unwrap();
        let mut form = SshImportFormModalState::new();
        assert!(form.set_identity_from_browse(key.clone()).is_ok());
        assert_eq!(form.identity, key.display().to_string());

        let bad = dir.path().join("nope.mp4");
        let mut bytes = vec![0u8; 32];
        bytes[4..8].copy_from_slice(b"ftyp");
        fs::write(&bad, bytes).unwrap();
        assert!(form.set_identity_from_browse(bad).is_err());
        assert!(form.error.is_some());
    }

    #[test]
    fn submit_ok_with_explicit_login() {
        let mut form = SshImportFormModalState::new();
        form.destination = "mybox".into();
        form.user = "deploy".into();
        form.identity.clear();
        match form.try_submit() {
            SshImportFormOutcome::Submit(v) => {
                assert_eq!(v.destination, "mybox");
                assert_eq!(v.user.as_deref(), Some("deploy"));
                assert!(v.identity.is_none());
            }
            other => panic!("expected Submit, got {other:?}"),
        }
    }
}
