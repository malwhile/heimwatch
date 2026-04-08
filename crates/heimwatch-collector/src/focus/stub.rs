use anyhow::Result;

pub struct FocusCollector;

impl FocusCollector {
    pub fn try_new() -> Option<Self> {
        None
    }

    pub async fn run(
        &self,
        _storage: Arc<StorageLayer>,
        mut _shutdown: watch::Receiver<bool>,
    ) -> Result<()> {
        anyhow::bail!("Not yet implemented")
    }
}
