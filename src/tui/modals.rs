use ratatui::Frame;
use crate::tui::app::App;

pub fn render_create_modal(_frame: &mut Frame, _app: &App) {}
pub fn render_edit_modal(_frame: &mut Frame, _app: &App) {}
pub fn render_delete_modal(_frame: &mut Frame, _app: &App, _idx: usize) {}
pub fn render_created_modal(_frame: &mut Frame, _secret: &str) {}
pub fn render_details_modal(_frame: &mut Frame, _app: &App, _log_id: &str) {}
