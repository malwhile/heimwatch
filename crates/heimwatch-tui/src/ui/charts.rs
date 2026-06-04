use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::widgets::{Bar, BarChart, BarGroup, Block, Borders, Paragraph, Sparkline};
use ratatui::Frame;

use crate::app::{App, Tab};

pub fn draw_chart(f: &mut Frame, app: &App, area: Rect) {
    match app.snapshot {
        None => {
            let para = Paragraph::new("No data — waiting for refresh...").style(
                Style::default().fg(ratatui::style::Color::DarkGray),
            );
            f.render_widget(para, area);
        }
        Some(ref snapshot) => match app.active_tab {
            Tab::Overview => draw_overview_chart(f, snapshot, area),
            Tab::Network => draw_network_chart(f, snapshot, area),
            Tab::Cpu => draw_cpu_chart(f, snapshot, area),
            Tab::Focus => draw_focus_chart(f, snapshot, area),
            Tab::Power => draw_power_chart(f, snapshot, area),
        },
    }
}

fn draw_overview_chart(f: &mut Frame, snapshot: &crate::data::AppSnapshot, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);

    let cpu_sparkline = Sparkline::default()
        .block(Block::default().title(" CPU % ").borders(Borders::ALL))
        .data(snapshot.cpu_series.as_slice())
        .max(100)
        .style(Style::default().fg(Color::Yellow));

    let mem_max = snapshot.mem_series.iter().copied().max().unwrap_or(100);
    let mem_sparkline = Sparkline::default()
        .block(Block::default().title(" RAM (MB) ").borders(Borders::ALL))
        .data(snapshot.mem_series.as_slice())
        .max(mem_max)
        .style(Style::default().fg(Color::Blue));

    f.render_widget(cpu_sparkline, chunks[0]);
    f.render_widget(mem_sparkline, chunks[1]);
}

fn draw_network_chart(f: &mut Frame, snapshot: &crate::data::AppSnapshot, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);

    let tx_max = snapshot.net_tx_series.iter().copied().max().unwrap_or(100);
    let tx_sparkline = Sparkline::default()
        .block(Block::default().title(" TX (bytes) ").borders(Borders::ALL))
        .data(snapshot.net_tx_series.as_slice())
        .max(tx_max)
        .style(Style::default().fg(Color::Cyan));

    let rx_max = snapshot.net_rx_series.iter().copied().max().unwrap_or(100);
    let rx_sparkline = Sparkline::default()
        .block(Block::default().title(" RX (bytes) ").borders(Borders::ALL))
        .data(snapshot.net_rx_series.as_slice())
        .max(rx_max)
        .style(Style::default().fg(Color::Cyan));

    f.render_widget(tx_sparkline, chunks[0]);
    f.render_widget(rx_sparkline, chunks[1]);
}

fn draw_cpu_chart(f: &mut Frame, snapshot: &crate::data::AppSnapshot, area: Rect) {
    let bars: Vec<(String, u64)> = snapshot
        .top_cpu_apps
        .iter()
        .take(10)
        .map(|(app, pct)| (truncate_app_name(app), (*pct as u64).min(100)))
        .collect();

    if bars.is_empty() {
        let para = Paragraph::new("No data");
        f.render_widget(para, area);
        return;
    }

    let bar_group = BarGroup::default().bars(
        &bars
            .iter()
            .map(|(name, val)| Bar::default().value(*val).label(ratatui::text::Line::from(name.clone())))
            .collect::<Vec<_>>(),
    );

    let chart = BarChart::default()
        .block(Block::default().title(" CPU % ").borders(Borders::ALL))
        .data(bar_group)
        .bar_width(7)
        .bar_gap(1)
        .max(100)
        .style(Style::default().fg(Color::Yellow));

    f.render_widget(chart, area);
}

fn draw_focus_chart(f: &mut Frame, snapshot: &crate::data::AppSnapshot, area: Rect) {
    let bars: Vec<(String, u64)> = snapshot
        .top_focus_apps
        .iter()
        .take(10)
        .map(|stats| {
            let minutes = stats.total_duration_ms / 60_000;
            (truncate_app_name(&stats.app_name), minutes)
        })
        .collect();

    if bars.is_empty() {
        let para = Paragraph::new("No data");
        f.render_widget(para, area);
        return;
    }

    let max_val = bars.iter().map(|(_, v)| *v).max().unwrap_or(100).max(1);

    let bar_group = BarGroup::default().bars(
        &bars
            .iter()
            .map(|(name, val)| Bar::default().value(*val).label(ratatui::text::Line::from(name.clone())))
            .collect::<Vec<_>>(),
    );

    let chart = BarChart::default()
        .block(Block::default().title(" Focus (minutes) ").borders(Borders::ALL))
        .data(bar_group)
        .bar_width(7)
        .bar_gap(1)
        .max(max_val)
        .style(Style::default().fg(Color::Green));

    f.render_widget(chart, area);
}

fn draw_power_chart(f: &mut Frame, snapshot: &crate::data::AppSnapshot, area: Rect) {
    let bars: Vec<(String, u64)> = snapshot
        .top_power_apps
        .iter()
        .take(10)
        .map(|stats| {
            (
                truncate_app_name(&stats.app_name),
                (stats.power_pct as u64).min(100),
            )
        })
        .collect();

    if bars.is_empty() {
        let para = Paragraph::new("No data");
        f.render_widget(para, area);
        return;
    }

    let bar_group = BarGroup::default().bars(
        &bars
            .iter()
            .map(|(name, val)| Bar::default().value(*val).label(ratatui::text::Line::from(name.clone())))
            .collect::<Vec<_>>(),
    );

    let chart = BarChart::default()
        .block(Block::default().title(" Power % ").borders(Borders::ALL))
        .data(bar_group)
        .bar_width(7)
        .bar_gap(1)
        .max(100)
        .style(Style::default().fg(Color::Magenta));

    f.render_widget(chart, area);
}

fn truncate_app_name(name: &str) -> String {
    if name.len() > 12 {
        format!("{}…", &name[..11])
    } else {
        name.to_string()
    }
}
