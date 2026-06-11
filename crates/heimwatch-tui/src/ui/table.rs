use ratatui::Frame;
use ratatui::layout::{Constraint, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, Borders, Cell, Row, Table};

use crate::app::{App, Tab};

pub fn draw_table(f: &mut Frame, app: &mut App, area: Rect) {
    app.visible_height = area.height.saturating_sub(3); // Account for header row

    let (mut rows, title, column_widths) = match app.snapshot {
        None => (vec![], "Applications".to_string(), vec![]),
        Some(ref snapshot) => match app.active_tab {
            Tab::Overview => build_overview_table(snapshot, app.table_scroll, app.visible_height),
            Tab::Network => build_network_table(snapshot, app.table_scroll, app.visible_height),
            Tab::Cpu => build_cpu_table(snapshot, app.table_scroll, app.visible_height),
            Tab::Focus => build_focus_table(snapshot, app.table_scroll, app.visible_height),
            Tab::Power => build_power_table(snapshot, app.table_scroll, app.visible_height),
        },
    };

    // Add header row
    let header_style = Style::default().add_modifier(Modifier::BOLD | Modifier::UNDERLINED);
    let header = match app.active_tab {
        Tab::Overview => Row::new(vec![
            Cell::from("App").style(header_style),
            Cell::from("CPU").style(header_style),
            Cell::from("Memory").style(header_style),
            Cell::from("Network").style(header_style),
        ]),
        Tab::Network => Row::new(vec![
            Cell::from("App").style(header_style),
            Cell::from("TX").style(header_style),
            Cell::from("RX").style(header_style),
            Cell::from("Total").style(header_style),
        ]),
        Tab::Cpu => Row::new(vec![
            Cell::from("App").style(header_style),
            Cell::from("CPU %").style(header_style),
            Cell::from("Threads").style(header_style),
            Cell::from("CPU Time").style(header_style),
        ]),
        Tab::Focus => Row::new(vec![
            Cell::from("App").style(header_style),
            Cell::from("Duration").style(header_style),
        ]),
        Tab::Power => Row::new(vec![
            Cell::from("App").style(header_style),
            Cell::from("Power %").style(header_style),
            Cell::from("CPU").style(header_style),
            Cell::from("GPU").style(header_style),
            Cell::from("Display").style(header_style),
            Cell::from("Disk").style(header_style),
        ]),
    };
    rows.insert(0, header);

    let total_count = app.current_tab_item_count();
    let _displayed = rows.len().saturating_sub(1); // Don't count header

    let title_str = if total_count == 0 {
        format!(" {} (no data) ", title)
    } else {
        format!(
            " {} (↑↓ {}/{}) ",
            title,
            app.table_scroll.min(total_count.saturating_sub(1)),
            total_count
        )
    };

    let table = Table::new(rows, &column_widths)
        .block(Block::default().title(title_str).borders(Borders::ALL))
        .style(Style::default());

    f.render_widget(table, area);
}

fn build_overview_table(
    snapshot: &crate::data::AppSnapshot,
    scroll: usize,
    visible_height: u16,
) -> (Vec<Row<'_>>, String, Vec<Constraint>) {
    let visible_height = visible_height as usize;
    let rows = snapshot
        .top_cpu_apps
        .iter()
        .skip(scroll)
        .take(visible_height)
        .enumerate()
        .map(|(i, cpu_stats)| {
            let is_selected = i == 0 && scroll < snapshot.top_cpu_apps.len();
            let style = if is_selected {
                Style::default().bg(Color::DarkGray)
            } else {
                Style::default()
            };

            // Find matching network and memory data for this app
            let (tx_bytes, rx_bytes) = snapshot
                .top_net_apps
                .iter()
                .find(|n| n.app_name == cpu_stats.app_name)
                .map(|n| (n.tx_bytes, n.rx_bytes))
                .unwrap_or((0, 0));

            let memory_mb = snapshot.current_ram_mb / snapshot.top_cpu_apps.len().max(1) as u64;

            Row::new(vec![
                Cell::from(truncate(&cpu_stats.app_name, 22)).style(style),
                Cell::from(format!("{:.1}%", cpu_stats.cpu_usage_percent)).style(style),
                Cell::from(format!("{}MB", memory_mb)).style(style),
                Cell::from(format!("↑{} ↓{}", fmt_bytes(tx_bytes), fmt_bytes(rx_bytes))).style(style),
            ])
        })
        .collect();

    (
        rows,
        "Applications".to_string(),
        vec![
            Constraint::Percentage(30),
            Constraint::Percentage(15),
            Constraint::Percentage(15),
            Constraint::Percentage(40),
        ],
    )
}

fn build_network_table(
    snapshot: &crate::data::AppSnapshot,
    scroll: usize,
    visible_height: u16,
) -> (Vec<Row<'_>>, String, Vec<Constraint>) {
    let visible_height = visible_height as usize;
    let rows = snapshot
        .top_net_apps
        .iter()
        .skip(scroll)
        .take(visible_height)
        .enumerate()
        .map(|(i, app)| {
            let is_selected = i == 0 && scroll < snapshot.top_net_apps.len();
            let style = if is_selected {
                Style::default().bg(Color::DarkGray)
            } else {
                Style::default()
            };

            Row::new(vec![
                Cell::from(truncate(&app.app_name, 28)).style(style),
                Cell::from(fmt_bytes(app.tx_bytes)).style(style),
                Cell::from(fmt_bytes(app.rx_bytes)).style(style),
                Cell::from(fmt_bytes(app.tx_bytes + app.rx_bytes)).style(style),
            ])
        })
        .collect();

    (
        rows,
        "Network".to_string(),
        vec![
            Constraint::Percentage(40),
            Constraint::Percentage(20),
            Constraint::Percentage(20),
            Constraint::Percentage(20),
        ],
    )
}

fn build_cpu_table(
    snapshot: &crate::data::AppSnapshot,
    scroll: usize,
    visible_height: u16,
) -> (Vec<Row<'_>>, String, Vec<Constraint>) {
    let visible_height = visible_height as usize;
    let rows = snapshot
        .top_cpu_apps
        .iter()
        .skip(scroll)
        .take(visible_height)
        .enumerate()
        .map(|(i, cpu_stats)| {
            let is_selected = i == 0 && scroll < snapshot.top_cpu_apps.len();
            let style = if is_selected {
                Style::default().bg(Color::DarkGray)
            } else {
                Style::default()
            };

            let cpu_time_secs = cpu_stats.cpu_time_ns / 1_000_000_000;
            let cpu_time_str = if cpu_time_secs < 60 {
                format!("{}s", cpu_time_secs)
            } else if cpu_time_secs < 3600 {
                format!("{}m", cpu_time_secs / 60)
            } else {
                format!("{}h", cpu_time_secs / 3600)
            };

            Row::new(vec![
                Cell::from(truncate(&cpu_stats.app_name, 22)).style(style),
                Cell::from(format!("{:.1}%", cpu_stats.cpu_usage_percent)).style(style),
                Cell::from(format!("{}", cpu_stats.thread_count)).style(style),
                Cell::from(cpu_time_str).style(style),
            ])
        })
        .collect();

    (
        rows,
        "CPU".to_string(),
        vec![
            Constraint::Percentage(40),
            Constraint::Percentage(20),
            Constraint::Percentage(20),
            Constraint::Percentage(20),
        ],
    )
}

fn build_focus_table(
    snapshot: &crate::data::AppSnapshot,
    scroll: usize,
    visible_height: u16,
) -> (Vec<Row<'_>>, String, Vec<Constraint>) {
    let visible_height = visible_height as usize;
    let rows = snapshot
        .top_focus_apps
        .iter()
        .skip(scroll)
        .take(visible_height)
        .enumerate()
        .map(|(i, app)| {
            let is_selected = i == 0 && scroll < snapshot.top_focus_apps.len();
            let style = if is_selected {
                Style::default().bg(Color::DarkGray)
            } else {
                Style::default()
            };

            let duration_str = format_duration_ms(app.total_duration_ms);

            Row::new(vec![
                Cell::from(truncate(&app.app_name, 28)).style(style),
                Cell::from(duration_str).style(style),
            ])
        })
        .collect();

    (
        rows,
        "Focus".to_string(),
        vec![Constraint::Percentage(60), Constraint::Percentage(40)],
    )
}

fn build_power_table(
    snapshot: &crate::data::AppSnapshot,
    scroll: usize,
    visible_height: u16,
) -> (Vec<Row<'_>>, String, Vec<Constraint>) {
    let visible_height = visible_height as usize;
    let rows = snapshot
        .top_power_apps
        .iter()
        .skip(scroll)
        .take(visible_height)
        .enumerate()
        .map(|(i, app)| {
            let is_selected = i == 0 && scroll < snapshot.top_power_apps.len();
            let style = if is_selected {
                Style::default().bg(Color::DarkGray)
            } else {
                Style::default()
            };

            Row::new(vec![
                Cell::from(truncate(&app.app_name, 15)).style(style),
                Cell::from(format!("{:.1}%", app.power_pct)).style(style),
                Cell::from(format!("{:.1}%", app.cpu_contribution)).style(style),
                Cell::from(format!("{:.1}%", app.gpu_contribution)).style(style),
                Cell::from(format!("{:.1}%", app.display_contribution)).style(style),
                Cell::from(format!("{:.1}%", app.disk_contribution)).style(style),
            ])
        })
        .collect();

    (
        rows,
        "Power".to_string(),
        vec![
            Constraint::Percentage(35),
            Constraint::Percentage(13),
            Constraint::Percentage(13),
            Constraint::Percentage(13),
            Constraint::Percentage(13),
            Constraint::Percentage(13),
        ],
    )
}

fn truncate(s: &str, max_len: usize) -> String {
    if s.len() > max_len {
        format!("{}…", &s[..max_len.saturating_sub(1)])
    } else {
        s.to_string()
    }
}

fn fmt_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.1} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}

fn format_duration_ms(ms: u64) -> String {
    let seconds = ms / 1000;
    let minutes = seconds / 60;
    let hours = minutes / 60;
    let days = hours / 24;

    if days > 0 {
        format!("{} days", days)
    } else if hours > 0 {
        format!("{}h {}m", hours, minutes % 60)
    } else if minutes > 0 {
        format!("{}m", minutes)
    } else {
        format!("{}s", seconds)
    }
}
