//! Control-plane TUI with tab chrome and async IPC.
//!
//! Terminal restore: raw mode and the alternate screen are cleared on normal
//! exit and from a panic hook so a crash does not leave the tty broken.
#![forbid(unsafe_code)]

mod app;
mod ipc;
mod palette;
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

use crate::app::{App, DEFAULT_SOCKET, KeyAction};
use crate::ipc::spawn as spawn_ipc;

#[tokio::main]
async fn main() -> io::Result<()> {
    let (socket, theme_mode) = parse_args(env::args().skip(1));
    palette::set_mode(theme_mode);
    install_panic_hook();
    let mut terminal = setup_terminal()?;
    let result = run(&mut terminal, socket).await;
    restore_terminal()?;
    result
}

fn parse_args(args: impl IntoIterator<Item = String>) -> (String, palette::Mode) {
    let mut socket = DEFAULT_SOCKET.to_owned();
    let mut theme_mode = palette::Mode::Dark;
    for argument in args {
        if let Some(value) = argument.strip_prefix("--socket=") {
            value.clone_into(&mut socket);
        } else if let Some(value) = argument.strip_prefix("--theme=")
            && let Some(mode) = palette::Mode::parse(value)
        {
            theme_mode = mode;
        }
    }
    (socket, theme_mode)
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
    let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
    spawn_ipc(socket, tx, cmd_rx);
    let mut events = EventStream::new();
    let mut tick = time::interval(Duration::from_millis(100));
    loop {
        terminal.draw(|frame| ui::draw(frame, &app))?;
        tokio::select! {
            maybe = events.next() => {
                match maybe {
                    Some(Ok(Event::Key(key))) if key.kind == KeyEventKind::Press => {
                        match app.handle_key(key.code) {
                            KeyAction::Quit => return Ok(()),
                            KeyAction::Command(command) => {
                                let _ = cmd_tx.send(command);
                            }
                            KeyAction::None => {}
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
    use super::{DEFAULT_SOCKET, parse_args};
    use crate::palette::Mode;

    #[test]
    fn parse_args_defaults_and_overrides() {
        let (socket, theme) = parse_args(Vec::<String>::new());
        assert_eq!(socket, DEFAULT_SOCKET);
        assert_eq!(theme, Mode::Dark);
        let (socket, theme) = parse_args(vec![
            "--socket=/tmp/interfire.sock".into(),
            "--theme=light".into(),
        ]);
        assert_eq!(socket, "/tmp/interfire.sock");
        assert_eq!(theme, Mode::Light);
    }
}
