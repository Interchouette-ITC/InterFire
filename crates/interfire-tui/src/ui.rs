//! Ratatui drawing for tab chrome, panes, footer, and overlay.
#![forbid(unsafe_code)]

use interfire_proto::IPC_VERSION;
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap};

use crate::app::{AddField, AddRuleForm, AnswerPromptForm, AnswerVerdict, App, Overlay, Pane, Tab};

pub fn draw(frame: &mut Frame<'_>, app: &App) {
    let chunks = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(3),
        Constraint::Length(1),
    ])
    .split(frame.area());
    frame.render_widget(tabs_bar(app), chunks[0]);
    draw_body(frame, app, chunks[1]);
    frame.render_widget(Paragraph::new(crate::app::footer_hints(app)), chunks[2]);
    match &app.overlay {
        Overlay::None => {}
        Overlay::Notice(message) => draw_notice(frame, message),
        Overlay::AddRule(form) => draw_add_rule(frame, form),
        Overlay::AnswerPrompt(form) => draw_answer_prompt(frame, form),
    }
}

fn tabs_bar(app: &App) -> Paragraph<'static> {
    let mut spans = vec![
        Span::styled("InterFire", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(format!(" v{IPC_VERSION}  ")),
    ];
    for tab in Tab::ALL {
        let label = format!(" {} ", tab.label());
        if tab == app.tab {
            spans.push(Span::styled(
                label,
                Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD),
            ));
        } else {
            spans.push(Span::raw(label));
        }
    }
    Paragraph::new(Line::from(spans)).block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!("tabs · {}", app.chrome_title())),
    )
}

fn draw_body(frame: &mut Frame<'_>, app: &App, area: Rect) {
    if app.tab.has_split() {
        let panes = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
            .split(area);
        let mut state = list_state(app);
        frame.render_stateful_widget(list_widget(app), panes[0], &mut state);
        frame.render_widget(detail_widget(app), panes[1]);
    } else {
        frame.render_widget(detail_widget(app), area);
    }
}

fn list_state(app: &App) -> ListState {
    let mut state = ListState::default();
    if app.list_len() > 0 {
        state.select(Some(app.list_selected));
    }
    state
}

fn list_widget(app: &App) -> List<'static> {
    let items: Vec<ListItem<'static>> = app.list_items().into_iter().map(ListItem::new).collect();
    let title = if app.pane == Pane::List {
        format!("{} list [focus]", app.tab.label())
    } else {
        format!("{} list", app.tab.label())
    };
    List::new(items)
        .block(Block::default().borders(Borders::ALL).title(title))
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
}

fn detail_widget(app: &App) -> Paragraph<'_> {
    let title = if app.tab.has_split() && app.pane == Pane::Detail {
        format!("{} detail [focus]", app.tab.label())
    } else {
        format!("{} detail", app.tab.label())
    };
    let lines: Vec<Line<'_>> = app.detail_lines().into_iter().map(Line::from).collect();
    Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .block(Block::default().borders(Borders::ALL).title(title))
}

fn draw_notice(frame: &mut Frame<'_>, message: &str) {
    let area = centered_rect(60, 30, frame.area());
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(
                "Overlay",
                Style::default().add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from(message.to_owned()),
            Line::from(""),
            Line::from("Esc dismisses · does not quit"),
        ])
        .block(Block::default().borders(Borders::ALL).title("notice")),
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
        Line::from("Tab next field · Enter submit · Esc cancel"),
    ];
    frame.render_widget(
        Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title("add rule")),
        area,
    );
}

fn draw_answer_prompt(frame: &mut Frame<'_>, form: &AnswerPromptForm) {
    let area = centered_rect(70, 60, frame.area());
    frame.render_widget(Clear, area);
    let prompt = &form.prompt;
    let allow_style = if form.verdict == AnswerVerdict::Allow {
        Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD)
    } else {
        Style::default()
    };
    let deny_style = if form.verdict == AnswerVerdict::Deny {
        Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD)
    } else {
        Style::default()
    };
    let lines = vec![
        Line::from(format!("prompt #{}", prompt.id)),
        Line::from(format!("path: {}", prompt.executable)),
        Line::from(format!(
            "dest: {}:{} ({})",
            prompt.destination, prompt.port, prompt.protocol
        )),
        Line::from(format!("remaining: {}s", prompt.remaining_secs)),
        Line::from(""),
        Line::from(vec![
            Span::raw("verdict: "),
            Span::styled(" allow ", allow_style),
            Span::styled(" deny ", deny_style),
        ]),
        Line::from(format!(
            "scope:   {}  (Tab cycles once|session|permanent)",
            form.scope.as_str()
        )),
        Line::from(""),
        Line::from("a/d or Left/Right verdict · Enter submit · Esc cancel"),
    ];
    frame.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .title("answer prompt"),
        ),
        area,
    );
}

fn field_line(form: &AddRuleForm, field: AddField, value: &str) -> Line<'static> {
    let marker = if form.focus == field { ">" } else { " " };
    let style = if form.focus == field {
        Style::default().add_modifier(Modifier::BOLD)
    } else {
        Style::default()
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
