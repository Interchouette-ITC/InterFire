//! Control-plane TUI with async IPC status and audit subscribe.
//!
//! Terminal restore: raw mode and the alternate screen are cleared on normal
//! exit and from a panic hook so a crash does not leave the tty broken.
#![forbid(unsafe_code)]

mod ipc;

use std::collections::VecDeque;
use std::env;
use std::io::{self, stdout};
use std::panic;
use std::time::Duration;

use crossterm::event::{Event, EventStream, KeyCode, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use futures_util::StreamExt;
use interfire_proto::{DaemonStatus, IPC_VERSION};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use tokio::sync::mpsc;
use tokio::time;

use crate::ipc::{IpcEvent, spawn as spawn_ipc};

const DEFAULT_SOCKET: &str = "/run/interfire/interfired.sock";
const MAX_AUDIT_LINES: usize = 200;

#[derive(Clone, Debug)]
enum Link {
    Connecting,
    Down { detail: String },
    Up(DaemonStatus),
}

struct App {
    socket: String,
    link: Link,
    audit: VecDeque<String>,
    subscribed: bool,
}

impl App {
    const fn new(socket: String) -> Self {
        Self {
            socket,
            link: Link::Connecting,
            audit: VecDeque::new(),
            subscribed: false,
        }
    }

    fn apply(&mut self, event: IpcEvent) {
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
            }
            IpcEvent::SubscriptionReady => {
                self.subscribed = true;
            }
        }
    }

    fn chrome_title(&self) -> &'static str {
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
}

#[tokio::main]
async fn main() -> io::Result<()> {
    let socket = parse_socket(env::args().skip(1));
    install_panic_hook();
    let mut terminal = setup_terminal()?;
    let result = run(&mut terminal, socket).await;
    restore_terminal()?;
    result
}

fn parse_socket(args: impl IntoIterator<Item = String>) -> String {
    let mut socket = DEFAULT_SOCKET.to_owned();
    for argument in args {
        if let Some(value) = argument.strip_prefix("--socket=") {
            value.clone_into(&mut socket);
        }
    }
    socket
}

fn install_panic_hook() {
    let previous = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        let _ = restore_terminal();
        previous(info);
    }));
}

fn setup_terminal() -> io::Result<Terminal<CrosstermBackend<io::Stdout>>> {
    enable_raw_mode()?;
    execute!(stdout(), EnterAlternateScreen)?;
    Terminal::new(CrosstermBackend::new(stdout()))
}

fn restore_terminal() -> io::Result<()> {
    disable_raw_mode()?;
    execute!(stdout(), LeaveAlternateScreen)?;
    Ok(())
}

async fn run(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    socket: String,
) -> io::Result<()> {
    let mut app = App::new(socket.clone());
    let (tx, mut rx) = mpsc::unbounded_channel();
    spawn_ipc(socket, tx);
    let mut events = EventStream::new();
    let mut tick = time::interval(Duration::from_millis(100));
    loop {
        terminal.draw(|frame| draw(frame, &app))?;
        tokio::select! {
            maybe = events.next() => {
                match maybe {
                    Some(Ok(Event::Key(key))) if key.kind == KeyEventKind::Press => {
                        if matches!(key.code, KeyCode::Char('q' | 'Q')) {
                            return Ok(());
                        }
                    }
                    Some(Err(error)) => return Err(error),
                    None => return Ok(()),
                    _ => {}
                }
            }
            Some(event) = rx.recv() => {
                app.apply(event);
            }
            _ = tick.tick() => {}
        }
    }
}

fn draw(frame: &mut ratatui::Frame<'_>, app: &App) {
    let chunks = Layout::vertical([
        Constraint::Length(5),
        Constraint::Min(3),
        Constraint::Length(1),
    ])
    .split(frame.area());
    frame.render_widget(status_panel(app), chunks[0]);
    frame.render_widget(audit_panel(app), chunks[1]);
    frame.render_widget(Paragraph::new("q quit"), chunks[2]);
}

fn status_panel(app: &App) -> Paragraph<'_> {
    let title = format!("status · {}", app.chrome_title());
    let lines = match &app.link {
        Link::Connecting => vec![
            Line::from(format!("ipc=v{IPC_VERSION}  socket={}", app.socket)),
            Line::from("waiting for daemon…"),
        ],
        Link::Down { detail } => vec![
            Line::from(Span::styled(
                "daemon unavailable",
                Style::default().add_modifier(Modifier::BOLD),
            )),
            Line::from(format!("socket={}", app.socket)),
            Line::from(detail.as_str()),
        ],
        Link::Up(status) => vec![
            Line::from(format!(
                "enforcement={}  observation={}  ipc_version={}",
                status.enforcement, status.observation, status.ipc_version
            )),
            Line::from(format!("socket={}", app.socket)),
            Line::from(if app.subscribed {
                "audit subscribe: ready (id=interfire-tui)"
            } else {
                "audit subscribe: connecting…"
            }),
        ],
    };
    Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title(title))
}

fn audit_panel(app: &App) -> Paragraph<'_> {
    let body: Vec<Line<'_>> = if app.audit.is_empty() {
        vec![Line::from("no audit frames yet")]
    } else {
        app.audit
            .iter()
            .rev()
            .take(32)
            .map(|line| Line::from(line.as_str()))
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect()
    };
    Paragraph::new(body).block(Block::default().borders(Borders::ALL).title("audit"))
}

#[cfg(test)]
mod tests {
    use super::{App, DEFAULT_SOCKET, IpcEvent, Link, parse_socket};
    use interfire_proto::DaemonStatus;

    #[test]
    fn parse_socket_defaults_and_overrides() {
        assert_eq!(parse_socket(Vec::<String>::new()), DEFAULT_SOCKET);
        assert_eq!(
            parse_socket(vec!["--socket=/tmp/interfire.sock".into()]),
            "/tmp/interfire.sock"
        );
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
