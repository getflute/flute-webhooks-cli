use chrono::Utc;
use flute_webhook::api::models::WebhookEndpointStatus;
use flute_webhook::domain::{Endpoint, EventTypeMeta};
use flute_webhook::tui::app::App;
use flute_webhook::tui::ui::render;
use ratatui::{backend::TestBackend, Terminal};

fn endpoint(name: &str, subscribed_event_types: Vec<&str>) -> Endpoint {
    Endpoint {
        id: name.into(),
        name: name.into(),
        endpoint_url: format!("https://example.com/{name}"),
        event_types: subscribed_event_types.into_iter().map(String::from).collect(),
        status: WebhookEndpointStatus::Active,
        created_on: Some(Utc::now()),
    }
}

fn meta(name: &str) -> EventTypeMeta {
    EventTypeMeta { name: name.into(), description: String::new(), group: "Test".into() }
}

#[test]
fn endpoints_table_shows_subscribed_event_count_not_strings() {
    let mut app = App::new(None);
    app.event_types = vec![
        meta("transaction.card.captured"),
        meta("transaction.card.declined"),
        meta("payment.failed"),
        meta("settlement.batch.completed"),
    ]; // total = 4
    app.endpoints = vec![
        // Subscribed to 2 of 4 event types — expect "2/4".
        endpoint("Order Processing", vec!["transaction.card.captured", "payment.failed"]),
        // Subscribed to all 4 — expect "All".
        endpoint("Hot path", vec![
            "transaction.card.captured",
            "transaction.card.declined",
            "payment.failed",
            "settlement.batch.completed",
        ]),
    ];

    let backend = TestBackend::new(120, 20);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|f| render(f, &app)).unwrap();
    let buf = terminal.backend().buffer().clone();
    let text = buf_to_string(&buf);

    assert!(text.contains("Order Processing"), "row missing: {text}");
    assert!(text.contains("2/4"), "subscribed count missing: {text}");
    assert!(text.contains("All"), "All marker missing: {text}");
    // The column should NEVER show the event-type strings themselves.
    assert!(!text.contains("transaction.card.captured"),
        "events column should be a count, not the event-type names: {text}");
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
