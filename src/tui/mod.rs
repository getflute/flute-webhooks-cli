pub mod app;
pub mod modals;
pub mod ui;

use std::io;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Context};
use crossterm::{
    event::{self, Event},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::prelude::*;
use tokio::sync::{mpsc, watch};

use crate::api::ApiClient;
use crate::auth::{keychain, token::{OAuth2Fetcher, TokenStore}};
use crate::config::{self, validate_poll_interval, Profile};
use crate::poller::{self, CadenceMode, PollerEvent};
use crate::tui::app::{App, AppAction};

pub async fn run(profile_name: &str) -> anyhow::Result<()> {
    let profile = Profile::by_name(profile_name)
        .ok_or_else(|| anyhow!("unknown profile: {profile_name}"))?;
    let cfg = config::load_or_default();
    let validated = validate_poll_interval(cfg.poll_interval_seconds);

    let (id, secret) = keychain::load_with_env_fallback(profile_name)?
        .ok_or_else(|| anyhow!("no credentials for [{profile_name}]; run `flute-webhook auth login`"))?;

    let http = reqwest::Client::builder().timeout(Duration::from_secs(15)).build()?;
    let fetcher = Arc::new(OAuth2Fetcher {
        oauth_url: profile.oauth_url.clone(),
        client_id: id, client_secret: secret, http: http.clone(),
    });
    let api = ApiClient {
        base_url: profile.api_base_url.clone(),
        http, tokens: TokenStore::new(fetcher),
    };

    let (cadence_tx, cadence_rx) = watch::channel(CadenceMode::Active);
    let (events_tx, mut events_rx) = mpsc::channel::<PollerEvent>(8);
    let (action_tx, mut action_rx) = mpsc::channel::<AppAction>(8);
    let (outcome_tx, mut outcome_rx) = mpsc::channel::<crate::tui::app::ActionOutcome>(8);

    let _poller_handle = poller::spawn(api.clone(), cadence_rx, validated.seconds, events_tx);

    let api_for_actions = api.clone();
    let outcome_tx_for_executor = outcome_tx.clone();
    tokio::spawn(async move {
        while let Some(action) = action_rx.recv().await {
            execute_action(&api_for_actions, action, &outcome_tx_for_executor).await;
        }
    });

    install_panic_hook();
    enable_raw_mode().context("enable_raw_mode")?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen).context("EnterAlternateScreen")?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new(validated.warning);

    let res = event_loop(&mut terminal, &mut app, &mut events_rx, &mut outcome_rx, &cadence_tx, &action_tx).await;

    disable_raw_mode().ok();
    execute!(terminal.backend_mut(), LeaveAlternateScreen).ok();
    terminal.show_cursor().ok();
    res
}

async fn event_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
    events_rx: &mut mpsc::Receiver<PollerEvent>,
    outcome_rx: &mut mpsc::Receiver<crate::tui::app::ActionOutcome>,
    cadence_tx: &watch::Sender<CadenceMode>,
    action_tx: &mpsc::Sender<AppAction>,
) -> anyhow::Result<()> {
    let mut last_mode = app.cadence_mode();
    while app.running {
        terminal.draw(|f| ui::render(f, app))?;

        while let Ok(ev) = events_rx.try_recv() {
            match ev {
                PollerEvent::Snapshot(s) => app.apply_snapshot(s.endpoints, s.logs, s.event_types),
                // Surface poller errors in the persistent error banner so the user has time
                // to read them (Esc on the main screen dismisses).
                PollerEvent::Error(e) => app.last_error = Some(format!("Poll error: {e}")),
            }
        }
        while let Ok(o) = outcome_rx.try_recv() { app.apply_outcome(o); }

        if event::poll(Duration::from_millis(100))?
            && let Event::Key(key) = event::read()?
        {
            let action = app.handle_key(key);
            if !matches!(action, AppAction::None)
                && let Err(tokio::sync::mpsc::error::TrySendError::Full(_)) = action_tx.try_send(action)
            {
                app.show_toast("Busy — try again in a moment");
            }
        }
        app.tick_toast();

        let mode = app.cadence_mode();
        if mode != last_mode { let _ = cadence_tx.send(mode); last_mode = mode; }
    }
    Ok(())
}

async fn execute_action(api: &ApiClient, action: AppAction,
    outcome_tx: &mpsc::Sender<crate::tui::app::ActionOutcome>) {
    use crate::tui::app::ActionOutcome;
    match action {
        AppAction::Create(req) => match api.create_endpoint(&req).await {
            Ok(resp) => {
                let secret = resp.secret.unwrap_or_else(|| "(none returned)".into());
                let _ = outcome_tx.send(ActionOutcome::Created { secret }).await;
            }
            Err(e) => { let _ = outcome_tx.send(ActionOutcome::Error(e.to_string())).await; }
        },
        AppAction::Update(id, req) => match api.update_endpoint(&id, &req).await {
            Ok(_) => { let _ = outcome_tx.send(ActionOutcome::Toast("Webhook updated".into())).await; }
            Err(e) => { let _ = outcome_tx.send(ActionOutcome::Error(e.to_string())).await; }
        },
        AppAction::Delete(id) => match api.delete_endpoint(&id).await {
            Ok(_) => { let _ = outcome_tx.send(ActionOutcome::Toast("Webhook deleted".into())).await; }
            Err(e) => { let _ = outcome_tx.send(ActionOutcome::Error(e.to_string())).await; }
        },
        AppAction::OpenDetails(id) => match api.get_delivery_log(&id).await {
            Ok(d) => { let _ = outcome_tx.send(ActionOutcome::DeliveryDetail(Box::new(d))).await; }
            Err(e) => { let _ = outcome_tx.send(ActionOutcome::Error(e.to_string())).await; }
        },
        AppAction::None => {}
    }
}

fn install_panic_hook() {
    use std::sync::Once;
    static HOOK_INSTALLED: Once = Once::new();
    HOOK_INSTALLED.call_once(|| {
        let original = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let _ = disable_raw_mode();
            let _ = execute!(io::stdout(), LeaveAlternateScreen);
            original(info);
        }));
    });
}
