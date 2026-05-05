use crate::api::models::{WebhookDeliveryLogStatus, WebhookEndpointStatus};
use crate::domain::{DeliveryLog, Endpoint, EventTypeMeta};
use std::collections::HashSet;

#[derive(Clone, Debug, PartialEq)]
pub enum Screen { Endpoints, DeliveryLogs }

#[derive(Clone, Debug, PartialEq)]
pub enum ModalState {
    None,
    CreateWebhook,
    EditWebhook(usize),
    DeleteWebhook(usize),
    WebhookCreated(String), // signing secret
    DeliveryDetails(String), // delivery log id (for fetching detail on demand)
    ListenerConfig,         // configure local-listener URL + on/off
}

#[derive(Clone, Debug, PartialEq)]
pub enum ListenerField {
    Url,
    Enabled,
    Cancel,
    Save,
}

#[derive(Clone, Debug)]
pub struct ListenerForm {
    pub url: String,
    pub enabled: bool,
    pub active_field: ListenerField,
}

impl ListenerForm {
    pub fn from_app(url: Option<String>, enabled: bool) -> Self {
        Self {
            url: url.unwrap_or_default(),
            enabled,
            active_field: ListenerField::Url,
        }
    }
    pub fn next_field(&mut self) {
        self.active_field = match self.active_field {
            ListenerField::Url => ListenerField::Enabled,
            ListenerField::Enabled => ListenerField::Cancel,
            ListenerField::Cancel => ListenerField::Save,
            ListenerField::Save => ListenerField::Url,
        };
    }
    pub fn prev_field(&mut self) {
        self.active_field = match self.active_field {
            ListenerField::Url => ListenerField::Save,
            ListenerField::Enabled => ListenerField::Url,
            ListenerField::Cancel => ListenerField::Enabled,
            ListenerField::Save => ListenerField::Cancel,
        };
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum FormField {
    Url,
    Name,
    Status,
    CheckAll,
    UncheckAll,
    Event(usize),
    Cancel,
    Submit,
}

#[derive(Clone, Debug)]
pub struct FormState {
    pub url: String,
    pub name: String,
    pub events: Vec<bool>, // length matches App.event_types
    pub status: WebhookEndpointStatus,
    pub active_field: FormField,
    pub is_edit: bool,
    pub scroll: u16, // for the long event list inside the modal
}

impl FormState {
    pub fn new_create(num_events: usize) -> Self {
        Self {
            url: String::new(), name: String::new(),
            events: vec![true; num_events],
            status: WebhookEndpointStatus::Active,
            active_field: FormField::Url,
            is_edit: false, scroll: 0,
        }
    }

    pub fn new_edit(ep: &Endpoint, event_types: &[EventTypeMeta]) -> Self {
        let events: Vec<bool> = event_types.iter()
            .map(|et| ep.event_types.iter().any(|n| n == &et.name))
            .collect();
        Self {
            url: ep.endpoint_url.clone(),
            name: ep.name.clone(),
            events,
            status: ep.status,
            active_field: FormField::Url,
            is_edit: true, scroll: 0,
        }
    }

    pub fn next_field(&mut self, num_events: usize) {
        self.active_field = match &self.active_field {
            FormField::Url => FormField::Name,
            FormField::Name => if self.is_edit { FormField::Status } else { FormField::CheckAll },
            FormField::Status => FormField::CheckAll,
            FormField::CheckAll => FormField::UncheckAll,
            FormField::UncheckAll if num_events > 0 => FormField::Event(0),
            FormField::UncheckAll => FormField::Cancel,
            FormField::Event(i) if *i + 1 < num_events => FormField::Event(*i + 1),
            FormField::Event(_) => FormField::Cancel,
            FormField::Cancel => FormField::Submit,
            FormField::Submit => FormField::Url,
        };
        self.auto_scroll();
    }

    pub fn prev_field(&mut self, num_events: usize) {
        self.active_field = match &self.active_field {
            FormField::Url => FormField::Submit,
            FormField::Name => FormField::Url,
            FormField::Status => FormField::Name,
            FormField::CheckAll => if self.is_edit { FormField::Status } else { FormField::Name },
            FormField::UncheckAll => FormField::CheckAll,
            FormField::Event(0) => FormField::UncheckAll,
            FormField::Event(i) => FormField::Event(*i - 1),
            FormField::Cancel => if num_events > 0 { FormField::Event(num_events - 1) } else { FormField::UncheckAll },
            FormField::Submit => FormField::Cancel,
        };
        self.auto_scroll();
    }

    // Each event row produces a checkbox line + a description line.
    const LINES_PER_EVENT: u16 = 2;
    // Subtitle + URL field block + Name field block + (Status block if edit)
    // + Events header + blank lines.
    const PREAMBLE_LINES: u16 = 14;
    // Group headers + blank-between-groups across the whole event list — ≈ 7
    // groups * 2 lines is the typical case from the live API.
    const GROUP_OVERHEAD_LINES: u16 = 14;
    // Final blank + the button row.
    const FOOTER_LINES: u16 = 3;
    // How many lines of context to keep above the active event.
    const VISIBLE_OFFSET: u16 = 20;

    /// Best-effort estimate of the wrapped form's total line count. Used both
    /// to clamp scroll on PgDn and to position scroll for Cancel/Submit.
    pub fn total_form_lines(&self) -> u16 {
        Self::PREAMBLE_LINES
            .saturating_add((self.events.len() as u16).saturating_mul(Self::LINES_PER_EVENT))
            .saturating_add(Self::GROUP_OVERHEAD_LINES)
            .saturating_add(Self::FOOTER_LINES)
    }

    /// Maximum scroll value we should allow. Going past this leaves the
    /// viewport blank and (for very large values) underflows ratatui's
    /// Paragraph and panics.
    pub fn max_scroll(&self) -> u16 {
        // Leave at least 8 lines of content visible at the bottom so the user
        // can always see SOMETHING when they scroll all the way down.
        self.total_form_lines().saturating_sub(8)
    }

    /// Adjust `scroll` so the active field stays visible after Tab navigation.
    fn auto_scroll(&mut self) {
        let raw = match &self.active_field {
            FormField::Url | FormField::Name | FormField::Status
            | FormField::CheckAll | FormField::UncheckAll => 0,
            FormField::Event(i) => {
                let approx_line = (*i as u16)
                    .saturating_mul(Self::LINES_PER_EVENT)
                    .saturating_add(Self::PREAMBLE_LINES);
                approx_line.saturating_sub(Self::VISIBLE_OFFSET)
            }
            FormField::Cancel | FormField::Submit => self.max_scroll(),
        };
        self.scroll = raw.min(self.max_scroll());
    }
}

#[allow(dead_code)]
pub struct App {
    pub running: bool,
    pub screen: Screen,
    pub modal: ModalState,
    pub endpoints: Vec<Endpoint>,
    pub logs: Vec<DeliveryLog>,
    pub event_types: Vec<EventTypeMeta>,
    pub selected_endpoint: usize,
    pub selected_log: usize,
    pub form: FormState,
    pub detail_scroll: u16,
    pub last_error: Option<String>,
    pub poll_warning: Option<String>,

    pub filter_endpoint: usize, // 0=All, 1+=index+1
    pub filter_event: usize,
    pub filter_status: usize,   // 0=All, 1=Success, 2=Failure
    pub sort_ascending: bool,

    pub toast_message: Option<String>,
    pub toast_timer: u8,

    pub delivery_detail: Option<crate::api::models::DeliveryLogDetailDto>,

    // Local-listener config: forward incoming successful webhook deliveries
    // to a user-supplied URL. Session-only — not persisted to ~/.flute.
    pub listener_url: Option<String>,
    pub listener_enabled: bool,
    pub listener_form: ListenerForm,
    /// Log IDs we've already considered for forwarding. Initialised from the
    /// snapshot at the moment the listener is enabled so we don't blast the
    /// listener with the entire backlog.
    pub seen_log_ids: HashSet<String>,
}

impl App {
    pub fn new(poll_warning: Option<String>) -> Self {
        Self {
            running: true,
            screen: Screen::Endpoints,
            modal: ModalState::None,
            endpoints: Vec::new(),
            logs: Vec::new(),
            event_types: Vec::new(),
            selected_endpoint: 0,
            selected_log: 0,
            form: FormState::new_create(0),
            detail_scroll: 0,
            last_error: None,
            poll_warning,
            filter_endpoint: 0,
            filter_event: 0,
            filter_status: 0,
            sort_ascending: false,
            toast_message: None,
            toast_timer: 0,
            delivery_detail: None,
            listener_url: None,
            listener_enabled: false,
            listener_form: ListenerForm::from_app(None, false),
            seen_log_ids: HashSet::new(),
        }
    }

    pub fn show_toast(&mut self, msg: impl Into<String>) {
        self.toast_message = Some(msg.into());
        self.toast_timer = 20;
    }

    pub fn tick_toast(&mut self) {
        if self.toast_timer > 0 {
            self.toast_timer -= 1;
            if self.toast_timer == 0 { self.toast_message = None; }
        }
    }

    pub fn clear_last_error(&mut self) { self.last_error = None; }

    pub fn set_delivery_detail(&mut self, d: crate::api::models::DeliveryLogDetailDto) {
        self.delivery_detail = Some(d);
    }

    pub fn clear_delivery_detail(&mut self) { self.delivery_detail = None; }

    /// Apply a fresh poller snapshot to the app state. Returns any
    /// auto-forward actions that should be sent to the action executor —
    /// produced when the listener is enabled and a previously-unseen
    /// successful delivery log appears in the snapshot.
    pub fn apply_snapshot(
        &mut self,
        endpoints: Vec<Endpoint>,
        logs: Vec<DeliveryLog>,
        event_types: Vec<EventTypeMeta>,
    ) -> Vec<AppAction> {
        let mut forwards = Vec::new();
        if self.listener_enabled
            && let Some(url) = self.listener_url.clone()
        {
            for log in &logs {
                if log.status == WebhookDeliveryLogStatus::Success
                    && !self.seen_log_ids.contains(&log.id)
                {
                    forwards.push(AppAction::ForwardLog {
                        log_id: log.id.clone(),
                        url: url.clone(),
                    });
                }
            }
        }
        // Always remember every log id we've seen (even when the listener is
        // off) so toggling it on later starts forwarding from the next snapshot,
        // not from the entire historical backlog.
        for log in &logs {
            self.seen_log_ids.insert(log.id.clone());
        }

        self.endpoints = endpoints;
        self.logs = logs;
        if self.event_types.is_empty() && !event_types.is_empty() {
            self.event_types = event_types;
        }
        if self.selected_endpoint >= self.endpoints.len() && !self.endpoints.is_empty() {
            self.selected_endpoint = self.endpoints.len() - 1;
        }
        forwards
    }

    /// Persist the listener-config form to the live App fields. When enabling
    /// the listener for the first time, prime `seen_log_ids` with the current
    /// log ids so the listener won't replay history — only future arrivals.
    pub fn save_listener_config(&mut self) {
        let url = self.listener_form.url.trim().to_string();
        let enabled = self.listener_form.enabled && !url.is_empty();
        self.listener_url = if url.is_empty() { None } else { Some(url) };
        let was_enabled = self.listener_enabled;
        self.listener_enabled = enabled;
        if enabled && !was_enabled {
            self.seen_log_ids = self.logs.iter().map(|l| l.id.clone()).collect();
        }
        self.modal = ModalState::None;
        self.show_toast(if enabled { "Listener enabled" } else { "Listener disabled" });
    }

    pub fn cadence_mode(&self) -> crate::poller::CadenceMode {
        match self.modal {
            ModalState::CreateWebhook | ModalState::EditWebhook(_) => crate::poller::CadenceMode::Backoff,
            _ => crate::poller::CadenceMode::Active,
        }
    }
}

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

#[derive(Debug)]
pub enum AppAction {
    None,
    Create(crate::api::models::CreateWebhookEndpointRequest),
    Update(String, crate::api::models::UpdateWebhookEndpointRequest),
    Delete(String),
    OpenDetails(String),
    /// Forward this delivery log's headers + payload to a target URL.
    ForwardLog { log_id: String, url: String },
}

impl App {
    pub fn handle_key(&mut self, key: KeyEvent) -> AppAction {
        if key.kind != KeyEventKind::Press { return AppAction::None; }
        // Ctrl-C always quits
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.running = false;
            return AppAction::None;
        }
        match self.modal.clone() {
            ModalState::None => self.handle_main_key(key),
            ModalState::CreateWebhook | ModalState::EditWebhook(_) => self.handle_form_key(key),
            ModalState::DeleteWebhook(idx) => self.handle_delete_key(key, idx),
            ModalState::WebhookCreated(_) => self.handle_created_key(key),
            ModalState::DeliveryDetails(_) => self.handle_details_key(key),
            ModalState::ListenerConfig => self.handle_listener_key(key),
        }
    }

    fn handle_main_key(&mut self, key: KeyEvent) -> AppAction {
        // Error modal (rendered when modal=None and last_error=Some) absorbs every key
        // except Enter/Esc (dismiss). q does NOT quit while an error is showing — same
        // contract the create/edit form uses for its text-input fields.
        if self.last_error.is_some() {
            if matches!(key.code, KeyCode::Esc | KeyCode::Enter) {
                self.clear_last_error();
            }
            return AppAction::None;
        }
        match key.code {
            KeyCode::Char('q') => { self.running = false; AppAction::None }
            KeyCode::Tab | KeyCode::BackTab => {
                self.screen = match self.screen {
                    Screen::Endpoints => Screen::DeliveryLogs,
                    Screen::DeliveryLogs => Screen::Endpoints,
                };
                AppAction::None
            }
            _ => match self.screen {
                Screen::Endpoints => self.handle_endpoints_key(key),
                Screen::DeliveryLogs => self.handle_logs_key(key),
            }
        }
    }

    fn handle_endpoints_key(&mut self, key: KeyEvent) -> AppAction {
        let n = self.endpoints.len();
        match key.code {
            KeyCode::Up | KeyCode::Char('k') if n > 0 && self.selected_endpoint > 0 => {
                self.selected_endpoint -= 1; AppAction::None
            }
            KeyCode::Down | KeyCode::Char('j') if n > 0 && self.selected_endpoint + 1 < n => {
                self.selected_endpoint += 1; AppAction::None
            }
            KeyCode::Char('c') => {
                self.form = FormState::new_create(self.event_types.len());
                self.modal = ModalState::CreateWebhook;
                AppAction::None
            }
            KeyCode::Char('e') | KeyCode::Enter if n > 0 => {
                self.form = FormState::new_edit(&self.endpoints[self.selected_endpoint], &self.event_types);
                self.modal = ModalState::EditWebhook(self.selected_endpoint);
                AppAction::None
            }
            KeyCode::Char('d') if n > 0 => {
                self.modal = ModalState::DeleteWebhook(self.selected_endpoint);
                AppAction::None
            }
            _ => AppAction::None,
        }
    }

    fn handle_logs_key(&mut self, key: KeyEvent) -> AppAction {
        // How many rows PgUp/PgDn skip per press.
        const PAGE_STEP: usize = 10;
        let filtered = self.filtered_log_indices();
        let n = filtered.len();
        match key.code {
            KeyCode::Up | KeyCode::Char('k') if n > 0 && self.selected_log > 0 => {
                self.selected_log -= 1; AppAction::None
            }
            KeyCode::Down | KeyCode::Char('j') if n > 0 && self.selected_log + 1 < n => {
                self.selected_log += 1; AppAction::None
            }
            KeyCode::PageDown if n > 0 => {
                self.selected_log = (self.selected_log + PAGE_STEP).min(n - 1);
                AppAction::None
            }
            KeyCode::PageUp if n > 0 => {
                self.selected_log = self.selected_log.saturating_sub(PAGE_STEP);
                AppAction::None
            }
            KeyCode::Home if n > 0 => { self.selected_log = 0; AppAction::None }
            KeyCode::End if n > 0 => { self.selected_log = n - 1; AppAction::None }
            KeyCode::Enter | KeyCode::Char('v') if n > 0 => {
                let id = self.logs[filtered[self.selected_log]].id.clone();
                self.detail_scroll = 0;
                self.modal = ModalState::DeliveryDetails(id.clone());
                AppAction::OpenDetails(id)
            }
            KeyCode::Char('1') => {
                self.filter_endpoint = (self.filter_endpoint + 1) % (self.endpoints.len() + 1);
                self.selected_log = 0; AppAction::None
            }
            KeyCode::Char('2') => {
                self.filter_event = (self.filter_event + 1) % (self.event_types.len() + 1);
                self.selected_log = 0; AppAction::None
            }
            KeyCode::Char('3') => {
                self.filter_status = (self.filter_status + 1) % 3;
                self.selected_log = 0; AppAction::None
            }
            KeyCode::Char('s') => { self.sort_ascending = !self.sort_ascending; AppAction::None }
            KeyCode::Char('x') => {
                self.filter_endpoint = 0; self.filter_event = 0; self.filter_status = 0;
                self.selected_log = 0; AppAction::None
            }
            // Open the listener-config modal: enable/disable + set URL.
            KeyCode::Char('l') => {
                self.listener_form = ListenerForm::from_app(self.listener_url.clone(), self.listener_enabled);
                self.modal = ModalState::ListenerConfig;
                AppAction::None
            }
            // Trigger one-shot forward of the selected log to the configured
            // listener URL. Only meaningful for successful deliveries (the
            // failed ones don't have a payload that's worth replaying).
            KeyCode::Char('t') if n > 0 => {
                let (log_id, log_status) = {
                    let log = &self.logs[filtered[self.selected_log]];
                    (log.id.clone(), log.status)
                };
                if log_status != WebhookDeliveryLogStatus::Success {
                    self.show_toast("Trigger only works on successful deliveries");
                    return AppAction::None;
                }
                let Some(url) = self.listener_url.clone() else {
                    self.show_toast("No listener URL configured (press [l] to set one)");
                    return AppAction::None;
                };
                let prefix = log_id.get(..8.min(log_id.len())).unwrap_or("").to_string();
                self.show_toast(format!("Forwarding {prefix} -> {url}"));
                AppAction::ForwardLog { log_id, url }
            }
            _ => AppAction::None,
        }
    }

    fn handle_listener_key(&mut self, key: KeyEvent) -> AppAction {
        match key.code {
            KeyCode::Esc => { self.modal = ModalState::None; return AppAction::None; }
            KeyCode::Tab | KeyCode::Down => {
                self.listener_form.next_field();
                return AppAction::None;
            }
            KeyCode::BackTab | KeyCode::Up => {
                self.listener_form.prev_field();
                return AppAction::None;
            }
            KeyCode::Enter => {
                match self.listener_form.active_field {
                    ListenerField::Cancel => self.modal = ModalState::None,
                    ListenerField::Save => self.save_listener_config(),
                    ListenerField::Enabled => self.listener_form.enabled = !self.listener_form.enabled,
                    ListenerField::Url => self.listener_form.next_field(),
                }
                return AppAction::None;
            }
            KeyCode::Char(' ') => {
                if let ListenerField::Enabled = self.listener_form.active_field {
                    self.listener_form.enabled = !self.listener_form.enabled;
                    return AppAction::None;
                }
                if let ListenerField::Url = self.listener_form.active_field {
                    self.listener_form.url.push(' ');
                    return AppAction::None;
                }
            }
            KeyCode::Backspace => {
                if let ListenerField::Url = self.listener_form.active_field {
                    self.listener_form.url.pop();
                    return AppAction::None;
                }
            }
            KeyCode::Char(c) => {
                if let ListenerField::Url = self.listener_form.active_field {
                    self.listener_form.url.push(c);
                    return AppAction::None;
                }
            }
            _ => {}
        }
        AppAction::None
    }

    pub fn filtered_log_indices(&self) -> Vec<usize> {
        let mut out: Vec<usize> = (0..self.logs.len()).filter(|&i| {
            let log = &self.logs[i];
            if self.filter_endpoint > 0 {
                let ep_idx = self.filter_endpoint - 1;
                if ep_idx >= self.endpoints.len() || self.endpoints[ep_idx].id != log.endpoint_id {
                    return false;
                }
            }
            if self.filter_event > 0 {
                let evt_idx = self.filter_event - 1;
                if evt_idx >= self.event_types.len() || self.event_types[evt_idx].name != log.event_type {
                    return false;
                }
            }
            match self.filter_status {
                1 => log.status == WebhookDeliveryLogStatus::Success,
                2 => log.status == WebhookDeliveryLogStatus::Failure,
                _ => true,
            }
        }).collect();
        if self.sort_ascending { out.reverse(); }
        out
    }

    fn handle_form_key(&mut self, key: KeyEvent) -> AppAction {
        let n = self.event_types.len();
        match key.code {
            KeyCode::Esc => { self.modal = ModalState::None; return AppAction::None; }
            KeyCode::Tab | KeyCode::Down => { self.form.next_field(n); return AppAction::None; }
            KeyCode::BackTab | KeyCode::Up => { self.form.prev_field(n); return AppAction::None; }
            // Left/Right swap between Cancel and Submit (a horizontal button
            // group). Ignored elsewhere so they don't accidentally jump out of
            // text-input fields.
            KeyCode::Left | KeyCode::Right => {
                self.form.active_field = match &self.form.active_field {
                    FormField::Cancel => FormField::Submit,
                    FormField::Submit => FormField::Cancel,
                    other => other.clone(),
                };
                return AppAction::None;
            }
            KeyCode::PageDown => {
                let max = self.form.max_scroll();
                self.form.scroll = self.form.scroll.saturating_add(15).min(max);
                return AppAction::None;
            }
            KeyCode::PageUp => {
                self.form.scroll = self.form.scroll.saturating_sub(15);
                return AppAction::None;
            }
            KeyCode::Enter => return self.activate_form_field(),
            KeyCode::Backspace => match self.form.active_field {
                FormField::Url => { self.form.url.pop(); return AppAction::None; }
                FormField::Name => { self.form.name.pop(); return AppAction::None; }
                _ => return AppAction::None,
            },
            KeyCode::Char(' ') => match self.form.active_field {
                FormField::Url => self.form.url.push(' '),
                FormField::Name => self.form.name.push(' '),
                _ => return self.activate_form_field(),
            },
            KeyCode::Char(c) => match self.form.active_field {
                FormField::Url => self.form.url.push(c),
                FormField::Name => self.form.name.push(c),
                _ => {}
            },
            _ => {}
        }
        AppAction::None
    }

    fn activate_form_field(&mut self) -> AppAction {
        match self.form.active_field.clone() {
            FormField::Cancel => { self.modal = ModalState::None; AppAction::None }
            FormField::Submit => self.submit_form(),
            FormField::CheckAll => { self.form.events.iter_mut().for_each(|e| *e = true); AppAction::None }
            FormField::UncheckAll => { self.form.events.iter_mut().for_each(|e| *e = false); AppAction::None }
            FormField::Event(i) => {
                if let Some(slot) = self.form.events.get_mut(i) { *slot = !*slot; }
                AppAction::None
            }
            FormField::Status => {
                self.form.status = match self.form.status {
                    WebhookEndpointStatus::Active => WebhookEndpointStatus::Inactive,
                    WebhookEndpointStatus::Inactive => WebhookEndpointStatus::Active,
                };
                AppAction::None
            }
            _ => AppAction::None,
        }
    }

    fn submit_form(&mut self) -> AppAction {
        let selected: Vec<String> = self.event_types.iter().enumerate()
            .filter(|(i, _)| self.form.events.get(*i).copied().unwrap_or(false))
            .map(|(_, et)| et.name.clone())
            .collect();
        if self.form.url.trim().is_empty() {
            self.show_toast("Endpoint URL is required");
            return AppAction::None;
        }
        if selected.is_empty() {
            self.show_toast("Select at least one event type");
            return AppAction::None;
        }
        let name = if self.form.name.trim().is_empty() {
            "Untitled Webhook".to_string()
        } else {
            self.form.name.clone()
        };
        match self.modal.clone() {
            ModalState::CreateWebhook => {
                AppAction::Create(crate::api::models::CreateWebhookEndpointRequest {
                    name, endpoint_url: self.form.url.clone(), event_types: selected,
                })
            }
            ModalState::EditWebhook(idx) => {
                let Some(ep) = self.endpoints.get(idx) else {
                    self.modal = ModalState::None;
                    self.show_toast("Endpoint was removed; edit cancelled");
                    return AppAction::None;
                };
                let id = ep.id.clone();
                AppAction::Update(id, crate::api::models::UpdateWebhookEndpointRequest {
                    name, endpoint_url: self.form.url.clone(),
                    status: self.form.status, event_types: selected,
                })
            }
            _ => AppAction::None,
        }
    }

    fn handle_delete_key(&mut self, key: KeyEvent, idx: usize) -> AppAction {
        match key.code {
            KeyCode::Esc | KeyCode::Char('n') => { self.modal = ModalState::None; AppAction::None }
            KeyCode::Enter | KeyCode::Char('y') => {
                if let Some(ep) = self.endpoints.get(idx) {
                    let id = ep.id.clone();
                    self.modal = ModalState::None;
                    return AppAction::Delete(id);
                }
                AppAction::None
            }
            _ => AppAction::None,
        }
    }

    fn handle_created_key(&mut self, key: KeyEvent) -> AppAction {
        if matches!(key.code, KeyCode::Enter | KeyCode::Esc) {
            self.modal = ModalState::None;
        }
        AppAction::None
    }

    fn handle_details_key(&mut self, key: KeyEvent) -> AppAction {
        match key.code {
            KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q') => {
                self.modal = ModalState::None;
                self.clear_delivery_detail();
            }
            KeyCode::Down | KeyCode::Char('j') => self.detail_scroll = self.detail_scroll.saturating_add(2),
            KeyCode::Up | KeyCode::Char('k') => self.detail_scroll = self.detail_scroll.saturating_sub(2),
            KeyCode::PageDown => self.detail_scroll = self.detail_scroll.saturating_add(10),
            KeyCode::PageUp => self.detail_scroll = self.detail_scroll.saturating_sub(10),
            _ => {}
        }
        AppAction::None
    }
}

#[derive(Debug)]
pub enum ActionOutcome {
    Toast(String),
    Created { secret: String },
    Error(String),
    // Boxed so this variant doesn't bloat the enum's stack size — the DTO is
    // ~360 bytes vs the other variants at ~24 bytes, which clippy correctly
    // flags as `large_enum_variant`.
    DeliveryDetail(Box<crate::api::models::DeliveryLogDetailDto>),
}

impl App {
    pub fn apply_outcome(&mut self, outcome: ActionOutcome) {
        match outcome {
            ActionOutcome::Toast(msg) => self.show_toast(msg),
            ActionOutcome::Created { secret } => {
                self.modal = ModalState::WebhookCreated(secret);
            }
            ActionOutcome::Error(msg) => {
                // Stick the error in the persistent banner so the user has time to read it
                // (and see the correlation id). Esc on the main screen dismisses it.
                self.last_error = Some(msg);
            }
            ActionOutcome::DeliveryDetail(d) => self.set_delivery_detail(*d),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};

    fn key(c: char) -> KeyEvent {
        KeyEvent { code: KeyCode::Char(c), modifiers: KeyModifiers::NONE, kind: KeyEventKind::Press, state: KeyEventState::empty() }
    }

    #[test]
    fn q_at_top_level_quits() {
        let mut app = App::new(None);
        app.handle_key(key('q'));
        assert!(!app.running);
    }

    #[test]
    fn tab_switches_screens() {
        let mut app = App::new(None);
        let kp = KeyEvent { code: KeyCode::Tab, modifiers: KeyModifiers::NONE, kind: KeyEventKind::Press, state: KeyEventState::empty() };
        app.handle_key(kp);
        assert_eq!(app.screen, Screen::DeliveryLogs);
        app.handle_key(kp);
        assert_eq!(app.screen, Screen::Endpoints);
    }

    #[test]
    fn ctrl_c_always_quits_even_inside_form() {
        let mut app = App::new(None);
        app.modal = ModalState::CreateWebhook;
        app.form = FormState::new_create(0);
        let kp = KeyEvent { code: KeyCode::Char('c'), modifiers: KeyModifiers::CONTROL, kind: KeyEventKind::Press, state: KeyEventState::empty() };
        app.handle_key(kp);
        assert!(!app.running);
    }

    #[test]
    fn typing_q_in_url_field_appends_q_and_does_not_quit() {
        let mut app = App::new(None);
        app.modal = ModalState::CreateWebhook;
        app.form = FormState::new_create(0);
        // FormField::Url is the default starting field
        app.handle_key(key('q'));
        app.handle_key(key('a'));
        assert_eq!(app.form.url, "qa");
        assert!(app.running, "q in URL field must not quit the app");
    }

    #[test]
    fn typing_q_c_d_e_in_name_field_are_all_literal() {
        let mut app = App::new(None);
        app.modal = ModalState::CreateWebhook;
        app.form = FormState::new_create(0);
        app.form.active_field = FormField::Name;
        for c in ['q', 'c', 'd', 'e'] { app.handle_key(key(c)); }
        assert_eq!(app.form.name, "qcde");
        assert!(app.running);
    }

    #[test]
    fn esc_in_form_closes_modal() {
        let mut app = App::new(None);
        app.modal = ModalState::CreateWebhook;
        app.form = FormState::new_create(0);
        let kp = KeyEvent { code: KeyCode::Esc, modifiers: KeyModifiers::NONE, kind: KeyEventKind::Press, state: crossterm::event::KeyEventState::empty() };
        app.handle_key(kp);
        assert_eq!(app.modal, ModalState::None);
    }

    #[test]
    fn pgdown_in_form_scrolls_event_list() {
        let mut app = App::new(None);
        app.modal = ModalState::CreateWebhook;
        app.form = FormState::new_create(35);
        let kp = KeyEvent { code: KeyCode::PageDown, modifiers: KeyModifiers::NONE, kind: KeyEventKind::Press, state: crossterm::event::KeyEventState::empty() };
        app.handle_key(kp);
        app.handle_key(kp);
        // Manual PgDn step is 15 — two presses gives 30.
        assert_eq!(app.form.scroll, 30);
    }

    #[test]
    fn tabbing_to_submit_does_not_crash_with_huge_scroll() {
        // Reproduces the "down arrow at the bottom quits the app" report:
        // auto_scroll used to set scroll = u16::MAX for Cancel/Submit, which
        // caused ratatui's Paragraph to underflow and panic at render time.
        // The new bounded value must stay well within the form's actual height.
        let mut app = App::new(None);
        app.modal = ModalState::CreateWebhook;
        app.event_types = (0..35)
            .map(|i| crate::domain::EventTypeMeta {
                name: format!("event.{i}"),
                description: format!("desc {i}"),
                group: "Test".into(),
            })
            .collect();
        app.form = FormState::new_create(app.event_types.len());

        app.form.active_field = FormField::Submit;
        app.form.auto_scroll();
        assert!(
            app.form.scroll < 1000,
            "scroll must stay bounded for buttons (was {}) — large values panic ratatui",
            app.form.scroll
        );
        assert!(app.form.scroll > 50, "scroll must still reach the bottom of the form");
    }

    #[test]
    fn tab_navigation_auto_scrolls_to_distant_events() {
        // Reproduces the reported bug: tabbing to events near the bottom of
        // the list left the highlight off-screen because scroll wasn't following.
        let mut app = App::new(None);
        app.modal = ModalState::CreateWebhook;
        // Pretend we have 35 event types loaded.
        app.event_types = (0..35)
            .map(|i| crate::domain::EventTypeMeta {
                name: format!("event.{i}"),
                description: format!("desc {i}"),
                group: "Test".into(),
            })
            .collect();
        app.form = FormState::new_create(app.event_types.len());

        // Top of the form: scroll stays at 0.
        assert_eq!(app.form.scroll, 0);

        // Jump active_field to a far-down event (skipping intermediate Tabs).
        app.form.active_field = FormField::Event(34);
        app.form.auto_scroll();
        assert!(
            app.form.scroll > 50,
            "scroll must follow active event 34, got {}",
            app.form.scroll
        );

        // Returning to a top field resets the viewport.
        app.form.active_field = FormField::Url;
        app.form.auto_scroll();
        assert_eq!(app.form.scroll, 0);
    }

    #[test]
    fn pgdown_clamps_to_max_scroll_so_you_dont_scroll_past_the_form() {
        let mut app = App::new(None);
        app.modal = ModalState::CreateWebhook;
        app.form = FormState::new_create(35);
        let max = app.form.max_scroll();
        let pg = KeyEvent { code: KeyCode::PageDown, modifiers: KeyModifiers::NONE, kind: KeyEventKind::Press, state: crossterm::event::KeyEventState::empty() };
        // Mash PgDn far more times than the form is long.
        for _ in 0..200 { app.handle_key(pg); }
        assert_eq!(app.form.scroll, max,
            "scroll must clamp at max_scroll ({max}), got {}", app.form.scroll);
    }

    #[test]
    fn pgdn_pgup_home_end_navigate_logs() {
        let mut app = App::new(None);
        app.screen = Screen::DeliveryLogs;
        // Seed with 50 fake logs so we have enough to page through.
        app.logs = (0..50)
            .map(|i| crate::domain::DeliveryLog {
                id: format!("{i}"),
                endpoint_id: "ep".into(),
                endpoint_name: "ep".into(),
                endpoint_url: "https://x".into(),
                event_id: format!("evt-{i}"),
                event_type: "transaction.card.captured".into(),
                status: crate::api::models::WebhookDeliveryLogStatus::Success,
                attempt_number: 1,
                response_status_code: Some(200),
                duration_ms: 1,
                error_message: None,
                created_on: chrono::Utc::now(),
            })
            .collect();

        let kp = |code| KeyEvent { code, modifiers: KeyModifiers::NONE, kind: KeyEventKind::Press, state: crossterm::event::KeyEventState::empty() };

        // PgDn jumps 10 rows.
        app.handle_key(kp(KeyCode::PageDown));
        assert_eq!(app.selected_log, 10);
        app.handle_key(kp(KeyCode::PageDown));
        assert_eq!(app.selected_log, 20);

        // PgUp jumps back 10.
        app.handle_key(kp(KeyCode::PageUp));
        assert_eq!(app.selected_log, 10);

        // End jumps to the last row.
        app.handle_key(kp(KeyCode::End));
        assert_eq!(app.selected_log, 49);

        // PgDn at the bottom is a no-op (clamped at n-1).
        app.handle_key(kp(KeyCode::PageDown));
        assert_eq!(app.selected_log, 49);

        // Home jumps back to row 0.
        app.handle_key(kp(KeyCode::Home));
        assert_eq!(app.selected_log, 0);

        // PgUp at the top is a no-op (saturating_sub).
        app.handle_key(kp(KeyCode::PageUp));
        assert_eq!(app.selected_log, 0);
    }

    #[test]
    fn left_right_swap_cancel_and_submit() {
        let mut app = App::new(None);
        app.modal = ModalState::CreateWebhook;
        app.form = FormState::new_create(0);
        app.form.active_field = FormField::Cancel;

        let right = KeyEvent { code: KeyCode::Right, modifiers: KeyModifiers::NONE, kind: KeyEventKind::Press, state: crossterm::event::KeyEventState::empty() };
        let left = KeyEvent { code: KeyCode::Left, modifiers: KeyModifiers::NONE, kind: KeyEventKind::Press, state: crossterm::event::KeyEventState::empty() };

        app.handle_key(right);
        assert_eq!(app.form.active_field, FormField::Submit);
        app.handle_key(left);
        assert_eq!(app.form.active_field, FormField::Cancel);
        // Pressing both directions toggles, so a second press of the same
        // key flips back.
        app.handle_key(right);
        app.handle_key(right);
        assert_eq!(app.form.active_field, FormField::Cancel);
    }

    #[test]
    fn left_right_does_not_jump_out_of_text_field() {
        // Arrow keys on URL/Name must NOT move focus — that would feel like
        // the cursor escaped while typing.
        let mut app = App::new(None);
        app.modal = ModalState::CreateWebhook;
        app.form = FormState::new_create(0);
        app.form.active_field = FormField::Url;
        let right = KeyEvent { code: KeyCode::Right, modifiers: KeyModifiers::NONE, kind: KeyEventKind::Press, state: crossterm::event::KeyEventState::empty() };
        app.handle_key(right);
        assert_eq!(app.form.active_field, FormField::Url);
    }

    #[test]
    fn submit_field_lands_within_visible_form() {
        // Cancel/Submit must end up with scroll <= max_scroll so the buttons
        // are actually visible (previously raw scroll could exceed total form
        // height, leaving the viewport empty).
        let mut app = App::new(None);
        app.modal = ModalState::CreateWebhook;
        app.form = FormState::new_create(35);
        app.form.active_field = FormField::Submit;
        app.form.auto_scroll();
        assert!(app.form.scroll <= app.form.max_scroll(),
            "scroll={} must be <= max_scroll={}",
            app.form.scroll, app.form.max_scroll());
        assert_eq!(app.form.scroll, app.form.max_scroll(),
            "Submit should sit at max_scroll so the buttons are flush at the bottom");
    }

    #[test]
    fn cadence_mode_is_backoff_when_form_open() {
        let mut app = App::new(None);
        app.modal = ModalState::CreateWebhook;
        assert_eq!(app.cadence_mode(), crate::poller::CadenceMode::Backoff);
        app.modal = ModalState::None;
        assert_eq!(app.cadence_mode(), crate::poller::CadenceMode::Active);
    }

    fn fake_log(id: &str, status: WebhookDeliveryLogStatus) -> crate::domain::DeliveryLog {
        crate::domain::DeliveryLog {
            id: id.into(),
            endpoint_id: "ep-1".into(),
            endpoint_name: "ep".into(),
            endpoint_url: "https://x".into(),
            event_id: format!("evt-{id}"),
            event_type: "transaction.card.captured".into(),
            status,
            attempt_number: 1,
            response_status_code: Some(200),
            duration_ms: 1,
            error_message: None,
            created_on: chrono::Utc::now(),
        }
    }

    #[test]
    fn apply_snapshot_emits_forward_for_new_successful_logs_when_listener_enabled() {
        let mut app = App::new(None);
        app.listener_url = Some("http://127.0.0.1:3000/webhook".into());
        app.listener_enabled = true;
        // Pretend logs "old-1" was already seen — no forward expected for it.
        app.seen_log_ids.insert("old-1".into());

        let new_logs = vec![
            fake_log("old-1", WebhookDeliveryLogStatus::Success),  // already seen
            fake_log("new-success", WebhookDeliveryLogStatus::Success),
            fake_log("new-failed", WebhookDeliveryLogStatus::Failure),
        ];
        let actions = app.apply_snapshot(vec![], new_logs, vec![]);

        // Exactly one forward — for the new successful log.
        assert_eq!(actions.len(), 1);
        match &actions[0] {
            AppAction::ForwardLog { log_id, url } => {
                assert_eq!(log_id, "new-success");
                assert_eq!(url, "http://127.0.0.1:3000/webhook");
            }
            other => panic!("unexpected action: {other:?}"),
        }
        // All three IDs are now marked seen so a re-poll won't re-forward.
        assert!(app.seen_log_ids.contains("old-1"));
        assert!(app.seen_log_ids.contains("new-success"));
        assert!(app.seen_log_ids.contains("new-failed"));
    }

    #[test]
    fn apply_snapshot_emits_no_forwards_when_listener_disabled() {
        let mut app = App::new(None);
        app.listener_url = Some("http://x".into());
        app.listener_enabled = false; // off
        let logs = vec![fake_log("a", WebhookDeliveryLogStatus::Success)];
        let actions = app.apply_snapshot(vec![], logs, vec![]);
        assert!(actions.is_empty());
    }

    #[test]
    fn save_listener_config_primes_seen_ids_so_history_is_not_replayed() {
        let mut app = App::new(None);
        // Three pre-existing logs sitting in the snapshot.
        app.logs = vec![
            fake_log("a", WebhookDeliveryLogStatus::Success),
            fake_log("b", WebhookDeliveryLogStatus::Success),
            fake_log("c", WebhookDeliveryLogStatus::Success),
        ];
        app.listener_form.url = "http://127.0.0.1:3000/webhook".into();
        app.listener_form.enabled = true;
        app.save_listener_config();

        // All historical IDs are now in seen_log_ids — a poll containing the
        // same three logs should produce zero forwards.
        let actions = app.apply_snapshot(vec![], app.logs.clone(), vec![]);
        assert!(actions.is_empty(), "history must not be replayed on enable");

        // A genuinely new log AFTER enabling must still be forwarded.
        let mut next_logs = app.logs.clone();
        next_logs.push(fake_log("d", WebhookDeliveryLogStatus::Success));
        let actions = app.apply_snapshot(vec![], next_logs, vec![]);
        assert_eq!(actions.len(), 1);
    }

    #[test]
    fn t_on_successful_log_emits_forward_when_listener_url_set() {
        let mut app = App::new(None);
        app.screen = Screen::DeliveryLogs;
        app.logs = vec![fake_log("the-log", WebhookDeliveryLogStatus::Success)];
        app.listener_url = Some("http://127.0.0.1:9000/hook".into());
        let kp = KeyEvent { code: KeyCode::Char('t'), modifiers: KeyModifiers::NONE, kind: KeyEventKind::Press, state: crossterm::event::KeyEventState::empty() };
        match app.handle_key(kp) {
            AppAction::ForwardLog { log_id, url } => {
                assert_eq!(log_id, "the-log");
                assert_eq!(url, "http://127.0.0.1:9000/hook");
            }
            other => panic!("expected ForwardLog, got {other:?}"),
        }
    }

    #[test]
    fn t_without_listener_url_toasts_instead_of_forwarding() {
        let mut app = App::new(None);
        app.screen = Screen::DeliveryLogs;
        app.logs = vec![fake_log("the-log", WebhookDeliveryLogStatus::Success)];
        // listener_url is None
        let kp = KeyEvent { code: KeyCode::Char('t'), modifiers: KeyModifiers::NONE, kind: KeyEventKind::Press, state: crossterm::event::KeyEventState::empty() };
        let action = app.handle_key(kp);
        assert!(matches!(action, AppAction::None));
        assert!(app.toast_message.is_some());
    }

    #[test]
    fn l_opens_listener_modal_from_logs_screen() {
        let mut app = App::new(None);
        app.screen = Screen::DeliveryLogs;
        let kp = KeyEvent { code: KeyCode::Char('l'), modifiers: KeyModifiers::NONE, kind: KeyEventKind::Press, state: crossterm::event::KeyEventState::empty() };
        app.handle_key(kp);
        assert_eq!(app.modal, ModalState::ListenerConfig);
    }

    #[test]
    fn esc_on_main_screen_dismisses_last_error() {
        let mut app = App::new(None);
        app.last_error = Some("Poll error: 401".into());
        let kp = KeyEvent { code: KeyCode::Esc, modifiers: KeyModifiers::NONE, kind: KeyEventKind::Press, state: crossterm::event::KeyEventState::empty() };
        app.handle_key(kp);
        assert!(app.last_error.is_none());
        assert!(app.running, "dismissing error must not quit the app");
    }

    #[test]
    fn enter_dismisses_error_modal() {
        let mut app = App::new(None);
        app.last_error = Some("API 500".into());
        let kp = KeyEvent { code: KeyCode::Enter, modifiers: KeyModifiers::NONE, kind: KeyEventKind::Press, state: crossterm::event::KeyEventState::empty() };
        app.handle_key(kp);
        assert!(app.last_error.is_none());
    }

    #[test]
    fn error_modal_absorbs_q_and_other_keys() {
        let mut app = App::new(None);
        app.last_error = Some("API 500".into());
        // q should NOT quit while the error modal is up — it gets swallowed.
        app.handle_key(key('q'));
        assert!(app.running, "q must not quit while error modal is showing");
        assert!(app.last_error.is_some(), "q must not dismiss the error");
        // c/d/e are also absorbed (no spurious modal opens).
        for c in ['c', 'd', 'e', 's', 'x', '1', '2', '3'] {
            app.handle_key(key(c));
        }
        assert_eq!(app.modal, ModalState::None);
        assert!(app.last_error.is_some());
    }

    #[test]
    fn error_outcome_sets_last_error_persistently() {
        let mut app = App::new(None);
        app.apply_outcome(ActionOutcome::Error("API 500 (correlation_id=abc): boom".into()));
        assert_eq!(app.last_error.as_deref(), Some("API 500 (correlation_id=abc): boom"));
        // Toast should NOT be set — errors are sticky in the banner, not transient.
        assert!(app.toast_message.is_none());
    }
}
