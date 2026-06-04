use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::App;

pub fn draw_status(f: &mut Frame, app: &App, area: Rect) {
    let left_span = if app.loading {
        Span::styled(
            format!(" DB: {}   Loading... ", app.db_path),
            Style::default().fg(Color::Yellow),
        )
    } else {
        let time_str = app
            .last_refresh
            .map(|t| t.format("%H:%M:%S").to_string())
            .unwrap_or_else(|| "—".to_string());

        Span::raw(format!(" DB: {}   Last: {}   ", app.db_path, time_str))
    };

    let hints = Span::raw("[q]uit  [h/l]tabs  [j/k]scroll  [r]efresh  [+/-]window  ");

    let line = Line::from(vec![left_span, hints]);

    let para = Paragraph::new(line).style(Style::default().fg(Color::Gray));
    f.render_widget(para, area);
}
