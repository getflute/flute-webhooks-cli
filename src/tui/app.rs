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

    // Stubs replaced in Task 18:
    fn handle_form_key(&mut self, _key: KeyEvent) -> AppAction { AppAction::None }
    fn handle_delete_key(&mut self, _key: KeyEvent, _idx: usize) -> AppAction { AppAction::None }
    fn handle_created_key(&mut self, _key: KeyEvent) -> AppAction { AppAction::None }
    fn handle_details_key(&mut self, _key: KeyEvent) -> AppAction { AppAction::None }
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
}
