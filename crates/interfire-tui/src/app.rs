//! App chrome: tabs, pane focus, overlay, IPC-backed status/rules/log state.
#![forbid(unsafe_code)]

use std::collections::VecDeque;

use crossterm::event::KeyCode;
use interfire_proto::{
    DaemonStatus, MAX_LOG_RECORDS_PER_SUBSCRIBER, ProcessRow, PromptRow, RuleRow,
};

use crate::ipc::{IpcCommand, IpcEvent};

pub use interfire_proto::DEFAULT_SOCKET_PATH as DEFAULT_SOCKET;
/// Cap aligned with `MAX_LOG_RECORDS_PER_SUBSCRIBER`.
pub const MAX_AUDIT_LINES: usize = MAX_LOG_RECORDS_PER_SUBSCRIBER;

/// Slice of list rows for the current viewport (Log is windowed).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VisibleList {
    pub relative_selected: usize,
    pub items: Vec<String>,
    pub start: usize,
    pub total: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Tab {
    Status,
    Apps,
    Rules,
    Prompts,
    Log,
    Help,
}

impl Tab {
    pub const ALL: [Self; 6] = [
        Self::Status,
        Self::Apps,
        Self::Rules,
        Self::Prompts,
        Self::Log,
        Self::Help,
    ];

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Status => "Status",
            Self::Apps => "Apps",
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
            Self::Apps => 1,
            Self::Rules => 2,
            Self::Prompts => 3,
            Self::Log => 4,
            Self::Help => 5,
        }
    }

    #[must_use]
    pub const fn from_index(index: usize) -> Self {
        Self::ALL[index % Self::ALL.len()]
    }

    #[must_use]
    pub const fn has_split(self) -> bool {
        matches!(self, Self::Apps | Self::Rules | Self::Prompts | Self::Log)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Pane {
    List,
    Detail,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AddField {
    Id,
    Executable,
    Verdict,
    Port,
}

impl AddField {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Id => "id",
            Self::Executable => "executable",
            Self::Verdict => "verdict",
            Self::Port => "port",
        }
    }

    const fn next(self) -> Self {
        match self {
            Self::Id => Self::Executable,
            Self::Executable => Self::Verdict,
            Self::Verdict => Self::Port,
            Self::Port => Self::Id,
        }
    }

    const fn prev(self) -> Self {
        match self {
            Self::Id => Self::Port,
            Self::Executable => Self::Id,
            Self::Verdict => Self::Executable,
            Self::Port => Self::Verdict,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AddRuleForm {
    pub id: String,
    pub executable: String,
    pub verdict: String,
    pub port: String,
    pub focus: AddField,
}

impl AddRuleForm {
    fn new() -> Self {
        Self {
            id: String::new(),
            executable: String::new(),
            verdict: "deny".into(),
            port: "443".into(),
            focus: AddField::Id,
        }
    }

    const fn focused_mut(&mut self) -> &mut String {
        match self.focus {
            AddField::Id => &mut self.id,
            AddField::Executable => &mut self.executable,
            AddField::Verdict => &mut self.verdict,
            AddField::Port => &mut self.port,
        }
    }

    fn to_command(&self) -> Result<IpcCommand, String> {
        let id = self
            .id
            .parse::<u64>()
            .map_err(|_| "id must be an integer".to_owned())?;
        if self.executable.is_empty() || !self.executable.starts_with('/') {
            return Err("executable must be an absolute path".into());
        }
        let verdict = self.verdict.to_ascii_lowercase();
        if !matches!(verdict.as_str(), "allow" | "deny" | "prompt") {
            return Err("verdict must be allow|deny|prompt".into());
        }
        let port = self
            .port
            .parse::<u16>()
            .map_err(|_| "port must be 0..65535".to_owned())?;
        Ok(IpcCommand::AddRule {
            id,
            executable: self.executable.clone(),
            verdict,
            port,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AnswerVerdict {
    Allow,
    Deny,
}

impl AnswerVerdict {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Allow => "allow",
            Self::Deny => "deny",
        }
    }

    const fn toggle(self) -> Self {
        match self {
            Self::Allow => Self::Deny,
            Self::Deny => Self::Allow,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AnswerScope {
    Once,
    Session,
    Permanent,
}

impl AnswerScope {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Once => "once",
            Self::Session => "session",
            Self::Permanent => "permanent",
        }
    }

    const fn next(self) -> Self {
        match self {
            Self::Once => Self::Session,
            Self::Session => Self::Permanent,
            Self::Permanent => Self::Once,
        }
    }

    const fn prev(self) -> Self {
        match self {
            Self::Once => Self::Permanent,
            Self::Session => Self::Once,
            Self::Permanent => Self::Session,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AnswerPromptForm {
    pub prompt: PromptRow,
    pub verdict: AnswerVerdict,
    pub scope: AnswerScope,
}

impl AnswerPromptForm {
    const fn new(prompt: PromptRow) -> Self {
        Self {
            prompt,
            verdict: AnswerVerdict::Deny,
            scope: AnswerScope::Once,
        }
    }

    fn to_command(&self) -> IpcCommand {
        IpcCommand::AnswerPrompt {
            id: self.prompt.id,
            verdict: self.verdict.as_str().into(),
            scope: self.scope.as_str().into(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Overlay {
    None,
    Notice(String),
    AddRule(AddRuleForm),
    AnswerPrompt(AnswerPromptForm),
}

#[derive(Clone, Debug)]
pub enum Link {
    Connecting,
    Down { detail: String },
    Up(DaemonStatus),
}

/// Result of handling a key press.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum KeyAction {
    None,
    Quit,
    Command(IpcCommand),
}

#[derive(Clone, Debug)]
pub struct App {
    pub socket: String,
    pub link: Link,
    pub audit: VecDeque<String>,
    pub rules: Vec<RuleRow>,
    pub prompts: Vec<PromptRow>,
    pub processes: Vec<ProcessRow>,
    pub subscribed: bool,
    pub tab: Tab,
    pub pane: Pane,
    pub list_selected: usize,
    pub overlay: Overlay,
    pub status_message: Option<String>,
}

impl App {
    #[must_use]
    pub const fn new(socket: String) -> Self {
        Self {
            socket,
            link: Link::Connecting,
            audit: VecDeque::new(),
            rules: Vec::new(),
            prompts: Vec::new(),
            processes: Vec::new(),
            subscribed: false,
            tab: Tab::Status,
            pane: Pane::List,
            list_selected: 0,
            overlay: Overlay::None,
            status_message: None,
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
                self.push_audit_line(format!("{}|{}", record.sequence, record.message));
            }
            IpcEvent::SubscriptionReady => {
                // Reconnect replaced the daemon subscription; keep the local
                // capped buffer and resume appending from the stream.
                self.subscribed = true;
            }
            IpcEvent::Rules(rules) => {
                self.rules = rules;
                self.clamp_selection();
            }
            IpcEvent::Prompts(prompts) => {
                self.prompts = prompts;
                self.clamp_selection();
            }
            IpcEvent::Processes(processes) => {
                self.processes = processes;
                self.clamp_selection();
            }
            IpcEvent::ActionOk(message) => {
                self.status_message = Some(message);
            }
            IpcEvent::ActionError(message) => {
                self.overlay = Overlay::Notice(format!("error: {message}"));
                self.status_message = Some(format!("error: {message}"));
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

    /// Handle a key at root chrome or overlay.
    pub fn handle_key(&mut self, code: KeyCode) -> KeyAction {
        match self.overlay {
            Overlay::None => self.handle_root_key(code),
            Overlay::Notice(_) => {
                if matches!(code, KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q')) {
                    self.overlay = Overlay::None;
                }
                KeyAction::None
            }
            Overlay::AddRule(_) => self.handle_add_overlay_key(code),
            Overlay::AnswerPrompt(_) => self.handle_answer_overlay_key(code),
        }
    }

    fn handle_add_overlay_key(&mut self, code: KeyCode) -> KeyAction {
        match code {
            KeyCode::Esc => {
                self.overlay = Overlay::None;
                return KeyAction::None;
            }
            KeyCode::Enter => {
                let command = match &self.overlay {
                    Overlay::AddRule(form) => form.to_command(),
                    Overlay::None | Overlay::Notice(_) | Overlay::AnswerPrompt(_) => {
                        return KeyAction::None;
                    }
                };
                return match command {
                    Ok(command) => {
                        self.overlay = Overlay::None;
                        KeyAction::Command(command)
                    }
                    Err(message) => {
                        self.status_message = Some(message);
                        KeyAction::None
                    }
                };
            }
            _ => {}
        }
        let Overlay::AddRule(form) = &mut self.overlay else {
            return KeyAction::None;
        };
        match code {
            KeyCode::Tab => {
                form.focus = form.focus.next();
            }
            KeyCode::BackTab => {
                form.focus = form.focus.prev();
            }
            KeyCode::Backspace => {
                form.focused_mut().pop();
            }
            KeyCode::Char(ch) if !ch.is_control() => {
                form.focused_mut().push(ch);
            }
            _ => {}
        }
        KeyAction::None
    }

    fn handle_answer_overlay_key(&mut self, code: KeyCode) -> KeyAction {
        match code {
            KeyCode::Esc => {
                self.overlay = Overlay::None;
                return KeyAction::None;
            }
            KeyCode::Enter => {
                let command = match &self.overlay {
                    Overlay::AnswerPrompt(form) => {
                        if !form.prompt.can_answer() {
                            self.overlay =
                                Overlay::Notice("prompt expired or stale; actions disabled".into());
                            return KeyAction::None;
                        }
                        form.to_command()
                    }
                    Overlay::None | Overlay::Notice(_) | Overlay::AddRule(_) => {
                        return KeyAction::None;
                    }
                };
                self.overlay = Overlay::None;
                return KeyAction::Command(command);
            }
            _ => {}
        }
        let Overlay::AnswerPrompt(form) = &mut self.overlay else {
            return KeyAction::None;
        };
        match code {
            KeyCode::Char('a' | 'A') => form.verdict = AnswerVerdict::Allow,
            KeyCode::Char('d' | 'D') => form.verdict = AnswerVerdict::Deny,
            KeyCode::Left | KeyCode::Right => form.verdict = form.verdict.toggle(),
            KeyCode::Tab => form.scope = form.scope.next(),
            KeyCode::BackTab => form.scope = form.scope.prev(),
            _ => {}
        }
        KeyAction::None
    }

    fn handle_root_key(&mut self, code: KeyCode) -> KeyAction {
        match code {
            KeyCode::Char('q' | 'Q') => KeyAction::Quit,
            KeyCode::Char('?') => {
                self.overlay = Overlay::Notice("Press Esc to dismiss this overlay.".into());
                KeyAction::None
            }
            KeyCode::Left | KeyCode::BackTab => {
                self.prev_tab();
                KeyAction::None
            }
            KeyCode::Right | KeyCode::Tab => {
                self.next_tab();
                KeyAction::None
            }
            KeyCode::Char('1') => {
                self.set_tab(Tab::Status);
                KeyAction::None
            }
            KeyCode::Char('2') => {
                self.set_tab(Tab::Apps);
                KeyAction::None
            }
            KeyCode::Char('3') => {
                self.set_tab(Tab::Rules);
                KeyAction::None
            }
            KeyCode::Char('4') => {
                self.set_tab(Tab::Prompts);
                KeyAction::None
            }
            KeyCode::Char('5') => {
                self.set_tab(Tab::Log);
                KeyAction::None
            }
            KeyCode::Char('6') => {
                self.set_tab(Tab::Help);
                KeyAction::None
            }
            KeyCode::Char('h') if self.tab.has_split() => {
                self.pane = Pane::List;
                KeyAction::None
            }
            KeyCode::Char('l') if self.tab.has_split() => {
                self.pane = Pane::Detail;
                KeyAction::None
            }
            KeyCode::Char('j') | KeyCode::Down if self.pane == Pane::List => {
                self.move_list(1);
                KeyAction::None
            }
            KeyCode::Char('k') | KeyCode::Up if self.pane == Pane::List => {
                self.move_list(-1);
                KeyAction::None
            }
            KeyCode::Char('a') if self.tab == Tab::Rules => {
                self.overlay = Overlay::AddRule(AddRuleForm::new());
                KeyAction::None
            }
            KeyCode::Char('d') if self.tab == Tab::Rules => self.delete_selected_rule(),
            KeyCode::Char('r') if self.tab == Tab::Rules => {
                KeyAction::Command(IpcCommand::RefreshRules)
            }
            KeyCode::Char('a') | KeyCode::Enter if self.tab == Tab::Prompts => {
                self.open_answer_overlay()
            }
            KeyCode::Char('r') if self.tab == Tab::Prompts => {
                KeyAction::Command(IpcCommand::RefreshPrompts)
            }
            _ => KeyAction::None,
        }
    }

    fn open_answer_overlay(&mut self) -> KeyAction {
        match self.prompts.get(self.list_selected) {
            Some(prompt) if prompt.can_answer() => {
                self.overlay = Overlay::AnswerPrompt(AnswerPromptForm::new(prompt.clone()));
                KeyAction::None
            }
            Some(_) => {
                self.overlay = Overlay::Notice("prompt expired or stale; answer disabled".into());
                KeyAction::None
            }
            None => KeyAction::None,
        }
    }

    fn delete_selected_rule(&self) -> KeyAction {
        self.rules
            .get(self.list_selected)
            .map_or(KeyAction::None, |rule| {
                KeyAction::Command(IpcCommand::DeleteRule { id: rule.id })
            })
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

    fn push_audit_line(&mut self, line: String) {
        if self.audit.len() == MAX_AUDIT_LINES {
            self.audit.pop_front();
            if self.list_selected > 0 && self.tab == Tab::Log {
                self.list_selected -= 1;
            }
        }
        self.audit.push_back(line);
        self.clamp_selection();
    }

    #[must_use]
    pub fn list_len(&self) -> usize {
        match self.tab {
            Tab::Log => self.audit.len(),
            Tab::Apps => self.processes.len(),
            Tab::Rules => self.rules.len(),
            Tab::Prompts => self.prompts.len(),
            Tab::Status | Tab::Help => 0,
        }
    }

    /// Visible list window for the given viewport height (virtualized for Log).
    #[must_use]
    pub fn visible_list(&self, viewport_rows: usize) -> VisibleList {
        match self.tab {
            Tab::Log => self.visible_audit_window(viewport_rows),
            Tab::Apps => VisibleList {
                relative_selected: self.list_selected,
                items: self.processes.iter().map(ProcessRow::list_label).collect(),
                start: 0,
                total: self.processes.len(),
            },
            Tab::Rules => VisibleList {
                relative_selected: self.list_selected,
                items: self.rules.iter().map(RuleRow::list_label).collect(),
                start: 0,
                total: self.rules.len(),
            },
            Tab::Prompts => VisibleList {
                relative_selected: self.list_selected,
                items: self.prompts.iter().map(PromptRow::list_label).collect(),
                start: 0,
                total: self.prompts.len(),
            },
            Tab::Status | Tab::Help => VisibleList {
                relative_selected: 0,
                items: Vec::new(),
                start: 0,
                total: 0,
            },
        }
    }

    fn visible_audit_window(&self, viewport_rows: usize) -> VisibleList {
        let total = self.audit.len();
        if total == 0 {
            return VisibleList {
                relative_selected: 0,
                items: Vec::new(),
                start: 0,
                total: 0,
            };
        }
        let height = viewport_rows.max(1);
        let selected = self.list_selected.min(total - 1);
        let start = if total <= height {
            0
        } else {
            let half = height / 2;
            selected
                .saturating_sub(half)
                .min(total.saturating_sub(height))
        };
        let end = (start + height).min(total);
        VisibleList {
            relative_selected: selected - start,
            items: self
                .audit
                .iter()
                .skip(start)
                .take(end - start)
                .cloned()
                .collect(),
            start,
            total,
        }
    }

    #[must_use]
    pub fn detail_lines(&self) -> Vec<String> {
        match self.tab {
            Tab::Status => self.status_detail_lines(),
            Tab::Apps => self.process_detail_lines(),
            Tab::Rules => self.rule_detail_lines(),
            Tab::Prompts => self.prompt_detail_lines(),
            Tab::Log => self
                .audit
                .get(self.list_selected)
                .cloned()
                .map_or_else(|| vec!["no audit frame selected".into()], |line| vec![line]),
            Tab::Help => help_lines(),
        }
    }

    fn process_detail_lines(&self) -> Vec<String> {
        self.processes.get(self.list_selected).map_or_else(
            || {
                vec![
                    "no observed processes yet".into(),
                    "outbound connects fill this list".into(),
                ]
            },
            |row| {
                let mut lines = vec![
                    format!("executable: {}", row.executable),
                    format!("cmdline: {}", row.cmdline),
                    format!(
                        "pid: {}  start_ticks: {}  uid: {}",
                        row.pid, row.start_ticks, row.uid
                    ),
                    format!("effective rule: {}", row.verdict),
                    String::new(),
                    "recent destinations:".into(),
                ];
                if row.ports.is_empty() {
                    lines.push("  (none)".into());
                } else {
                    for dest in row.ports.split('+') {
                        lines.push(format!("  {dest}"));
                    }
                }
                lines
            },
        )
    }

    fn rule_detail_lines(&self) -> Vec<String> {
        self.rules.get(self.list_selected).map_or_else(
            || vec!["no rules loaded".into(), "a add · r refresh".into()],
            |rule| {
                vec![
                    format!("id: {}", rule.id),
                    format!("executable: {}", rule.executable),
                    format!("verdict: {}", rule.verdict),
                    format!("port: {}", rule.port),
                    String::new(),
                    "a add · d delete · r refresh".into(),
                ]
            },
        )
    }

    fn prompt_detail_lines(&self) -> Vec<String> {
        self.prompts.get(self.list_selected).map_or_else(
            || vec!["no pending prompts".into(), "r refresh".into()],
            |prompt| {
                let mut lines = vec![
                    format!("id: {}", prompt.id),
                    format!("path: {}", prompt.executable),
                    format!(
                        "dest: {}:{} ({})",
                        prompt.destination, prompt.port, prompt.protocol
                    ),
                    format!("remaining: {}s", prompt.remaining_secs),
                ];
                lines.push(String::new());
                if prompt.can_answer() {
                    lines.push("a/Enter answer · r refresh".into());
                } else {
                    lines.push("expired/stale · answer disabled".into());
                }
                lines
            },
        )
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
        "Tabs: Left/Right or 1..6  (Status Apps Rules Prompts Log Help)".into(),
        "Panes: h list · l detail  (Apps / Rules / Prompts / Log)".into(),
        "List: j/k or Up/Down".into(),
        "Log: capped 2000 rows, virtualized viewport; reconnect replaces subscribe".into(),
        "Apps: observed firewall identities (path, pid+start ticks, ports)".into(),
        "Rules: a add · d delete · r refresh".into(),
        "Prompts: a/Enter answer · r refresh".into(),
        "Answer overlay: a/d verdict · Tab scope · Enter submit · Esc cancel".into(),
        "Add overlay: Tab fields · Enter submit · Esc cancel".into(),
        "Overlay: ? opens · Esc dismisses (Esc never quits root)".into(),
        "Quit: q".into(),
    ]
}

#[must_use]
pub fn footer_hints(app: &App) -> String {
    if !matches!(app.overlay, Overlay::None) {
        return match &app.overlay {
            Overlay::AddRule(_) => "Tab fields  Enter submit  Esc cancel".into(),
            Overlay::AnswerPrompt(_) => "a/d verdict  Tab scope  Enter submit  Esc cancel".into(),
            Overlay::Notice(_) => "Esc dismiss overlay".into(),
            Overlay::None => String::new(),
        };
    }
    let mut parts = vec![
        format!("{} · {}", app.chrome_title(), app.tab.label()),
        "1-6 tabs".into(),
        "q quit".into(),
    ];
    if app.tab.has_split() {
        parts.insert(1, "h/l panes".into());
        parts.insert(2, "j/k list".into());
    }
    if app.tab == Tab::Rules {
        parts.insert(1, "a/d/r rules".into());
    }
    if app.tab == Tab::Prompts {
        parts.insert(1, "a/r prompts".into());
    }
    if let Some(message) = &app.status_message {
        parts.push(message.clone());
    }
    parts.join("  │  ")
}

#[cfg(test)]
mod tests {
    use super::{App, IpcEvent, KeyAction, Link, Overlay, Pane, Tab};
    use crossterm::event::KeyCode;
    use interfire_proto::{DaemonStatus, PromptRow, RuleRow};

    use crate::ipc::IpcCommand;

    #[test]
    fn esc_dismisses_overlay_and_never_quits() {
        let mut app = App::new("/tmp/x.sock".into());
        assert_eq!(app.handle_key(KeyCode::Char('?')), KeyAction::None);
        assert!(matches!(app.overlay, Overlay::Notice(_)));
        assert_eq!(app.handle_key(KeyCode::Esc), KeyAction::None);
        assert_eq!(app.overlay, Overlay::None);
        assert_eq!(app.handle_key(KeyCode::Esc), KeyAction::None);
    }

    #[test]
    fn tab_and_pane_keys_are_keyboard_driven() {
        let mut app = App::new("/tmp/x.sock".into());
        assert_eq!(app.tab, Tab::Status);
        app.handle_key(KeyCode::Char('2'));
        assert_eq!(app.tab, Tab::Apps);
        assert_eq!(app.pane, Pane::List);
        app.handle_key(KeyCode::Char('l'));
        assert_eq!(app.pane, Pane::Detail);
        app.handle_key(KeyCode::Char('h'));
        assert_eq!(app.pane, Pane::List);
        app.handle_key(KeyCode::Right);
        assert_eq!(app.tab, Tab::Rules);
    }

    #[test]
    fn rules_list_detail_and_delete_command() {
        let mut app = App::new("/tmp/x.sock".into());
        app.handle_key(KeyCode::Char('3'));
        app.apply(IpcEvent::Rules(vec![RuleRow {
            id: 9,
            executable: "/bin/curl".into(),
            verdict: "Allow".into(),
            port: 443,
        }]));
        assert_eq!(
            app.visible_list(usize::MAX).items[0],
            "9  /bin/curl  Allow  :443"
        );
        assert!(app.detail_lines().iter().any(|line| line.contains("id: 9")));
        assert_eq!(
            app.handle_key(KeyCode::Char('d')),
            KeyAction::Command(IpcCommand::DeleteRule { id: 9 })
        );
    }

    #[test]
    fn add_overlay_submits_rule_command() {
        let mut app = App::new("/tmp/x.sock".into());
        app.handle_key(KeyCode::Char('3'));
        app.handle_key(KeyCode::Char('a'));
        assert!(matches!(app.overlay, Overlay::AddRule(_)));
        for ch in "7".chars() {
            app.handle_key(KeyCode::Char(ch));
        }
        app.handle_key(KeyCode::Tab);
        for ch in "/bin/curl".chars() {
            app.handle_key(KeyCode::Char(ch));
        }
        assert_eq!(
            app.handle_key(KeyCode::Enter),
            KeyAction::Command(IpcCommand::AddRule {
                id: 7,
                executable: "/bin/curl".into(),
                verdict: "deny".into(),
                port: 443,
            })
        );
        assert_eq!(app.overlay, Overlay::None);
    }

    #[test]
    fn prompt_answer_overlay_submits_command() {
        let mut app = App::new("/tmp/x.sock".into());
        app.handle_key(KeyCode::Char('4'));
        app.apply(IpcEvent::Prompts(vec![PromptRow {
            id: 4,
            executable: "/bin/curl".into(),
            destination: "203.0.113.1".into(),
            port: 443,
            protocol: "tcp".into(),
            remaining_secs: 30,
        }]));
        assert!(app.visible_list(usize::MAX).items[0].contains("/bin/curl"));
        assert!(
            app.detail_lines()
                .iter()
                .any(|line| line.contains("remaining: 30s"))
        );
        app.handle_key(KeyCode::Char('a'));
        assert!(matches!(app.overlay, Overlay::AnswerPrompt(_)));
        app.handle_key(KeyCode::Char('a'));
        app.handle_key(KeyCode::Tab);
        assert_eq!(
            app.handle_key(KeyCode::Enter),
            KeyAction::Command(IpcCommand::AnswerPrompt {
                id: 4,
                verdict: "allow".into(),
                scope: "session".into(),
            })
        );
    }

    #[test]
    fn expired_prompt_disables_answer() {
        let mut app = App::new("/tmp/x.sock".into());
        app.handle_key(KeyCode::Char('4'));
        app.apply(IpcEvent::Prompts(vec![PromptRow {
            id: 4,
            executable: "/bin/curl".into(),
            destination: "203.0.113.1".into(),
            port: 443,
            protocol: "tcp".into(),
            remaining_secs: 0,
        }]));
        app.handle_key(KeyCode::Char('a'));
        assert!(matches!(app.overlay, Overlay::Notice(_)));
    }

    #[test]
    fn status_up_shows_live_or_degraded_chrome() {
        let mut app = App::new("/tmp/x.sock".into());
        app.apply(IpcEvent::Status(DaemonStatus {
            enforcement: "nfqueue".into(),
            observation: "attached".into(),
            ipc_version: 1,
            pid: None,
            rss_kib: None,
            cpu_jiffies: None,
        }));
        assert_eq!(app.chrome_title(), "live");
        assert!(matches!(app.link, Link::Up(_)));
        app.apply(IpcEvent::Status(DaemonStatus {
            enforcement: "none".into(),
            observation: "degraded".into(),
            ipc_version: 1,
            pid: None,
            rss_kib: None,
            cpu_jiffies: None,
        }));
        assert_eq!(app.chrome_title(), "degraded");
    }

    #[test]
    fn down_without_prior_status_is_unavailable() {
        let mut app = App::new("/tmp/x.sock".into());
        app.apply(IpcEvent::Down("connect refused".into()));
        assert_eq!(app.chrome_title(), "daemon unavailable");
    }

    #[test]
    fn audit_buffer_stays_capped_under_load() {
        use super::MAX_AUDIT_LINES;
        use interfire_proto::AuditStreamRecord;

        let mut app = App::new("/tmp/x.sock".into());
        app.handle_key(KeyCode::Char('5'));
        for sequence in 1..=(MAX_AUDIT_LINES as u64 + 500) {
            app.apply(IpcEvent::Audit(AuditStreamRecord {
                sequence,
                message: format!("line-{sequence}"),
            }));
        }
        assert_eq!(app.audit.len(), MAX_AUDIT_LINES);
        assert_eq!(app.list_len(), MAX_AUDIT_LINES);
        let window = app.visible_list(10);
        assert!(window.items.len() <= 10);
        assert_eq!(window.total, MAX_AUDIT_LINES);
        assert!(window.items.last().unwrap().contains("line-"));
    }

    #[test]
    fn log_viewport_is_virtualized_around_selection() {
        use interfire_proto::AuditStreamRecord;

        let mut app = App::new("/tmp/x.sock".into());
        app.handle_key(KeyCode::Char('5'));
        for sequence in 1..=40 {
            app.apply(IpcEvent::Audit(AuditStreamRecord {
                sequence,
                message: format!("m{sequence}"),
            }));
        }
        app.list_selected = 30;
        let window = app.visible_list(5);
        assert_eq!(window.items.len(), 5);
        assert_eq!(window.total, 40);
        assert!(window.start <= 30);
        assert_eq!(window.relative_selected, 30 - window.start);
    }

    #[test]
    fn reconnect_ready_marks_subscription_without_unbounded_growth() {
        use super::MAX_AUDIT_LINES;
        use interfire_proto::AuditStreamRecord;

        let mut app = App::new("/tmp/x.sock".into());
        app.apply(IpcEvent::SubscriptionReady);
        assert!(app.subscribed);
        app.apply(IpcEvent::Down("eof".into()));
        assert!(!app.subscribed);
        app.apply(IpcEvent::SubscriptionReady);
        assert!(app.subscribed);
        for sequence in 1..=50 {
            app.apply(IpcEvent::Audit(AuditStreamRecord {
                sequence,
                message: "x".into(),
            }));
        }
        assert!(app.audit.len() <= MAX_AUDIT_LINES);
    }
}
