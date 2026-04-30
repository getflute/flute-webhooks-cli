use chrono::Utc;
use flute_webhook::api::models::WebhookEndpointStatus;
use flute_webhook::domain::Endpoint;
use flute_webhook::tui::app::App;
use flute_webhook::tui::ui::render;
use ratatui::{backend::TestBackend, Terminal};

fn endpoint(name: &str, count: u32, partial: bool) -> Endpoint {
    Endpoint {
        id: name.into(), name: name.into(),
        endpoint_url: format!("https://example.com/{name}"),
        event_types: vec!["transaction.card.captured".into()],
        status: WebhookEndpointStatus::Active,
        created_on: Some(Utc::now()),
        trigger_count: count, trigger_count_partial: partial,
    }
}

#[test]
fn endpoints_table_shows_trigger_count_not_strings() {
    let mut app = App::new(None);
    app.endpoints = vec![endpoint("Order Processing", 17, false), endpoint("Hot path", 200, true)];

    let backend = TestBackend::new(120, 20);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|f| render(f, &app)).unwrap();
    let buf = terminal.backend().buffer().clone();
    let text = buf_to_string(&buf);

    assert!(text.contains("Order Processing"), "row missing: {text}");
    assert!(text.contains("17"), "count missing: {text}");
    assert!(text.contains("200+"), "partial-count marker missing: {text}");
    // The column should NOT show a comma-joined event-type list
    assert!(!text.contains("transaction.card.captured"), "events should be a count, not strings: {text}");
}

fn buf_to_string(buf: &ratatui::buffer::Buffer) -> String {
    let mut out = String::new();
    for y in 0..buf.area.height {
        for x in 0..buf.area.width {
            out.push_str(buf.cell((x, y)).map(|c| c.symbol()).unwrap_or(""));
        }
        out.push('\n');
    }
    out
}
