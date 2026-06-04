use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;

use crate::app::App;
use crate::data::{self, AppSnapshot};
use heimwatch_storage::StorageLayer;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

pub enum AppEvent {
    Key(KeyEvent),
    Tick,
    Resize(u16, u16),
    DataReady(Box<AppSnapshot>),
    DataError(String),
}

struct TermGuard;

impl Drop for TermGuard {
    fn drop(&mut self) {
        let _ = crossterm::terminal::disable_raw_mode();
        let _ = crossterm::execute!(std::io::stdout(), crossterm::terminal::LeaveAlternateScreen);
    }
}

pub struct TuiApp {
    app: App,
    storage: Arc<StorageLayer>,
}

impl TuiApp {
    pub fn new(app: App, storage: Arc<StorageLayer>) -> Self {
        Self { app, storage }
    }

    pub async fn run(mut self) -> anyhow::Result<()> {
        let _guard = TermGuard;

        crossterm::terminal::enable_raw_mode()?;
        let mut stdout = std::io::stdout();
        crossterm::execute!(stdout, crossterm::terminal::EnterAlternateScreen)?;

        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        let (tx, mut rx) = mpsc::channel::<AppEvent>(32);

        self.load_initial_data(&tx).await;

        let mut event_stream = crossterm::event::EventStream::new();
        use futures::StreamExt;
        let mut reader = Box::pin(event_stream.next());

        let mut tick = tokio::time::interval(Duration::from_secs(5));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            terminal.draw(|f| crate::ui::draw_ui(f, &mut self.app))?;

            if self.app.should_quit {
                break;
            }

            tokio::select! {
                Some(Ok(event)) = &mut reader => {
                    use crossterm::event::Event;
                    match event {
                        Event::Key(k) => {
                            let _ = tx.send(AppEvent::Key(k)).await;
                        }
                        Event::Resize(w, h) => {
                            let _ = tx.send(AppEvent::Resize(w, h)).await;
                        }
                        _ => {}
                    }
                    reader = Box::pin(event_stream.next());
                }
                _ = tick.tick() => {
                    let _ = tx.send(AppEvent::Tick).await;
                }
                Some(event) = rx.recv() => {
                    self.handle_event(event, &tx).await;
                }
            }
        }

        Ok(())
    }

    async fn load_initial_data(&mut self, tx: &mpsc::Sender<AppEvent>) {
        let storage = Arc::clone(&self.storage);
        let window = self.app.time_window_secs;
        let tx = tx.clone();

        tokio::spawn(async move {
            let result =
                tokio::task::spawn_blocking(move || data::load_snapshot(&storage, window)).await;

            match result {
                Ok(Ok(snapshot)) => {
                    let _ = tx.send(AppEvent::DataReady(Box::new(snapshot))).await;
                }
                Ok(Err(e)) => {
                    let _ = tx
                        .send(AppEvent::DataError(format!("Data load failed: {}", e)))
                        .await;
                }
                Err(e) => {
                    let _ = tx
                        .send(AppEvent::DataError(format!("Task error: {}", e)))
                        .await;
                }
            }
        });
    }

    async fn handle_event(&mut self, event: AppEvent, tx: &mpsc::Sender<AppEvent>) {
        match event {
            AppEvent::Key(key) => {
                self.handle_key(key, tx).await;
            }
            AppEvent::Tick => {
                if !self.app.loading {
                    self.app.loading = true;
                    self.load_initial_data(tx).await;
                }
            }
            AppEvent::Resize(_w, _h) => {}
            AppEvent::DataReady(snapshot) => {
                self.app.update_snapshot(*snapshot);
            }
            AppEvent::DataError(err) => {
                self.app.set_error(err);
            }
        }
    }

    async fn handle_key(&mut self, key: KeyEvent, tx: &mpsc::Sender<AppEvent>) {
        match (key.code, key.modifiers) {
            (KeyCode::Char('q') | KeyCode::Char('Q'), _) => {
                self.app.quit();
            }
            (KeyCode::Char('j') | KeyCode::Down, _) => {
                self.app.scroll_down();
            }
            (KeyCode::Char('k') | KeyCode::Up, _) => {
                self.app.scroll_up();
            }
            (KeyCode::Char('h') | KeyCode::Left, _) => {
                self.app.prev_tab();
            }
            (KeyCode::Char('l') | KeyCode::Right, _) => {
                self.app.next_tab();
            }
            (KeyCode::Tab, KeyModifiers::SHIFT) => {
                self.app.prev_tab();
            }
            (KeyCode::Tab, _) => {
                self.app.next_tab();
            }
            (KeyCode::Char('r') | KeyCode::Char('R'), _) => {
                if !self.app.loading {
                    self.app.loading = true;
                    self.load_initial_data(tx).await;
                }
            }
            (KeyCode::Char('+') | KeyCode::Char('='), _) => {
                self.app.widen_window();
                if !self.app.loading {
                    self.app.loading = true;
                    self.load_initial_data(tx).await;
                }
            }
            (KeyCode::Char('-'), _) => {
                self.app.narrow_window();
                if !self.app.loading {
                    self.app.loading = true;
                    self.load_initial_data(tx).await;
                }
            }
            _ => {}
        }
    }
}
