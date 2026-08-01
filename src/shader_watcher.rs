use std::path::Path;
use std::sync::mpsc;

use log::*;
use notify::{Event, RecursiveMode, Watcher};

pub struct ShaderChanges {
    #[expect(unused)]
    watcher: notify::RecommendedWatcher,
    receiver: mpsc::Receiver<notify::Result<Event>>,
}

impl ShaderChanges {
    /// gathers all shader source edit events since the last frame
    /// (or since this function was last called)
    pub fn events(&mut self) -> anyhow::Result<Vec<notify::Event>> {
        let events: notify::Result<Vec<notify::Event>> = self.receiver.try_iter().collect();
        let mut events = events?;

        events.retain(|event| match event.kind {
            notify::EventKind::Create(_) => true,
            notify::EventKind::Modify(_) => true,
            notify::EventKind::Remove(_) => true,

            notify::EventKind::Access(_) => false,
            notify::EventKind::Any => {
                error!("unexpected notify event: {event:?}");
                false
            }
            notify::EventKind::Other => {
                error!("unexpected notify event: {event:?}");
                false
            }
        });

        Ok(events)
    }
}

pub fn watch(dir: &Path) -> notify::Result<ShaderChanges> {
    let (sender, receiver) = mpsc::channel::<notify::Result<Event>>();

    let mut watcher = notify::recommended_watcher(sender)?;

    watcher.watch(dir, RecursiveMode::Recursive)?;

    Ok(ShaderChanges { watcher, receiver })
}
