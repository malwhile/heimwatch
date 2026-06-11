pub mod charts;
pub mod header;
pub mod statusbar;
pub mod table;
pub mod tabs;

use crate::app::App;
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout};

pub fn draw_ui(f: &mut Frame, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),  // header
            Constraint::Length(3),  // tabs
            Constraint::Length(12), // chart
            Constraint::Min(6),     // table
            Constraint::Length(1),  // statusbar
        ])
        .split(f.area());

    header::draw_header(f, app, chunks[0]);
    tabs::draw_tabs(f, app, chunks[1]);
    charts::draw_chart(f, app, chunks[2]);
    table::draw_table(f, app, chunks[3]);
    statusbar::draw_status(f, app, chunks[4]);
}
