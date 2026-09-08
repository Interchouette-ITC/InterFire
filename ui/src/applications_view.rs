//! Applications list and detail for observed process identities.
#![allow(clippy::wildcard_imports)]
#![forbid(unsafe_code)]

use std::process::Command;

use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use interfire_proto::ProcessRow;

use crate::app::App;

/// Optional host process viewers launched from Applications detail (never embedded).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProcessViewer {
    Htop,
    Atop,
    Btop,
    Top,
}

impl ProcessViewer {
    pub const ALL: [Self; 4] = [Self::Htop, Self::Atop, Self::Btop, Self::Top];

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Htop => "htop",
            Self::Atop => "atop",
            Self::Btop => "btop",
            Self::Top => "top",
        }
    }

    #[must_use]
    pub const fn bin(self) -> &'static str {
        self.label()
    }

    /// argv for the viewer process itself (no terminal wrapper).
    #[must_use]
    pub fn argv(self, pid: u32) -> Vec<String> {
        let pid_s = pid.to_string();
        match self {
            // Filter to the selected PID when the tool supports it.
            Self::Htop | Self::Top => vec![self.bin().into(), "-p".into(), pid_s],
            // atop `-p` means "per program", not PID filter; open live view.
            Self::Atop | Self::Btop => vec![self.bin().into()],
        }
    }

    #[must_use]
    pub fn is_on_path(self) -> bool {
        Command::new("sh")
            .args(["-c", &format!("command -v {}", self.bin())])
            .output()
            .is_ok_and(|out| out.status.success())
    }
}

/// Viewers present on `PATH` (order matches [`ProcessViewer::ALL`]).
#[must_use]
pub fn available_viewers() -> Vec<ProcessViewer> {
    ProcessViewer::ALL
        .into_iter()
        .filter(|viewer| viewer.is_on_path())
        .collect()
}

/// Render Applications list + detail (path, pid, ports, optional process viewers).
pub fn applications_body(
    processes: &[ProcessRow],
    selected: Option<(u32, u64)>,
    viewer_message: Option<&str>,
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
        body = body.child(process_detail(row, viewer_message, muted, cx));
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
    viewer_message: Option<&str>,
    muted: Hsla,
    cx: &Context<App>,
) -> Div {
    let pid = row.pid;
    let viewers = available_viewers();
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
        .child(div().font_semibold().mt_2().child("Open in process viewer"));

    if viewers.is_empty() {
        body = body.child(
            div()
                .text_color(muted)
                .child("No htop/atop/btop/top found on PATH."),
        );
    } else {
        let mut row_actions = div().id("open-viewers").flex().gap_2().flex_wrap();
        for viewer in viewers {
            let label = match viewer {
                ProcessViewer::Htop | ProcessViewer::Top => {
                    format!("Open in {} (pid {pid})", viewer.label())
                }
                ProcessViewer::Atop | ProcessViewer::Btop => {
                    format!("Open in {}", viewer.label())
                }
            };
            row_actions = row_actions.child(
                div()
                    .id(ElementId::Name(format!("open-{}", viewer.label()).into()))
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .cursor_pointer()
                    .bg(cx.theme().accent.opacity(0.2))
                    .child(label)
                    .on_click(cx.listener(move |app, _, _, cx| {
                        app.open_process_viewer(viewer, pid, cx);
                    })),
            );
        }
        body = body.child(row_actions);
    }

    if let Some(message) = viewer_message {
        body = body.child(div().text_color(muted).child(message.to_owned()));
    }

    body
}

/// Try to open a host process viewer in a terminal emulator when available.
pub fn try_open_viewer(viewer: ProcessViewer, pid: u32) -> Result<(), String> {
    if !viewer.is_on_path() {
        return Err(format!("{} is not installed (not on PATH)", viewer.label()));
    }
    let argv = viewer.argv(pid);
    let joined = argv.join(" ");
    let gnome_argv = {
        let mut prefixed = vec!["--".to_owned()];
        prefixed.extend(argv.iter().cloned());
        prefixed
    };
    let xdg_argv = argv;
    let candidates: [(&str, Vec<String>); 3] = [
        ("gnome-terminal", gnome_argv),
        ("x-terminal-emulator", vec!["-e".to_owned(), joined]),
        ("xdg-terminal-exec", xdg_argv),
    ];
    for (term, term_argv) in candidates {
        if Command::new(term).args(&term_argv).spawn().is_ok() {
            return Ok(());
        }
    }
    Err(format!(
        "could not launch {} for pid {pid} (install a terminal emulator)",
        viewer.label()
    ))
}

#[cfg(test)]
mod tests {
    use super::ProcessViewer;

    #[test]
    fn htop_and_top_pass_pid_filter() {
        assert_eq!(
            ProcessViewer::Htop.argv(42),
            vec!["htop".to_owned(), "-p".to_owned(), "42".to_owned()]
        );
        assert_eq!(
            ProcessViewer::Top.argv(7),
            vec!["top".to_owned(), "-p".to_owned(), "7".to_owned()]
        );
    }

    #[test]
    fn atop_and_btop_launch_without_fake_pid_flag() {
        assert_eq!(ProcessViewer::Atop.argv(1), vec!["atop".to_owned()]);
        assert_eq!(ProcessViewer::Btop.argv(1), vec!["btop".to_owned()]);
    }
}
