//! Ratatui drawing for tab chrome, panes, footer, and overlay.
#![forbid(unsafe_code)]

use interfire_proto::IPC_VERSION;
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap};

use crate::app::{AddField, AddRuleForm, AnswerPromptForm, AnswerVerdict, App, Overlay, Pane, Tab};
use crate::palette;

pub fn draw(frame: &mut Frame<'_>, app: &App) {
    let chunks = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(3),
        Constraint::Length(1),
    ])
    .split(frame.area());
    frame.render_widget(tabs_bar(app), chunks[0]);
    draw_body(frame, app, chunks[1]);
    frame.render_widget(
        Paragraph::new(crate::app::footer_hints(app)).style(palette::muted()),
        chunks[2],
    );
    match &app.overlay {
        Overlay::None => {}
        Overlay::Notice(message) => draw_notice(frame, message),
        Overlay::AddRule(form) => draw_add_rule(frame, form),
        Overlay::AnswerPrompt(form) => draw_answer_prompt(frame, form),
    }
}

fn tabs_bar(app: &App) -> Paragraph<'static> {
    let mut spans = vec![
        Span::styled("InterFire", palette::title()),
        Span::styled(format!(" v{IPC_VERSION}  "), palette::muted()),
    ];
    for tab in Tab::ALL {
        let label = format!(" {} ", tab.label());
        if tab == app.tab {
            spans.push(Span::styled(label, palette::selected()));
        } else {
            spans.push(Span::styled(label, palette::body()));
        }
    }
    Paragraph::new(Line::from(spans)).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(palette::border())
            .style(palette::chrome())
            .title(Span::styled(
                format!("tabs · {}", app.chrome_title()),
                palette::muted(),
            )),
    )
}

fn draw_body(frame: &mut Frame<'_>, app: &App, area: Rect) {
    if app.tab.has_split() {
        let panes = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
            .split(area);
        let viewport_rows = usize::from(panes[0].height.saturating_sub(2));
        let visible = app.visible_list(viewport_rows);
        let mut state = ListState::default();
        if !visible.items.is_empty() {
            state.select(Some(visible.relative_selected));
        }
        frame.render_stateful_widget(list_widget(app, &visible), panes[0], &mut state);
        frame.render_widget(detail_widget(app), panes[1]);
    } else {
        frame.render_widget(detail_widget(app), area);
    }
}

fn list_widget(app: &App, visible: &crate::app::VisibleList) -> List<'static> {
    let items: Vec<ListItem<'static>> = visible
        .items
        .iter()
        .cloned()
        .map(|line| ListItem::new(line).style(palette::body()))
        .collect();
    let focus = if app.pane == Pane::List {
        " [focus]"
    } else {
        ""
    };
    let title = if app.tab == Tab::Log && visible.total > 0 {
        let shown = visible.start.saturating_add(1);
        let end = visible.start.saturating_add(visible.items.len());
        format!(
            "Log list{focus}  {shown}-{end}/{total}",
            total = visible.total
        )
    } else {
        format!("{} list{focus}", app.tab.label())
    };
    List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(palette::border())
                .style(palette::panel())
                .title(Span::styled(title, palette::muted())),
        )
        .highlight_style(palette::selected())
}

fn detail_widget(app: &App) -> Paragraph<'_> {
    let title = if app.tab.has_split() && app.pane == Pane::Detail {
        format!("{} detail [focus]", app.tab.label())
    } else {
        format!("{} detail", app.tab.label())
    };
    let lines: Vec<Line<'_>> = app
        .detail_lines()
        .into_iter()
        .map(|line| Line::from(Span::styled(line, palette::body())))
        .collect();
    Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .style(palette::panel())
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(palette::border())
                .title(Span::styled(title, palette::muted())),
        )
}

fn draw_notice(frame: &mut Frame<'_>, message: &str) {
    let area = centered_rect(60, 30, frame.area());
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled("Overlay", palette::title())),
            Line::from(""),
            Line::from(Span::styled(message.to_owned(), palette::body())),
            Line::from(""),
            Line::from(Span::styled(
                "Esc dismisses · does not quit",
                palette::muted(),
            )),
        ])
        .style(palette::overlay_panel())
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(palette::border())
                .title(Span::styled("notice", palette::accent_label())),
        ),
        area,
    );
}

fn draw_add_rule(frame: &mut Frame<'_>, form: &AddRuleForm) {
    let area = centered_rect(70, 55, frame.area());
    frame.render_widget(Clear, area);
    let lines = vec![
        field_line(form, AddField::Id, &form.id),
        field_line(form, AddField::Executable, &form.executable),
        field_line(form, AddField::Verdict, &form.verdict),
        field_line(form, AddField::Port, &form.port),
        Line::from(""),
        Line::from(Span::styled(
            "Tab next field · Enter submit · Esc cancel",
            palette::muted(),
        )),
    ];
    frame.render_widget(
        Paragraph::new(lines).style(palette::overlay_panel()).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(palette::border())
                .title(Span::styled("add rule", palette::accent_label())),
        ),
        area,
    );
}

fn draw_answer_prompt(frame: &mut Frame<'_>, form: &AnswerPromptForm) {
    let area = centered_rect(70, 60, frame.area());
    frame.render_widget(Clear, area);
    let prompt = &form.prompt;
    let allow_style = if form.verdict == AnswerVerdict::Allow {
        palette::allow()
    } else {
        palette::body()
    };
    let deny_style = if form.verdict == AnswerVerdict::Deny {
        palette::deny()
    } else {
        palette::body()
    };
    let remaining_style = if prompt.remaining_secs <= 5 {
        palette::err()
    } else if prompt.remaining_secs <= 15 {
        palette::warn()
    } else {
        palette::ok()
    };
    let lines = vec![
        Line::from(Span::styled(
            format!("prompt #{}", prompt.id),
            palette::title(),
        )),
        Line::from(Span::styled(
            format!("path: {}", prompt.executable),
            palette::body(),
        )),
        Line::from(Span::styled(
            format!(
                "dest: {}:{} ({})",
                prompt.destination, prompt.port, prompt.protocol
            ),
            palette::body(),
        )),
        Line::from(Span::styled(
            format!("remaining: {}s", prompt.remaining_secs),
            remaining_style,
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("verdict: ", palette::muted()),
            Span::styled(" allow ", allow_style),
            Span::styled(" deny ", deny_style),
        ]),
        Line::from(Span::styled(
            format!(
                "scope:   {}  (Tab cycles once|session|permanent)",
                form.scope.as_str()
            ),
            palette::body(),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "a/d or Left/Right verdict · Enter submit · Esc cancel",
            palette::muted(),
        )),
    ];
    frame.render_widget(
        Paragraph::new(lines).style(palette::overlay_panel()).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(palette::border())
                .title(Span::styled("answer prompt", palette::accent_label())),
        ),
        area,
    );
}

fn field_line(form: &AddRuleForm, field: AddField, value: &str) -> Line<'static> {
    let marker = if form.focus == field { ">" } else { " " };
    let style = if form.focus == field {
        palette::focus()
    } else {
        palette::body()
    };
    Line::from(Span::styled(
        format!("{marker} {}: {value}", field.label()),
        style,
    ))
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let vertical = Layout::vertical([
        Constraint::Percentage((100 - percent_y) / 2),
        Constraint::Percentage(percent_y),
        Constraint::Percentage((100 - percent_y) / 2),
    ])
    .split(area);
    Layout::horizontal([
        Constraint::Percentage((100 - percent_x) / 2),
        Constraint::Percentage(percent_x),
        Constraint::Percentage((100 - percent_x) / 2),
    ])
    .split(vertical[1])[1]
}

#[cfg(test)]
mod tests {
    use interfire_proto::{AuditStreamRecord, DaemonStatus, ProcessRow, PromptRow, RuleRow};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::draw;
    use crate::app::{
        AddField, AddRuleForm, AnswerPromptForm, AnswerScope, AnswerVerdict, App, Overlay, Pane,
        Tab,
    };
    use crate::palette::{self, Mode};

    fn draw_app(app: &App) {
        let backend = TestBackend::new(120, 40);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal.draw(|frame| draw(frame, app)).expect("draw frame");
    }

    fn sample_app(mode: Mode) -> App {
        palette::set_mode(mode);
        let mut app = App::new("/tmp/interfire-tui-draw.sock".into());
        app.apply(crate::ipc::IpcEvent::Status(DaemonStatus {
            enforcement: "nfqueue".into(),
            observation: "attached".into(),
            ipc_version: 1,
            pid: Some(42),
            rss_kib: Some(6400),
            cpu_jiffies: Some(10),
        }));
        app.subscribed = true;
        app.rules = vec![RuleRow {
            id: 1,
            executable: "/bin/curl".into(),
            verdict: "allow".into(),
            port: 443,
        }];
        app.prompts = vec![PromptRow {
            id: 2,
            executable: "/bin/curl".into(),
            destination: "203.0.113.1".into(),
            port: 443,
            protocol: "tcp".into(),
            remaining_secs: 30,
        }];
        app.processes = vec![ProcessRow {
            pid: 100,
            start_ticks: 50,
            uid: 1000,
            executable: "/bin/curl".into(),
            cmdline: "curl -s".into(),
            verdict: "allow".into(),
            ports: "203.0.113.1:443/allow".into(),
        }];
        for sequence in 1..=25 {
            app.apply(crate::ipc::IpcEvent::Audit(AuditStreamRecord {
                sequence,
                message: format!("audit-{sequence}"),
            }));
        }
        app.status_message = Some("ready".into());
        app
    }

    #[test]
    fn draws_every_tab_in_dark_and_light_modes() {
        for mode in [Mode::Dark, Mode::Light] {
            let mut app = sample_app(mode);
            for tab in Tab::ALL {
                app.tab = tab;
                app.pane = if tab.has_split() {
                    Pane::Detail
                } else {
                    Pane::List
                };
                draw_app(&app);
            }
        }
    }

    #[test]
    fn draws_list_focus_and_log_window_titles() {
        let mut app = sample_app(Mode::Dark);
        app.tab = Tab::Log;
        app.pane = Pane::List;
        app.list_selected = 20;
        draw_app(&app);
        app.pane = Pane::Detail;
        draw_app(&app);
    }

    #[test]
    fn draws_notice_add_rule_and_answer_overlays() {
        let mut app = sample_app(Mode::Dark);
        app.overlay = Overlay::Notice("test notice".into());
        draw_app(&app);

        app.overlay = Overlay::AddRule(AddRuleForm {
            id: "9".into(),
            executable: "/bin/curl".into(),
            verdict: "deny".into(),
            port: "443".into(),
            focus: AddField::Executable,
        });
        draw_app(&app);

        for (remaining, verdict) in [
            (3_u64, AnswerVerdict::Allow),
            (10, AnswerVerdict::Deny),
            (30, AnswerVerdict::Deny),
        ] {
            app.overlay = Overlay::AnswerPrompt(AnswerPromptForm {
                prompt: PromptRow {
                    id: 2,
                    executable: "/bin/curl".into(),
                    destination: "203.0.113.1".into(),
                    port: 443,
                    protocol: "tcp".into(),
                    remaining_secs: remaining,
                },
                verdict,
                scope: AnswerScope::Session,
            });
            draw_app(&app);
        }
    }

    #[test]
    fn draws_connecting_and_down_link_states() {
        palette::set_mode(Mode::Light);
        let mut app = App::new("/tmp/interfire-tui-down.sock".into());
        draw_app(&app);
        app.apply(crate::ipc::IpcEvent::Down("connect refused".into()));
        draw_app(&app);
        app.apply(crate::ipc::IpcEvent::Status(DaemonStatus {
            enforcement: "none".into(),
            observation: "degraded".into(),
            ipc_version: 1,
            pid: None,
            rss_kib: None,
            cpu_jiffies: None,
        }));
        draw_app(&app);
    }
}
