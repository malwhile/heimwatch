use crate::data::AppSnapshot;
use chrono::{DateTime, Local};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Overview,
    Network,
    Cpu,
    Focus,
    Power,
}

impl Tab {
    pub const ALL: &'static [Tab] = &[
        Tab::Overview,
        Tab::Network,
        Tab::Cpu,
        Tab::Focus,
        Tab::Power,
    ];

    pub fn title(&self) -> &'static str {
        match self {
            Tab::Overview => "Overview",
            Tab::Network => "Network",
            Tab::Cpu => "CPU",
            Tab::Focus => "Focus",
            Tab::Power => "Power",
        }
    }

    pub fn index(&self) -> usize {
        Self::ALL.iter().position(|&t| t == *self).unwrap_or(0)
    }
}

pub struct App {
    pub active_tab: Tab,
    pub table_scroll: usize,
    pub visible_height: u16,
    pub snapshot: Option<AppSnapshot>,
    pub last_refresh: Option<DateTime<Local>>,
    pub db_path: String,
    pub loading: bool,
    pub error_message: Option<String>,
    pub should_quit: bool,
    pub time_window_secs: u64,
}

impl App {
    pub fn new(db_path: String) -> Self {
        Self {
            active_tab: Tab::Overview,
            table_scroll: 0,
            visible_height: 10,
            snapshot: None,
            last_refresh: None,
            db_path,
            loading: false,
            error_message: None,
            should_quit: false,
            time_window_secs: 86400, // 24 hours default
        }
    }

    pub fn next_tab(&mut self) {
        let current_idx = self.active_tab.index();
        let next_idx = (current_idx + 1) % Tab::ALL.len();
        self.active_tab = Tab::ALL[next_idx];
        self.table_scroll = 0;
    }

    pub fn prev_tab(&mut self) {
        let current_idx = self.active_tab.index();
        let next_idx = if current_idx == 0 {
            Tab::ALL.len() - 1
        } else {
            current_idx - 1
        };
        self.active_tab = Tab::ALL[next_idx];
        self.table_scroll = 0;
    }

    pub fn scroll_down(&mut self) {
        let max = self.current_tab_item_count().saturating_sub(1);
        if self.table_scroll < max {
            self.table_scroll += 1;
        }
    }

    pub fn scroll_up(&mut self) {
        self.table_scroll = self.table_scroll.saturating_sub(1);
    }

    pub fn quit(&mut self) {
        self.should_quit = true;
    }

    pub fn widen_window(&mut self) {
        let max_secs = 7 * 24 * 3600; // 7 days
        self.time_window_secs = (self.time_window_secs * 2).min(max_secs);
    }

    pub fn narrow_window(&mut self) {
        const MIN_SECS: u64 = 300; // 5 minutes
        self.time_window_secs = (self.time_window_secs / 2).max(MIN_SECS);
    }

    pub fn update_snapshot(&mut self, snapshot: AppSnapshot) {
        self.snapshot = Some(snapshot);
        self.last_refresh = Some(Local::now());
        self.loading = false;
        self.error_message = None;
        self.table_scroll = 0;
    }

    pub fn set_error(&mut self, error: String) {
        self.error_message = Some(error);
        self.loading = false;
    }

    pub fn current_tab_item_count(&self) -> usize {
        match self.snapshot {
            None => 0,
            Some(ref snap) => match self.active_tab {
                Tab::Overview => snap.top_cpu_apps.len(),
                Tab::Network => snap.top_net_apps.len(),
                Tab::Cpu => snap.top_cpu_apps.len(),
                Tab::Focus => snap.top_focus_apps.len(),
                Tab::Power => snap.top_power_apps.len(),
            },
        }
    }
}
