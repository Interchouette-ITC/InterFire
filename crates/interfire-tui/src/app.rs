//! App chrome: tabs, pane focus, overlay, IPC-backed status/log state.
#![forbid(unsafe_code)]

use std::collections::VecDeque;

use crossterm::event::KeyCode;
use interfire_proto::DaemonStatus;

use crate::ipc::IpcEvent;

pub const DEFAULT_SOCKET: &str = "/run/interfire/interfired.sock";
pub const MAX_AUDIT_LINES: usize = 200;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Tab {
    Status,
    Rules,
    Prompts,
    Log,
    Help,
}

impl Tab {
    pub const ALL: [Self; 5] = [
        Self::Status,
        Self::Rules,
        Self::Prompts,
        Self::Log,
        Self::Help,
    ];

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Status => "Status",
            Self::Rules => "Rules",
            Self::Prompts => "Prompts",
            Self::Log => "Log",
            Self::Help => "Help",
        }
    }

    #[must_use]
    pub const fn index(self) -> usize {
        match self {
            Self::Status => 0,
            Self::Rules => 1,
            Self::Prompts => 2,
            Self::Log => 3,
            Self::Help => 4,
        }
    }

    #[must_use]
    pub const fn from_index(index: usize) -> Self {
        Self::ALL[index % Self::ALL.len()]
    }

    #[must_use]
    pub const fn has_split(self) -> bool {
        matches!(self, Self::Rules | Self::Prompts | Self::Log)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Pane {
    List,
    Detail,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Overlay {
    None,
    Notice(&'static str),
}

#[derive(Clone, Debug)]
pub enum Link {
    Connecting,
    Down { detail: String },
    Up(DaemonStatus),
}

#[derive(Clone, Debug)]
pub struct App {
    pub socket: String,
    pub link: Link,
    pub audit: VecDeque<String>,
    pub subscribed: bool,
    pub tab: Tab,
    pub pane: Pane,
    pub list_selected: usize,
    pub overlay: Overlay,
}

impl App {
    #[must_use]
    pub const fn new(socket: String) -> Self {
        Self {
            socket,
            link: Link::Connecting,
            audit: VecDeque::new(),
            subscribed: false,
            tab: Tab::Status,
            pane: Pane::List,
            list_selected: 0,
            overlay: Overlay::None,
        }
    }

    pub fn apply(&mut self, event: IpcEvent) {
        match event {
            IpcEvent::Status(status) => {
                self.link = Link::Up(status);
            }
            IpcEvent::Down(detail) => {
                self.link = Link::Down { detail };
                self.subscribed = false;
            }
            IpcEvent::Audit(record) => {
                if self.audit.len() == MAX_AUDIT_LINES {
                    self.audit.pop_front();
                }
                self.audit
                    .push_back(format!("{}|{}", record.sequence, record.message));
                self.clamp_selection();
            }
            IpcEvent::SubscriptionReady => {
                self.subscribed = true;
            }
        }
    }

    #[must_use]
    pub fn chrome_title(&self) -> &'static str {
        match &self.link {
            Link::Connecting => "connecting",
            Link::Down { .. } => "daemon unavailable",
            Link::Up(status)
                if status.observation == "degraded" || status.enforcement == "degraded" =>
            {
                "degraded"
            }
            Link::Up(_) => "live",
        }
    }

    /// Handle a key at root chrome. Returns `true` when the app should quit.
    pub fn handle_key(&mut self, code: KeyCode) -> bool {
        if self.overlay != Overlay::None {
            if matches!(code, KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q')) {
                self.overlay = Overlay::None;
            }
            return false;
        }
        match code {
            KeyCode::Char('q' | 'Q') => return true,
            KeyCode::Char('?') => {
                self.overlay = Overlay::Notice("Press Esc to dismiss this overlay.");
            }
            KeyCode::Left | KeyCode::BackTab => self.prev_tab(),
            KeyCode::Right | KeyCode::Tab => self.next_tab(),
            KeyCode::Char('1') => self.set_tab(Tab::Status),
            KeyCode::Char('2') => self.set_tab(Tab::Rules),
            KeyCode::Char('3') => self.set_tab(Tab::Prompts),
            KeyCode::Char('4') => self.set_tab(Tab::Log),
            KeyCode::Char('5') => self.set_tab(Tab::Help),
            KeyCode::Char('h') if self.tab.has_split() => self.pane = Pane::List,
            KeyCode::Char('l') if self.tab.has_split() => self.pane = Pane::Detail,
            KeyCode::Char('j') | KeyCode::Down if self.pane == Pane::List => self.move_list(1),
            KeyCode::Char('k') | KeyCode::Up if self.pane == Pane::List => self.move_list(-1),
            _ => {}
        }
        false
    }

    fn set_tab(&mut self, tab: Tab) {
        self.tab = tab;
        self.pane = Pane::List;
        self.list_selected = 0;
        self.clamp_selection();
    }

    fn next_tab(&mut self) {
        self.set_tab(Tab::from_index(self.tab.index().saturating_add(1)));
    }

    fn prev_tab(&mut self) {
        let index = if self.tab.index() == 0 {
            Tab::ALL.len() - 1
        } else {
            self.tab.index() - 1
        };
        self.set_tab(Tab::from_index(index));
    }

    fn move_list(&mut self, delta: isize) {
        let len = self.list_len();
        if len == 0 {
            self.list_selected = 0;
            return;
        }
        let current = isize::try_from(self.list_selected).unwrap_or(0);
        let next = (current + delta).rem_euclid(isize::try_from(len).unwrap_or(1));
        self.list_selected = usize::try_from(next).unwrap_or(0);
    }

    fn clamp_selection(&mut self) {
        let len = self.list_len();
        if len == 0 {
            self.list_selected = 0;
        } else if self.list_selected >= len {
            self.list_selected = len - 1;
        }
    }

    #[must_use]
    pub fn list_len(&self) -> usize {
        match self.tab {
            Tab::Log => self.audit.len(),
            Tab::Rules | Tab::Prompts | Tab::Status | Tab::Help => 0,
        }
    }

    #[must_use]
    pub fn list_items(&self) -> Vec<String> {
        match self.tab {
            Tab::Log => self.audit.iter().cloned().collect(),
            Tab::Rules | Tab::Prompts | Tab::Status | Tab::Help => Vec::new(),
        }
    }

    #[must_use]
    pub fn detail_lines(&self) -> Vec<String> {
        match self.tab {
            Tab::Status => self.status_detail_lines(),
            Tab::Rules => vec![
                "Rules browser chrome is ready.".into(),
                "List + detail CRUD actions are not wired yet.".into(),
            ],
            Tab::Prompts => vec![
                "Prompts browser chrome is ready.".into(),
                "Allow/Deny overlays are not wired yet.".into(),
            ],
            Tab::Log => self
                .audit
                .get(self.list_selected)
                .cloned()
                .map_or_else(|| vec!["no audit frame selected".into()], |line| vec![line]),
            Tab::Help => help_lines(),
        }
    }

    fn status_detail_lines(&self) -> Vec<String> {
        match &self.link {
            Link::Connecting => vec![
                format!("ipc waiting  socket={}", self.socket),
                "waiting for daemon…".into(),
            ],
            Link::Down { detail } => vec![
                "daemon unavailable".into(),
                format!("socket={}", self.socket),
                detail.clone(),
            ],
            Link::Up(status) => vec![
                format!(
                    "enforcement={}  observation={}  ipc_version={}",
                    status.enforcement, status.observation, status.ipc_version
                ),
                format!("socket={}", self.socket),
                if self.subscribed {
                    "audit subscribe: ready (id=interfire-tui)".into()
                } else {
                    "audit subscribe: connecting…".into()
                },
            ],
        }
    }
}

#[must_use]
pub fn help_lines() -> Vec<String> {
    vec![
        "Tabs: Left/Right or 1..5  (Status Rules Prompts Log Help)".into(),
        "Panes: h list · l detail  (Rules / Prompts / Log)".into(),
        "List: j/k or Up/Down".into(),
        "Overlay: ? opens · Esc dismisses (Esc never quits root)".into(),
        "Quit: q".into(),
    ]
}

#[must_use]
pub fn footer_hints(app: &App) -> String {
    if app.overlay != Overlay::None {
        return "Esc dismiss overlay".into();
    }
    let mut parts = vec![
        format!("{} · {}", app.chrome_title(), app.tab.label()),
        "1-5 tabs".into(),
        "q quit".into(),
    ];
    if app.tab.has_split() {
        parts.insert(1, "h/l panes".into());
        parts.insert(2, "j/k list".into());
    }
    parts.join("  │  ")
}

#[cfg(test)]
mod tests {
    use super::{App, IpcEvent, Link, Overlay, Pane, Tab};
    use crossterm::event::KeyCode;
    use interfire_proto::DaemonStatus;

    #[test]
    fn esc_dismisses_overlay_and_never_quits() {
        let mut app = App::new("/tmp/x.sock".into());
        assert!(!app.handle_key(KeyCode::Char('?')));
        assert_ne!(app.overlay, Overlay::None);
        assert!(!app.handle_key(KeyCode::Esc));
        assert_eq!(app.overlay, Overlay::None);
        assert!(!app.handle_key(KeyCode::Esc));
    }

    #[test]
    fn tab_and_pane_keys_are_keyboard_driven() {
        let mut app = App::new("/tmp/x.sock".into());
        assert_eq!(app.tab, Tab::Status);
        app.handle_key(KeyCode::Char('2'));
        assert_eq!(app.tab, Tab::Rules);
        assert_eq!(app.pane, Pane::List);
        app.handle_key(KeyCode::Char('l'));
        assert_eq!(app.pane, Pane::Detail);
        app.handle_key(KeyCode::Char('h'));
        assert_eq!(app.pane, Pane::List);
        app.handle_key(KeyCode::Right);
        assert_eq!(app.tab, Tab::Prompts);
    }

    #[test]
    fn status_up_shows_live_or_degraded_chrome() {
        let mut app = App::new("/tmp/x.sock".into());
        app.apply(IpcEvent::Status(DaemonStatus {
            enforcement: "nfqueue".into(),
            observation: "attached".into(),
            ipc_version: 1,
        }));
        assert_eq!(app.chrome_title(), "live");
        assert!(matches!(app.link, Link::Up(_)));
        app.apply(IpcEvent::Status(DaemonStatus {
            enforcement: "none".into(),
            observation: "degraded".into(),
            ipc_version: 1,
        }));
        assert_eq!(app.chrome_title(), "degraded");
    }

    #[test]
    fn down_without_prior_status_is_unavailable() {
        let mut app = App::new("/tmp/x.sock".into());
        app.apply(IpcEvent::Down("connect refused".into()));
        assert_eq!(app.chrome_title(), "daemon unavailable");
    }
}
