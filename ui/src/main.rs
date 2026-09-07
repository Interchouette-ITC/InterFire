//! `InterFire` GPUI desktop client (`interfire-ui`).
//!
//! GPUI kits expose large prelude surfaces; clippy wildcard bans fight that API.
#![allow(clippy::wildcard_imports)]
#![forbid(unsafe_code)]

mod app;
mod ipc_poll;
mod section;
mod tray;
#[cfg(target_os = "linux")]
mod tray_host;

use std::env;

use gpui_kit::component::*;
use gpui_kit::*;
use interfire_proto::DEFAULT_SOCKET_PATH;

use crate::app::App;

fn main() {
    let socket = parse_socket(env::args().skip(1));

    gpui_kit::application().run(move |cx| {
        gpui_kit::init(cx);
        cx.spawn(async move |cx| {
            cx.open_window(
                WindowOptions {
                    titlebar: Some(TitlebarOptions {
                        title: Some("InterFire".into()),
                        ..TitlebarOptions::default()
                    }),
                    ..WindowOptions::default()
                },
                |window, cx| {
                    let view = cx.new(|cx| {
                        let app = App::new(socket.clone());
                        App::start_watchers(cx);
                        app
                    });
                    cx.new(|cx| Root::new(view, window, cx).bg(cx.theme().background))
                },
            )
            .expect("open InterFire window");
        })
        .detach();
    });
}

fn parse_socket(args: impl IntoIterator<Item = String>) -> String {
    let mut socket = DEFAULT_SOCKET_PATH.to_owned();
    for argument in args {
        if let Some(value) = argument.strip_prefix("--socket=") {
            value.clone_into(&mut socket);
        }
    }
    socket
}

#[cfg(test)]
mod tests {
    use super::parse_socket;
    use interfire_proto::DEFAULT_SOCKET_PATH;

    #[test]
    fn parse_socket_defaults_and_overrides() {
        assert_eq!(parse_socket(Vec::<String>::new()), DEFAULT_SOCKET_PATH);
        assert_eq!(
            parse_socket(vec!["--socket=/tmp/interfire.sock".into()]),
            "/tmp/interfire.sock"
        );
    }
}
