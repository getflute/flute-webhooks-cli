use crate::api::models::{WebhookDeliveryLogStatus, WebhookEndpointStatus};
use crate::domain::{DeliveryLog, Endpoint, EventTypeMeta};

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

    pub fn set_delivery_detail(&mut self, d: crate::api::models::DeliveryLogDetailDto) {
        self.delivery_detail = Some(d);
    }

    pub fn clear_delivery_detail(&mut self) { self.delivery_detail = None; }

    pub fn apply_snapshot(&mut self, endpoints: Vec<Endpoint>, logs: Vec<DeliveryLog>, event_types: Vec<EventTypeMeta>) {
        self.endpoints = endpoints;
        self.logs = logs;
        if self.event_types.is_empty() && !event_types.is_empty() {
            self.event_types = event_types;
        }
        if self.selected_endpoint >= self.endpoints.len() && !self.endpoints.is_empty() {
            self.selected_endpoint = self.endpoints.len() - 1;
        }
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
        }
    }

    fn handle_main_key(&mut self, key: KeyEvent) -> AppAction {
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
        let filtered = self.filtered_log_indices();
        let n = filtered.len();
        match key.code {
            KeyCode::Up | KeyCode::Char('k') if n > 0 && self.selected_log > 0 => {
                self.selected_log -= 1; AppAction::None
            }
            KeyCode::Down | KeyCode::Char('j') if n > 0 && self.selected_log + 1 < n => {
                self.selected_log += 1; AppAction::None
            }
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
            _ => AppAction::None,
        }
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
            KeyCode::PageDown => { self.form.scroll = self.form.scroll.saturating_add(5); return AppAction::None; }
            KeyCode::PageUp => { self.form.scroll = self.form.scroll.saturating_sub(5); return AppAction::None; }
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
    DeliveryDetail(crate::api::models::DeliveryLogDetailDto),
}

impl App {
    pub fn apply_outcome(&mut self, outcome: ActionOutcome) {
        match outcome {
            ActionOutcome::Toast(msg) => self.show_toast(msg),
            ActionOutcome::Created { secret } => {
                self.modal = ModalState::WebhookCreated(secret);
            }
            ActionOutcome::Error(msg) => {
                self.last_error = Some(msg.clone());
                self.show_toast(format!("Error: {msg}"));
            }
            ActionOutcome::DeliveryDetail(d) => self.set_delivery_detail(d),
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
        assert_eq!(app.form.scroll, 10);
    }

    #[test]
    fn cadence_mode_is_backoff_when_form_open() {
        let mut app = App::new(None);
        app.modal = ModalState::CreateWebhook;
        assert_eq!(app.cadence_mode(), crate::poller::CadenceMode::Backoff);
        app.modal = ModalState::None;
        assert_eq!(app.cadence_mode(), crate::poller::CadenceMode::Active);
    }
}
