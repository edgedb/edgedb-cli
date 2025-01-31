use std::collections::HashSet;
use std::path::{Path, PathBuf};

use notify::{EventKind, RecursiveMode, Watcher};
use tokio::sync::mpsc;
use tokio::time::Duration;

use crate::interrupt::Interrupt;

const STABLE_TIME: Duration = Duration::from_millis(100);

pub struct FsWatcher {
    rx: mpsc::UnboundedReceiver<Vec<PathBuf>>,
    inner: notify::RecommendedWatcher,
}

impl FsWatcher {
    pub fn new() -> anyhow::Result<Self> {
        let (tx, rx) = mpsc::unbounded_channel::<Vec<PathBuf>>();
        let handler = WatchHandler { tx };
        let watch = notify::recommended_watcher(handler)?;
        Ok(FsWatcher { rx, inner: watch })
    }

    pub fn watch(&mut self, path: &Path, recursive_mode: RecursiveMode) -> notify::Result<()> {
        self.inner.watch(path, recursive_mode)
    }

    #[allow(dead_code)]
    pub fn clear_queue(&mut self) {
        while self.rx.try_recv().is_ok() {}
    }

    /// Waits for either changes in fs, timeout or interrupt signal
    pub async fn wait(&mut self, timeout: Option<Duration>) -> anyhow::Result<HashSet<PathBuf>> {
        let ctrl_c = Interrupt::ctrl_c();
        tokio::select! {
            changes = self.wait_for_changes() => Ok(changes),
            _ = wait_for_timeout(timeout) => Ok(HashSet::default()),
            res = ctrl_c.wait_result() => res,
        }
    }

    async fn wait_for_changes(&mut self) -> HashSet<PathBuf> {
        let mut changed_paths = HashSet::new();

        let mut timeout = None;
        loop {
            tokio::select! {
                _ = wait_for_timeout(timeout) => { return changed_paths },
                paths = self.rx.recv() => {
                    if let Some(paths) = paths {
                        changed_paths.extend(paths);
                    } else {
                        return changed_paths;
                    }
                    if changed_paths.is_empty() {
                        timeout = None;
                    } else {
                        timeout = Some(STABLE_TIME);
                    }
                },
            }
        }
    }
}

async fn wait_for_timeout(timeout: Option<Duration>) {
    tokio::time::sleep(timeout.unwrap_or(Duration::MAX)).await;
}

struct WatchHandler {
    tx: mpsc::UnboundedSender<Vec<PathBuf>>,
}

impl notify::EventHandler for WatchHandler {
    fn handle_event(&mut self, event: notify::Result<notify::Event>) {
        match event {
            Ok(e) => {
                if matches!(
                    e.kind,
                    EventKind::Modify(_) | EventKind::Create(_) | EventKind::Remove(_)
                ) {
                    let res = self.tx.send(e.paths);

                    if let Err(e) = res {
                        log::warn!("Error watching filesystem: {:#}", e)
                    }
                }
            }
            Err(e) => log::warn!("Error watching filesystem: {:#}", e),
        }
    }
}
