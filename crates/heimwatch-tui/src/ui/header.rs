use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::App;

pub fn draw_header(f: &mut Frame, app: &App, area: Rect) {
    let line = if let Some(snapshot) = &app.snapshot {
        let cpu_pct = snapshot.current_cpu_pct;
        let cpu_color = if cpu_pct > 80.0 {
            Color::Red
        } else if cpu_pct > 50.0 {
            Color::Yellow
        } else {
            Color::Green
        };

        let battery_str = if let Some(pct) = snapshot.battery_pct {
            if snapshot.charging {
                format!("Battery: {:.0}%  ⚡", pct)
            } else {
                format!("Battery: {:.0}%", pct)
            }
        } else {
            "Battery: N/A".to_string()
        };

        let spans = vec![
            Span::styled(
                format!(" CPU: {:.1}% ", cpu_pct),
                Style::default().fg(cpu_color),
            ),
            Span::styled(
                format!("  RAM: {:.1} GB ", snapshot.current_ram_mb as f32 / 1024.0),
                Style::default().fg(Color::Blue),
            ),
            Span::styled(
                format!("  TX: {:.1} KB/s ", snapshot.current_net_tx as f32 / 1024.0),
                Style::default().fg(Color::Cyan),
            ),
            Span::styled(
                format!("  RX: {:.1} KB/s ", snapshot.current_net_rx as f32 / 1024.0),
                Style::default().fg(Color::Cyan),
            ),
            Span::styled(
                format!("  {} ", battery_str),
                Style::default().fg(if snapshot.charging {
                    Color::Green
                } else {
                    Color::Yellow
                }),
            ),
        ];

        Line::from(spans)
    } else {
        Line::from(Span::styled(
            " Connecting...",
            Style::default().fg(Color::DarkGray),
        ))
    };

    let para = Paragraph::new(line);
    f.render_widget(para, area);
}
