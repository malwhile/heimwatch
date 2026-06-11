pub mod app;
pub mod data;
pub mod events;
pub mod ui;

use std::sync::Arc;

use app::App;
use events::TuiApp;
use heimwatch_storage::StorageLayer;

pub async fn run(storage: Arc<StorageLayer>, db_path: String) -> anyhow::Result<()> {
    let app = App::new(db_path);
    let tui_app = TuiApp::new(app, storage);
    tui_app.run().await
}
