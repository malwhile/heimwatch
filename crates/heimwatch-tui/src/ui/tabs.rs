use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, Borders, Tabs};
use ratatui::Frame;

use crate::app::{App, Tab};

pub fn draw_tabs(f: &mut Frame, app: &App, area: Rect) {
    let titles: Vec<&str> = Tab::ALL.iter().map(|t| t.title()).collect();

    let tabs = Tabs::new(titles)
        .block(Block::default().borders(Borders::ALL))
        .select(app.active_tab.index())
        .style(Style::default().fg(Color::Gray))
        .highlight_style(
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        );

    f.render_widget(tabs, area);
}
