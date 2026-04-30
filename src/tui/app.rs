#[allow(unused_imports)]
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
