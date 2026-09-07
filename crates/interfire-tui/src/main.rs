//! Control-plane TUI with tab chrome and async IPC.
//!
//! Terminal restore: raw mode and the alternate screen are cleared on normal
//! exit and from a panic hook so a crash does not leave the tty broken.
#![forbid(unsafe_code)]

mod app;
mod ipc;
mod ui;

use std::env;
use std::io::{self, stdout};
use std::panic;
use std::time::Duration;

use crossterm::event::{Event, EventStream, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use futures_util::StreamExt;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use tokio::sync::mpsc;
use tokio::time;

use crate::app::{App, DEFAULT_SOCKET};
use crate::ipc::spawn as spawn_ipc;

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
        terminal.draw(|frame| ui::draw(frame, &app))?;
        tokio::select! {
            maybe = events.next() => {
                match maybe {
                    Some(Ok(Event::Key(key))) if key.kind == KeyEventKind::Press => {
                        if app.handle_key(key.code) {
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
