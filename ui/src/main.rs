//! `InterFire` GPUI desktop client (`interfire-ui`).
//!
//! GPUI kits expose large prelude surfaces; clippy wildcard bans fight that API.
#![allow(clippy::wildcard_imports)]
#![forbid(unsafe_code)]

mod alert;
mod alert_view;
mod app;
mod applications_view;
mod audit_host;
mod brand;
mod ipc_poll;
mod log_buf;
mod log_view;
mod network_view;
mod proc_sample;
mod rss_probe;
mod rules;
mod rules_view;
mod section;
mod theme;
mod tray;
#[cfg(target_os = "linux")]
mod tray_host;

use std::env;
use std::process::Command;
use std::time::Duration;

use gpui_kit::component::*;
use gpui_kit::*;
use interfire_proto::DEFAULT_SOCKET_PATH;

use crate::app::App;
use crate::rss_probe::RssProbeMode;

#[global_allocator]
static ALLOC: hotpath::CountingAllocator = hotpath::CountingAllocator::new();

fn main() {
    // Weak GPUs cannot run GPUI/wgpu natively; default to CPU software GL unless
    // the operator opts into native GPU or already set WGPU_BACKEND (e.g. memcheck).
    apply_default_renderer_env();

    // When built with `--features hotpath` (and optionally `hotpath-alloc`), print a
    // report after `HOTPATH_SHUTDOWN_MS` (default off; `make profile-ui` sets 8s).
    hotpath::HotpathGuardBuilder::new(concat!(module_path!(), "::main"))
        .build_with_shutdown(Duration::from_millis(profile_shutdown_ms()));

    let (socket, probe) = parse_args(env::args().skip(1));

    gpui_kit::application().run(move |cx| {
        gpui_kit::init(cx);
        theme::apply_phoenix_theme(
            theme::ChromeMode::from_appearance(cx.window_appearance()),
            None,
            cx,
        );
        cx.set_app_identity("net.interchouette.InterFire", "InterFire");
        cx.spawn(async move |cx| {
            cx.open_window(
                WindowOptions {
                    titlebar: Some(TitlebarOptions {
                        title: Some("InterFire".into()),
                        ..TitlebarOptions::default()
                    }),
                    window_background: WindowBackgroundAppearance::Opaque,
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
                    view.update(cx, |app, cx| {
                        let mode = app.chrome_preference().resolve(window.appearance());
                        theme::apply_phoenix_theme(mode, Some(window), cx);
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

/// Decision for default graphics env (unit-tested; no process mutation).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RendererEnvDecision {
    LeaveAlone,
    ApplySoftware,
}

fn renderer_env_decision(
    native_gpu: Option<&str>,
    wgpu_backend: Option<&str>,
) -> RendererEnvDecision {
    if native_gpu == Some("1") {
        return RendererEnvDecision::LeaveAlone;
    }
    if wgpu_backend.is_some() {
        return RendererEnvDecision::LeaveAlone;
    }
    RendererEnvDecision::ApplySoftware
}

/// Re-exec with software GL when needed (`#![forbid(unsafe_code)]` blocks `env::set_var`).
fn apply_default_renderer_env() {
    let native = env::var("INTERFIRE_UI_NATIVE_GPU").ok();
    let wgpu = env::var("WGPU_BACKEND").ok();
    if renderer_env_decision(native.as_deref(), wgpu.as_deref())
        != RendererEnvDecision::ApplySoftware
    {
        return;
    }
    reexec_with_software_gl();
}

fn reexec_with_software_gl() -> ! {
    let exe = env::current_exe().expect("current_exe for software-GL re-exec");
    let mut cmd = Command::new(exe);
    cmd.env("LIBGL_ALWAYS_SOFTWARE", "1")
        .env("WGPU_BACKEND", "gl")
        .args(env::args_os().skip(1));
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let err = cmd.exec();
        panic!("software-GL re-exec failed: {err}");
    }
    #[cfg(not(unix))]
    {
        let status = cmd.status().expect("software-GL child for non-unix host");
        std::process::exit(status.code().unwrap_or(1));
    }
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
    use super::{RendererEnvDecision, parse_args, renderer_env_decision};
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

    #[test]
    fn renderer_env_defaults_to_software() {
        assert_eq!(
            renderer_env_decision(None, None),
            RendererEnvDecision::ApplySoftware
        );
    }

    #[test]
    fn renderer_env_respects_native_gpu_flag() {
        assert_eq!(
            renderer_env_decision(Some("1"), None),
            RendererEnvDecision::LeaveAlone
        );
        assert_eq!(
            renderer_env_decision(Some("0"), None),
            RendererEnvDecision::ApplySoftware
        );
    }

    #[test]
    fn renderer_env_respects_explicit_wgpu_backend() {
        assert_eq!(
            renderer_env_decision(None, Some("gl")),
            RendererEnvDecision::LeaveAlone
        );
        assert_eq!(
            renderer_env_decision(None, Some("vulkan")),
            RendererEnvDecision::LeaveAlone
        );
        assert_eq!(
            renderer_env_decision(Some("1"), Some("vulkan")),
            RendererEnvDecision::LeaveAlone
        );
    }
}
