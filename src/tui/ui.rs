use ratatui::{
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table, Tabs},
    Frame,
};

use crate::api::models::{WebhookDeliveryLogStatus, WebhookEndpointStatus};
use crate::tui::app::{App, ModalState, Screen};
use crate::tui::modals;

pub fn render(frame: &mut Frame, app: &App) {
    let chunks = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(0),
        Constraint::Length(1),
    ]).split(frame.area());

    render_tab_bar(frame, app, chunks[0]);
    match app.screen {
        Screen::Endpoints => render_endpoints(frame, app, chunks[1]),
        Screen::DeliveryLogs => render_logs(frame, app, chunks[1]),
    }
    render_help_bar(frame, app, chunks[2]);

    match &app.modal {
        ModalState::None => {
            // When no other modal is open and we have a pending error, show it as a modal
            // so it can't shift the underlying layout out of alignment.
            if let Some(msg) = &app.last_error {
                modals::render_error_modal(frame, msg);
            }
        }
        ModalState::CreateWebhook => modals::render_create_modal(frame, app),
        ModalState::EditWebhook(_) => modals::render_edit_modal(frame, app),
        ModalState::DeleteWebhook(idx) => modals::render_delete_modal(frame, app, *idx),
        ModalState::WebhookCreated(secret) => modals::render_created_modal(frame, secret),
        ModalState::DeliveryDetails(id) => modals::render_details_modal(frame, app, id),
    }

    if let Some(msg) = &app.toast_message { render_toast(frame, msg); }
}

fn render_tab_bar(frame: &mut Frame, app: &App, area: Rect) {
    let titles = [" Endpoints ", " Delivery Logs "];
    let selected = match app.screen { Screen::Endpoints => 0, Screen::DeliveryLogs => 1 };
    let title = match &app.poll_warning {
        Some(w) => format!(" Flute Webhook Dashboard  ⚠ {w} "),
        None => " Flute Webhook Dashboard ".to_string(),
    };
    let title_style = if app.poll_warning.is_some() {
        Style::default().fg(Color::Yellow).bold()
    } else {
        Style::default().fg(Color::Cyan).bold()
    };
    let tabs = Tabs::new(titles.to_vec())
        .block(Block::default().borders(Borders::ALL)
            .title(title).title_style(title_style))
        .select(selected)
        .style(Style::default().fg(Color::Green))
        .highlight_style(Style::default().fg(Color::Black).bg(Color::Green).bold());
    frame.render_widget(tabs, area);
}

fn render_endpoints(frame: &mut Frame, app: &App, area: Rect) {
    if app.endpoints.is_empty() {
        let p = Paragraph::new(Line::from(Span::styled(
            "No webhooks yet. Press [c] to create one.",
            Style::default().fg(Color::Green))).alignment(Alignment::Center))
            .block(Block::default().borders(Borders::ALL));
        frame.render_widget(p, area);
        return;
    }

    let header = Row::new(vec![
        Cell::from("Description"), Cell::from("Endpoint URL"),
        Cell::from("Events"), Cell::from("Status"), Cell::from("Actions"),
    ]).style(Style::default().bold().fg(Color::Green));

    let rows: Vec<Row> = app.endpoints.iter().enumerate().map(|(i, ep)| {
        let status_style = match ep.status {
            WebhookEndpointStatus::Active => Style::default().fg(Color::Green),
            WebhookEndpointStatus::Inactive => Style::default().fg(Color::Yellow),
        };
        let count = if ep.trigger_count_partial {
            format!("{}+", ep.trigger_count)
        } else {
            ep.trigger_count.to_string()
        };
        let row = Row::new(vec![
            Cell::from(ep.name.clone()),
            Cell::from(Span::styled(ep.endpoint_url.clone(), Style::default().fg(Color::Blue))),
            Cell::from(count),
            Cell::from(Span::styled(
                match ep.status {
                    WebhookEndpointStatus::Active => "Active",
                    WebhookEndpointStatus::Inactive => "Inactive",
                },
                status_style,
            )),
            Cell::from("[e]dit [d]el"),
        ]);
        if i == app.selected_endpoint {
            row.style(Style::default().bg(Color::Rgb(0, 80, 0)).fg(Color::White).bold())
        } else { row }
    }).collect();

    let widths = [
        Constraint::Percentage(25), Constraint::Percentage(35),
        Constraint::Percentage(10), Constraint::Percentage(12), Constraint::Percentage(18),
    ];
    let table = Table::new(rows, widths)
        .header(header)
        .block(Block::default().borders(Borders::ALL));
    frame.render_widget(table, area);
}

fn render_logs(frame: &mut Frame, app: &App, area: Rect) {
    let filtered = app.filtered_log_indices();
    let chunks = Layout::vertical([Constraint::Length(3), Constraint::Min(0)]).split(area);

    let filter_line = Line::from(vec![
        Span::styled(format!(" [1] Endpoint: {} ", endpoint_filter_label(app)), Style::default().fg(Color::Cyan)),
        Span::styled(format!(" [2] Event: {} ", event_filter_label(app)), Style::default().fg(Color::Cyan)),
        Span::styled(format!(" [3] Status: {} ", status_filter_label(app)), Style::default().fg(Color::Cyan)),
        Span::styled(format!(" [s] Sort: {} ", if app.sort_ascending { "Asc" } else { "Desc" }), Style::default().fg(Color::Cyan)),
        Span::styled(" [x] Clear ", Style::default().fg(Color::Yellow)),
    ]);
    frame.render_widget(Paragraph::new(filter_line).block(Block::default().borders(Borders::ALL)), chunks[0]);

    if filtered.is_empty() {
        let p = Paragraph::new(Line::from(Span::styled(
            "No delivery logs match these filters.",
            Style::default().fg(Color::Green))).alignment(Alignment::Center))
            .block(Block::default().borders(Borders::ALL));
        frame.render_widget(p, chunks[1]);
        return;
    }

    let header = Row::new(vec![
        Cell::from("Timestamp"), Cell::from("Event Type"), Cell::from("Status"),
        Cell::from("HTTP"), Cell::from("Duration"), Cell::from("Webhook"), Cell::from("Actions"),
    ]).style(Style::default().bold().fg(Color::Green));

    let rows: Vec<Row> = filtered.iter().enumerate().map(|(display_i, &log_i)| {
        let log = &app.logs[log_i];
        let (text, color) = match log.status {
            WebhookDeliveryLogStatus::Success => ("Success", Color::Green),
            WebhookDeliveryLogStatus::Failure => ("Failed", Color::Red),
        };
        let http = log.response_status_code.map(|s| s.to_string()).unwrap_or_else(|| "—".into());
        let row = Row::new(vec![
            Cell::from(log.created_on.format("%m/%d/%y %H:%M:%S").to_string()),
            Cell::from(Span::styled(log.event_type.clone(), Style::default().fg(Color::Blue))),
            Cell::from(Span::styled(text, Style::default().fg(color).bold())),
            Cell::from(http),
            Cell::from(format!("{}ms", log.duration_ms)),
            Cell::from(log.endpoint_name.clone()),
            Cell::from("[v]iew"),
        ]);
        if display_i == app.selected_log {
            row.style(Style::default().bg(Color::Rgb(0, 80, 0)).fg(Color::White).bold())
        } else { row }
    }).collect();

    let widths = [
        Constraint::Length(18), Constraint::Percentage(22), Constraint::Length(9),
        Constraint::Length(6), Constraint::Length(10), Constraint::Percentage(20), Constraint::Length(10),
    ];
    frame.render_widget(Table::new(rows, widths).header(header).block(Block::default().borders(Borders::ALL)), chunks[1]);
}

fn endpoint_filter_label(app: &App) -> String {
    if app.filter_endpoint == 0 { "All".into() }
    else { app.endpoints.get(app.filter_endpoint - 1).map(|e| e.name.clone()).unwrap_or_else(|| "All".into()) }
}
fn event_filter_label(app: &App) -> String {
    if app.filter_event == 0 { "All".into() }
    else { app.event_types.get(app.filter_event - 1).map(|e| e.name.clone()).unwrap_or_else(|| "All".into()) }
}
fn status_filter_label(app: &App) -> &'static str {
    match app.filter_status { 1 => "Success", 2 => "Failed", _ => "All" }
}

fn render_help_bar(frame: &mut Frame, app: &App, area: Rect) {
    let help = match (&app.modal, &app.screen) {
        (ModalState::None, _) if app.last_error.is_some() => "Enter/Esc: dismiss error",
        (ModalState::None, Screen::Endpoints) => "Tab: switch | ↑↓/jk: nav | c: create | e: edit | d: delete | q: quit",
        (ModalState::None, Screen::DeliveryLogs) => "Tab: switch | ↑↓/jk: nav | v: details | 1-3: filters | s: sort | x: clear | q: quit",
        (ModalState::CreateWebhook | ModalState::EditWebhook(_), _) => "Tab/↑↓: fields | Space: toggle | Enter: activate | PgUp/PgDn: scroll | Esc: cancel",
        (ModalState::DeleteWebhook(_), _) => "y/Enter: confirm | n/Esc: cancel",
        (ModalState::WebhookCreated(_), _) => "Enter/Esc: done",
        (ModalState::DeliveryDetails(_), _) => "↑↓/jk/PgUp/PgDn: scroll | Esc/Enter/q: close",
    };
    frame.render_widget(Paragraph::new(Line::from(Span::styled(format!(" {help}"),
        Style::default().fg(Color::Green)))).style(Style::default().bg(Color::Black)), area);
}

fn render_toast(frame: &mut Frame, msg: &str) {
    let area = frame.area();
    let width = u16::try_from(msg.len()).unwrap_or(u16::MAX).saturating_add(4).min(area.width);
    let x = area.width.saturating_sub(width) / 2;
    // Anchor above the help bar (bottom row) so the toast doesn't cover key hints.
    let y = area.height.saturating_sub(4);
    let toast_area = Rect::new(x, y, width, 3);
    frame.render_widget(ratatui::widgets::Clear, toast_area);
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(msg, Style::default().fg(Color::White).bold())))
            .alignment(Alignment::Center)
            .block(Block::default().borders(Borders::ALL).style(Style::default().bg(Color::Black))),
        toast_area,
    );
}

pub fn centered_rect(width_pct: u16, height_pct: u16, area: Rect) -> Rect {
    let v = Layout::vertical([
        Constraint::Percentage((100 - height_pct) / 2),
        Constraint::Percentage(height_pct),
        Constraint::Percentage((100 - height_pct) / 2),
    ]).split(area);
    Layout::horizontal([
        Constraint::Percentage((100 - width_pct) / 2),
        Constraint::Percentage(width_pct),
        Constraint::Percentage((100 - width_pct) / 2),
    ]).split(v[1])[1]
}
