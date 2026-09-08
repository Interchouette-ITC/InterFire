//! `InterFire` GPUI desktop client (`interfire-ui`).
//!
//! GPUI kits expose large prelude surfaces; clippy wildcard bans fight that API.
#![allow(clippy::wildcard_imports)]
#![forbid(unsafe_code)]

mod alert;
mod alert_view;
mod app;
mod audit_host;
mod ipc_poll;
mod log_buf;
mod log_view;
mod rss_probe;
mod rules;
mod rules_view;
mod section;
mod tray;
#[cfg(target_os = "linux")]
mod tray_host;

use std::env;
use std::time::Duration;

use gpui_kit::component::*;
use gpui_kit::*;
use interfire_proto::DEFAULT_SOCKET_PATH;

use crate::app::App;
use crate::rss_probe::RssProbeMode;

#[global_allocator]
static ALLOC: hotpath::CountingAllocator = hotpath::CountingAllocator::new();

fn main() {
    // When built with `--features hotpath` (and optionally `hotpath-alloc`), print a
    // report after `HOTPATH_SHUTDOWN_MS` (default off; `make profile-ui` sets 8s).
    hotpath::HotpathGuardBuilder::new(concat!(module_path!(), "::main"))
        .build_with_shutdown(Duration::from_millis(profile_shutdown_ms()));

    let (socket, probe) = parse_args(env::args().skip(1));

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
                        let mut app = App::new(socket.clone());
                        if let Some(mode) = probe {
                            app.apply_rss_probe(mode);
                        }
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

fn profile_shutdown_ms() -> u64 {
    env::var("HOTPATH_SHUTDOWN_MS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(0)
}

fn parse_args(args: impl IntoIterator<Item = String>) -> (String, Option<RssProbeMode>) {
    let mut socket = DEFAULT_SOCKET_PATH.to_owned();
    let mut probe = None;
    for argument in args {
        if let Some(value) = argument.strip_prefix("--socket=") {
            value.clone_into(&mut socket);
        } else if let Some(value) = argument.strip_prefix("--rss-probe=") {
            probe = RssProbeMode::parse(value);
        }
    }
    (socket, probe)
}

#[cfg(test)]
mod tests {
    use super::parse_args;
    use crate::rss_probe::RssProbeMode;
    use interfire_proto::DEFAULT_SOCKET_PATH;

    #[test]
    fn parse_args_defaults_and_overrides() {
        let (socket, probe) = parse_args(Vec::<String>::new());
        assert_eq!(socket, DEFAULT_SOCKET_PATH);
        assert_eq!(probe, None);
        let (socket, probe) = parse_args(vec![
            "--socket=/tmp/interfire.sock".into(),
            "--rss-probe=prompt-load".into(),
        ]);
        assert_eq!(socket, "/tmp/interfire.sock");
        assert_eq!(probe, Some(RssProbeMode::PromptLoad));
    }
}
