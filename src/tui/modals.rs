use ratatui::{
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
    Frame,
};
use std::collections::BTreeMap;

use crate::api::models::WebhookEndpointStatus;
use crate::tui::app::{App, FormField};
use crate::tui::ui::centered_rect;

pub fn render_create_modal(frame: &mut Frame, app: &App) {
    render_form_modal(frame, app, "Create Webhook",
        "Configure an endpoint to receive event notifications", "Create Webhook");
}

pub fn render_edit_modal(frame: &mut Frame, app: &App) {
    render_form_modal(frame, app, "Edit Webhook",
        "Update endpoint URL, name, status, and event subscriptions", "Save Changes");
}

fn render_form_modal(frame: &mut Frame, app: &App, title: &str, subtitle: &str, submit_label: &str) {
    let area = centered_rect(70, 90, frame.area());
    frame.render_widget(Clear, area);
    let block = Block::default().borders(Borders::ALL)
        .title(format!(" {title} "))
        .title_style(Style::default().fg(Color::Cyan).bold())
        .border_style(Style::default().fg(Color::Cyan))
        .style(Style::default().bg(Color::Black));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(Span::styled(subtitle, Style::default().fg(Color::Green))));
    lines.push(Line::raw(""));

    // URL field
    push_text_field(&mut lines, "Endpoint URL *", &app.form.url,
        app.form.active_field == FormField::Url, "https://api.yourdomain.com/webhooks");
    lines.push(Line::from(Span::styled("  Must be an HTTPS endpoint", Style::default().fg(Color::Green))));
    lines.push(Line::raw(""));

    // Name field
    push_text_field(&mut lines, "Name", &app.form.name,
        app.form.active_field == FormField::Name, "e.g., Order Processing Webhook");
    lines.push(Line::raw(""));

    // Status (edit only)
    if app.form.is_edit {
        push_status_field(&mut lines, app);
        lines.push(Line::raw(""));
    }

    // Events header + Check/Uncheck All
    push_events_header(&mut lines, app);
    lines.push(Line::raw(""));

    // Events grouped by metadata.group
    let groups = group_events(app);
    for (group, indices) in &groups {
        lines.push(Line::from(Span::styled(format!("  {group}"),
            Style::default().fg(Color::White).bold())));
        for &i in indices {
            let et = &app.event_types[i];
            let checked = if app.form.events.get(i).copied().unwrap_or(false) { "☑" } else { "☐" };
            let active = app.form.active_field == FormField::Event(i);
            let pointer = if active { Span::styled("  ▸ ", Style::default().fg(Color::Cyan)) } else { Span::raw("    ") };
            let style = if active { Style::default().fg(Color::Cyan).bold() }
                else if app.form.events.get(i).copied().unwrap_or(false) { Style::default().fg(Color::Green) }
                else { Style::default().fg(Color::White) };
            lines.push(Line::from(vec![pointer, Span::styled(format!("{checked} "), style),
                Span::styled(et.name.clone(), style)]));
            if !et.description.is_empty() {
                lines.push(Line::from(Span::styled(format!("      {}", et.description),
                    Style::default().fg(Color::Green))));
            }
        }
        lines.push(Line::raw(""));
    }

    // Buttons
    push_buttons(&mut lines, app, submit_label);

    let para = Paragraph::new(lines).wrap(Wrap { trim: false }).scroll((app.form.scroll, 0));
    frame.render_widget(para, inner);
}

fn group_events(app: &App) -> Vec<(String, Vec<usize>)> {
    let mut map: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (i, et) in app.event_types.iter().enumerate() {
        map.entry(et.group.clone()).or_default().push(i);
    }
    map.into_iter().collect()
}

fn push_text_field(lines: &mut Vec<Line>, label: &str, value: &str, active: bool, placeholder: &str) {
    let label_style = if active { Style::default().fg(Color::Cyan).bold() } else { Style::default().fg(Color::White) };
    lines.push(Line::from(Span::styled(label.to_string(), label_style)));
    let cursor = if active { "█" } else { "" };
    let display = if value.is_empty() && !active {
        Span::styled(placeholder.to_string(), Style::default().fg(Color::Green))
    } else {
        Span::styled(format!("{value}{cursor}"), Style::default().fg(Color::White))
    };
    let pointer = if active { Span::styled("▸ ", Style::default().fg(Color::Cyan)) } else { Span::raw("  ") };
    lines.push(Line::from(vec![Span::raw(" "), pointer, display]));
}

fn push_status_field(lines: &mut Vec<Line>, app: &App) {
    let active = app.form.active_field == FormField::Status;
    let style = if active { Style::default().fg(Color::Cyan).bold() } else { Style::default().fg(Color::White) };
    lines.push(Line::from(Span::styled("Status *".to_string(), style)));
    let active_marker = if app.form.status == WebhookEndpointStatus::Active { "(●)" } else { "( )" };
    let inactive_marker = if app.form.status == WebhookEndpointStatus::Inactive { "(●)" } else { "( )" };
    let pointer = if active { Span::styled("▸ ", Style::default().fg(Color::Cyan)) } else { Span::raw("  ") };
    lines.push(Line::from(vec![
        Span::raw(" "), pointer,
        Span::styled(format!("{active_marker} Active"), Style::default().fg(Color::Green)),
        Span::raw("  "),
        Span::styled(format!("{inactive_marker} Inactive"), Style::default().fg(Color::Yellow)),
    ]));
}

fn push_events_header(lines: &mut Vec<Line>, app: &App) {
    let check_style = if app.form.active_field == FormField::CheckAll {
        Style::default().fg(Color::Cyan).bold() } else { Style::default().fg(Color::Green) };
    let uncheck_style = if app.form.active_field == FormField::UncheckAll {
        Style::default().fg(Color::Cyan).bold() } else { Style::default().fg(Color::Green) };
    lines.push(Line::from(vec![
        Span::styled("Events *", Style::default().fg(Color::White)),
        Span::raw("  "),
        Span::styled("[Check All]", check_style),
        Span::raw(" "),
        Span::styled("[Uncheck All]", uncheck_style),
    ]));
}

fn push_buttons(lines: &mut Vec<Line>, app: &App, submit_label: &str) {
    let cancel = if app.form.active_field == FormField::Cancel {
        Style::default().fg(Color::Black).bg(Color::White).bold() } else { Style::default().fg(Color::White) };
    let submit = if app.form.active_field == FormField::Submit {
        Style::default().fg(Color::Black).bg(Color::Cyan).bold() } else { Style::default().fg(Color::Cyan) };
    lines.push(Line::raw(""));
    lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled(" Cancel ", cancel),
        Span::raw("  "),
        Span::styled(format!(" {submit_label} "), submit),
    ]));
}

pub fn render_delete_modal(frame: &mut Frame, app: &App, idx: usize) {
    let area = centered_rect(50, 40, frame.area());
    frame.render_widget(Clear, area);
    let block = Block::default().borders(Borders::ALL)
        .title(" ⚠ Delete Webhook ")
        .title_style(Style::default().fg(Color::Red).bold())
        .border_style(Style::default().fg(Color::Red))
        .style(Style::default().bg(Color::Black));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let Some(ep) = app.endpoints.get(idx) else { return; };
    let lines = vec![
        Line::from(Span::styled("Are you sure you want to delete this webhook?", Style::default().fg(Color::White))),
        Line::raw(""),
        Line::from(Span::styled(ep.name.clone(), Style::default().fg(Color::White).bold())),
        Line::from(Span::styled(ep.endpoint_url.clone(), Style::default().fg(Color::Blue))),
        Line::raw(""),
        Line::from(Span::styled(
            "This action cannot be undone. The endpoint will stop receiving events immediately.",
            Style::default().fg(Color::Red))),
        Line::raw(""),
        Line::from(vec![
            Span::styled(" [n/Esc] Cancel ", Style::default().fg(Color::White)),
            Span::raw("  "),
            Span::styled(" [y/Enter] Delete Webhook ", Style::default().fg(Color::Red).bold()),
        ]),
    ];
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

pub fn render_created_modal(frame: &mut Frame, secret: &str) {
    let area = centered_rect(60, 50, frame.area());
    frame.render_widget(Clear, area);
    let block = Block::default().borders(Borders::ALL)
        .title(" ✓ Webhook Created ")
        .title_style(Style::default().fg(Color::Green).bold())
        .border_style(Style::default().fg(Color::Green))
        .style(Style::default().bg(Color::Black));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let lines = vec![
        Line::from(Span::styled("Your webhook has been created.", Style::default().fg(Color::White))),
        Line::from(Span::styled("Copy the signing secret now — it won't be shown again.",
            Style::default().fg(Color::Green))),
        Line::raw(""),
        Line::from(Span::styled("Your Signing Secret:", Style::default().fg(Color::White).bold())),
        Line::from(Span::styled(secret.to_string(), Style::default().fg(Color::Green).add_modifier(Modifier::BOLD))),
        Line::raw(""),
        Line::from(Span::styled(" [Enter/Esc] Done ", Style::default().fg(Color::Cyan).bold())),
    ];
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

pub fn render_details_modal(frame: &mut Frame, app: &App, log_id: &str) {
    let area = centered_rect(75, 90, frame.area());
    frame.render_widget(Clear, area);
    let block = Block::default().borders(Borders::ALL)
        .title(" Delivery Details ")
        .title_style(Style::default().fg(Color::Cyan).bold())
        .border_style(Style::default().fg(Color::Cyan))
        .style(Style::default().bg(Color::Black));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let Some(log) = app.logs.iter().find(|l| l.id == log_id) else {
        let p = Paragraph::new(Span::styled("Loading details…", Style::default().fg(Color::Yellow)));
        frame.render_widget(p, inner);
        return;
    };
    let lines = vec![
        Line::from(Span::styled(format!("Event: {}", log.event_type), Style::default().fg(Color::Blue))),
        Line::from(Span::styled(format!("Endpoint: {}", log.endpoint_name), Style::default().fg(Color::White).bold())),
        Line::from(Span::styled(format!("URL: {}", log.endpoint_url), Style::default().fg(Color::Blue))),
        Line::from(Span::styled(format!("Status: {:?}  HTTP: {:?}  Duration: {}ms  Attempt: {}",
            log.status, log.response_status_code, log.duration_ms, log.attempt_number),
            Style::default().fg(Color::White))),
        Line::raw(""),
        Line::from(Span::styled("Press v on a log row to fetch full request/response from the API",
            Style::default().fg(Color::Green))),
    ];
    let para = Paragraph::new(lines).wrap(Wrap { trim: false }).scroll((app.detail_scroll, 0));
    frame.render_widget(para, inner);
}
