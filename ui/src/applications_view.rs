//! Applications list and detail for observed process identities.
#![allow(clippy::wildcard_imports)]
#![forbid(unsafe_code)]

use std::process::Command;

use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use interfire_proto::ProcessRow;

use crate::app::App;

/// Render Applications list + detail (path, pid, ports, optional htop).
pub fn applications_body(
    processes: &[ProcessRow],
    selected: Option<(u32, u64)>,
    htop_message: Option<&str>,
    cx: &Context<App>,
) -> Div {
    let muted = cx.theme().muted_foreground;
    let mut body = div()
        .v_flex()
        .gap_2()
        .child(div().font_semibold().child("Observed processes"))
        .child(
            div()
                .text_color(muted)
                .text_xs()
                .child("Firewall-attributed identities only (not a full system process list)."),
        )
        .child(process_table(processes, selected, cx));

    if let Some(key) = selected
        && let Some(row) = processes
            .iter()
            .find(|row| row.pid == key.0 && row.start_ticks == key.1)
    {
        body = body.child(process_detail(row, htop_message, muted, cx));
    } else {
        body = body.child(div().text_color(muted).child(
            "Select a process to see path, cmdline, uid, start ticks, and recent destinations.",
        ));
    }

    body
}

fn process_table(
    processes: &[ProcessRow],
    selected: Option<(u32, u64)>,
    cx: &Context<App>,
) -> AnyElement {
    let mut table = div()
        .id("applications-list")
        .v_flex()
        .gap_1()
        .max_h(px(360.))
        .overflow_y_scroll();
    if processes.is_empty() {
        return table
            .child(
                div()
                    .text_color(cx.theme().muted_foreground)
                    .child("No observed processes yet. Outbound connects fill this list."),
            )
            .into_any_element();
    }
    for row in processes {
        let key = (row.pid, row.start_ticks);
        let selected_row = selected == Some(key);
        let label = row.list_label();
        table = table.child(
            div()
                .id(ElementId::Name(
                    format!("proc-{}-{}", row.pid, row.start_ticks).into(),
                ))
                .px_2()
                .py_1()
                .rounded_md()
                .cursor_pointer()
                .when(selected_row, |this| {
                    this.bg(cx.theme().accent)
                        .text_color(cx.theme().accent_foreground)
                })
                .when(!selected_row, |this| {
                    this.hover(|style| style.bg(cx.theme().accent.opacity(0.15)))
                })
                .child(label)
                .on_click(cx.listener(move |app, _, _, cx| {
                    app.select_process(key.0, key.1, cx);
                })),
        );
    }
    table.into_any_element()
}

fn process_detail(
    row: &ProcessRow,
    htop_message: Option<&str>,
    muted: Hsla,
    cx: &Context<App>,
) -> Div {
    let pid = row.pid;
    let mut body = div()
        .v_flex()
        .gap_1()
        .pt_2()
        .border_t_1()
        .child(div().font_semibold().child("Selected"))
        .child(
            div()
                .text_color(muted)
                .child(format!("path: {}", row.executable)),
        )
        .child(
            div()
                .text_color(muted)
                .child(format!("cmdline: {}", row.cmdline)),
        )
        .child(div().text_color(muted).child(format!(
            "pid: {}  start_ticks: {}  uid: {}",
            row.pid, row.start_ticks, row.uid
        )))
        .child(
            div()
                .text_color(muted)
                .child(format!("effective rule: {}", row.verdict)),
        )
        .child(div().font_semibold().mt_2().child("Recent destinations"))
        .child(div().text_color(muted).child(if row.ports.is_empty() {
            "none yet".to_owned()
        } else {
            row.ports.replace('+', ", ")
        }))
        .child(
            div()
                .id("open-htop")
                .mt_2()
                .px_2()
                .py_1()
                .rounded_md()
                .cursor_pointer()
                .bg(cx.theme().accent.opacity(0.2))
                .child(format!("Open in htop (pid {pid})"))
                .on_click(cx.listener(move |app, _, _, cx| {
                    app.open_htop(pid, cx);
                })),
        );

    if let Some(message) = htop_message {
        body = body.child(div().text_color(muted).child(message.to_owned()));
    }

    body
}

/// Try to open `htop -p PID` in a terminal emulator when available.
pub fn try_open_htop(pid: u32) -> Result<(), String> {
    let pid_s = pid.to_string();
    let candidates: [(&str, Vec<String>); 3] = [
        (
            "gnome-terminal",
            vec!["--".into(), "htop".into(), "-p".into(), pid_s.clone()],
        ),
        (
            "x-terminal-emulator",
            vec!["-e".into(), format!("htop -p {pid}")],
        ),
        ("xdg-terminal-exec", vec!["htop".into(), "-p".into(), pid_s]),
    ];
    for (bin, args) in candidates {
        if Command::new(bin).args(&args).spawn().is_ok() {
            return Ok(());
        }
    }
    Err(format!(
        "could not launch htop for pid {pid} (install htop and a terminal emulator)"
    ))
}
