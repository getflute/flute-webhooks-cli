use chrono::Utc;
use flute_webhooks_cli::api::models::WebhookEndpointStatus;
use flute_webhooks_cli::domain::{Endpoint, EventTypeMeta};
use flute_webhooks_cli::tui::app::App;
use flute_webhooks_cli::tui::ui::render;
use ratatui::{Terminal, backend::TestBackend};

fn endpoint(name: &str, subscribed_event_types: Vec<&str>) -> Endpoint {
    Endpoint {
        id: name.into(),
        name: name.into(),
        endpoint_url: format!("https://example.com/{name}"),
        event_types: subscribed_event_types
            .into_iter()
            .map(String::from)
            .collect(),
        status: WebhookEndpointStatus::Active,
        created_on: Some(Utc::now()),
    }
}

fn meta(name: &str) -> EventTypeMeta {
    EventTypeMeta {
        name: name.into(),
        description: String::new(),
        group: "Test".into(),
    }
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
        endpoint(
            "Order Processing",
            vec!["transaction.card.captured", "payment.failed"],
        ),
        // Subscribed to all 4 — expect "All".
        endpoint(
            "Hot path",
            vec![
                "transaction.card.captured",
                "transaction.card.declined",
                "payment.failed",
                "settlement.batch.completed",
            ],
        ),
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
    assert!(
        !text.contains("transaction.card.captured"),
        "events column should be a count, not the event-type names: {text}"
    );
}

#[test]
fn update_notice_renders_dismissable_modal_overlay() {
    let mut app = App::new(None);
    app.set_update_notice(Some("9.9.9".into()));

    let backend = TestBackend::new(120, 20);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|f| render(f, &app)).unwrap();
    let text = buf_to_string(&terminal.backend().buffer().clone());

    assert!(
        text.contains("Update Available"),
        "modal should announce itself: {text}"
    );
    assert!(text.contains("9.9.9"), "modal should embed the version");
    assert!(
        text.contains("flute-webhooks update"),
        "modal should tell the user how to install"
    );
    assert!(
        text.contains("Dismiss"),
        "modal should advertise the dismiss action"
    );
}

#[test]
fn update_notice_can_be_dismissed_with_enter() {
    use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
    let mut app = App::new(None);
    app.set_update_notice(Some("9.9.9".into()));
    assert!(app.update_notice.is_some());

    let _ = app.handle_key(KeyEvent::new_with_kind(
        KeyCode::Enter,
        KeyModifiers::NONE,
        KeyEventKind::Press,
    ));
    assert!(
        app.update_notice.is_none(),
        "Enter should dismiss the update-available modal"
    );
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
