//! Network tab: InterFire-owned nftables state and install/remove controls.
#![allow(clippy::wildcard_imports)]
#![forbid(unsafe_code)]

use gpui_kit::component::*;
use gpui_kit::*;
use interfire_proto::{NFQUEUE_NUM, NetworkStatus, NetworkTableState};

use crate::app::App;
use crate::rules_view::action_chip;
use crate::tray::DaemonLink;

pub fn network_body(
    link: &DaemonLink,
    network: Option<&NetworkStatus>,
    message: Option<&str>,
    cx: &Context<App>,
) -> Div {
    let muted = cx.theme().muted_foreground;
    let daemon_up = matches!(link, DaemonLink::Up { .. });
    let installed = network.is_some_and(|status| status.state == NetworkTableState::Installed);
    let table_present = network.is_some_and(|status| status.state != NetworkTableState::Missing);

    let mut body =
        div()
            .v_flex()
            .gap_3()
            .child(
                div()
                    .text_sm()
                    .font_semibold()
                    .child("InterFire-owned nftables"),
            )
            .child(div().text_xs().text_color(muted).child(
                "Only table inet interfire. No system firewall or unrelated tables are edited.",
            ))
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(action_chip(
                        "network-install",
                        "Install queue rule",
                        daemon_up && !installed,
                        true,
                        cx,
                        App::install_network_table,
                    ))
                    .child(action_chip(
                        "network-remove",
                        "Remove table",
                        daemon_up && table_present,
                        false,
                        cx,
                        App::remove_network_table,
                    ))
                    .child(action_chip(
                        "network-refresh",
                        "Refresh",
                        daemon_up,
                        false,
                        cx,
                        App::refresh_network_status,
                    )),
            );

    body = body.child(status_panel(link, network, muted, cx));

    if let Some(message) = message {
        body = body.child(div().text_color(muted).child(message.to_owned()));
    }

    body
}

fn status_panel(
    link: &DaemonLink,
    network: Option<&NetworkStatus>,
    muted: Hsla,
    cx: &Context<App>,
) -> Div {
    let mut panel = div()
        .v_flex()
        .gap_1()
        .px_3()
        .py_2()
        .rounded_md()
        .border_1()
        .border_color(cx.theme().border)
        .bg(cx.theme().popover);

    match link {
        DaemonLink::Down { reason } => {
            return panel
                .child(
                    div()
                        .font_semibold()
                        .text_color(cx.theme().danger)
                        .child("Daemon unavailable"),
                )
                .child(div().text_color(muted).child(reason.clone()));
        }
        DaemonLink::Up { status, .. } => {
            panel = panel.child(div().font_semibold().child("Daemon")).child(
                div().text_color(muted).child(format!(
                    "enforcement={}  observation={}",
                    status.enforcement, status.observation
                )),
            );
        }
    }

    match network {
        None => panel.child(
            div()
                .text_color(muted)
                .child("Network status unavailable (malformed or daemon error)."),
        ),
        Some(network) => panel
            .child(div().pt_2().font_semibold().child("Owned table"))
            .child(div().text_color(muted).child(network.summary()))
            .child(div().text_color(muted).child(guidance(network, link))),
    }
}

fn guidance(network: &NetworkStatus, link: &DaemonLink) -> String {
    let enforcement = match link {
        DaemonLink::Up { status, .. } => status.enforcement.as_str(),
        DaemonLink::Down { .. } => "none",
    };
    match network.state {
        NetworkTableState::Missing => {
            format!(
                "Queue rule not installed. Install to send new outbound TCP to NFQUEUE {NFQUEUE_NUM}."
            )
        }
        NetworkTableState::Incomplete => {
            "Table inet interfire exists but the expected TCP queue rule is missing or mismatched."
                .to_owned()
        }
        NetworkTableState::Installed if enforcement == "nfqueue" => {
            "Owned queue rule present and daemon bound NFQUEUE.".to_owned()
        }
        NetworkTableState::Installed if enforcement == "degraded" => {
            "Owned queue rule present, but daemon NFQUEUE bind is degraded (caps or listener)."
                .to_owned()
        }
        NetworkTableState::Installed => {
            "Owned queue rule present, but daemon enforcement is not nfqueue yet.".to_owned()
        }
    }
}
