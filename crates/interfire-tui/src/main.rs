//! Control-plane TUI scaffold (`interfire-tui`).
//!
//! Terminal restore: raw mode and the alternate screen are cleared on normal
//! exit and from a panic hook so a crash does not leave the tty broken.
#![forbid(unsafe_code)]

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
use interfire_proto::IPC_VERSION;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use tokio::time;

const DEFAULT_SOCKET: &str = "/run/interfire/interfired.sock";

#[tokio::main]
async fn main() -> io::Result<()> {
    let socket = parse_socket(env::args().skip(1));
    install_panic_hook();
    let mut terminal = setup_terminal()?;
    let result = run(&mut terminal, &socket).await;
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
    socket: &str,
) -> io::Result<()> {
    let mut events = EventStream::new();
    let mut tick = time::interval(Duration::from_millis(250));
    loop {
        terminal.draw(|frame| draw(frame, socket))?;
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
            _ = tick.tick() => {}
        }
    }
}

fn draw(frame: &mut ratatui::Frame<'_>, socket: &str) {
    let chunks = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .split(frame.area());
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                "InterFire TUI",
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::raw(format!("  ipc=v{IPC_VERSION}")),
        ]))
        .block(Block::default().borders(Borders::ALL).title("status")),
        chunks[0],
    );
    frame.render_widget(
        Paragraph::new(vec![
            Line::from("Scaffold only. Tabs and live IPC are not wired yet."),
            Line::from(format!("socket={socket}")),
            Line::from("Press q to quit. Esc does nothing at root chrome."),
        ])
        .block(Block::default().borders(Borders::ALL).title("help")),
        chunks[1],
    );
    frame.render_widget(Paragraph::new("q quit"), chunks[2]);
}

#[cfg(test)]
mod tests {
    use super::{DEFAULT_SOCKET, parse_socket};

    #[test]
    fn parse_socket_defaults_and_overrides() {
        assert_eq!(parse_socket(Vec::<String>::new()), DEFAULT_SOCKET);
        assert_eq!(
            parse_socket(vec!["--socket=/tmp/interfire.sock".into()]),
            "/tmp/interfire.sock"
        );
    }
}
