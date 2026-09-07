//! `InterFire` GPUI desktop client (`interfire-ui`).
//!
//! GPUI kits expose large prelude surfaces; clippy wildcard bans fight that API.
#![allow(clippy::wildcard_imports)]
#![forbid(unsafe_code)]

mod app;
mod section;

use gpui_kit::component::*;
use gpui_kit::*;

use crate::app::App;

fn main() {
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
                    let view = cx.new(|_| App::new());
                    cx.new(|cx| Root::new(view, window, cx).bg(cx.theme().background))
                },
            )
            .expect("open InterFire window");
        })
        .detach();
    });
}
