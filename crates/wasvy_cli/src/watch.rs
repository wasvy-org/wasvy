use std::{
    borrow::Borrow,
    collections::{HashMap, HashSet},
    fs,
    path::PathBuf,
    sync::mpsc::{self, RecvTimeoutError},
    time::{Duration, Instant},
};

use crate::{command::Logging, remote::Remote, source::Source};
use anyhow::{Context, Result, anyhow};
use error_collection::Errors;
use notify::{Event, EventHandler, EventKind, RecursiveMode, Watcher, recommended_watcher};

/// Given a list of sources, watch for changes and build/load sources upon any changes
pub fn watch(
    sources: impl IntoIterator<Item = impl Borrow<Source>>,
    remote: &Remote,
    timeout: Duration,
    count: Option<usize>,
    logging: Logging,
) -> Result<()> {
    let started_at = Instant::now();
    let mut reloads = 0;
    let (handler, rx) = WatchHandler::new();
    let mut watcher = recommended_watcher(handler).context("Creating file watcher")?;

    let sources: Vec<_> = sources.into_iter().collect();
    let mut path_to_source: HashMap<PathBuf, usize> = HashMap::new();

    let mut errors = Errors::new();
    for (index, source) in sources.iter().enumerate() {
        let source = source.borrow();
        if let Ok(path) = fs::canonicalize(source.path())
            && path.starts_with(&remote.asset_dir)
        {
            logging.eprintln(format!("Ignoring {source} already in asset directory"));
            continue;
        }

        let paths = source.watch_paths();

        if !paths.is_empty() {
            logging.println(format!("Watching {source}: {:?}", source.path()));
        }

        for path in paths {
            errors.collect(
                watcher
                    .watch(&path, RecursiveMode::Recursive)
                    .with_context(|| anyhow!("Failed to watch path {path:?}")),
            );

            path_to_source.insert(path, index);
        }
    }

    loop {
        let Some(remaining) = timeout.checked_sub(started_at.elapsed()) else {
            return Ok(());
        };

        let Event { paths, .. } = match rx.recv_timeout(remaining) {
            Ok(Ok(event)) => event,
            Ok(Err(err)) => return Err(err.into()),
            Err(RecvTimeoutError::Timeout) => return Ok(()),
            Err(RecvTimeoutError::Disconnected) => {
                return Err(anyhow!("watch event channel disconnected"));
            }
        };

        // Determine which sources changed
        let changed: HashSet<usize> = paths
            .iter()
            .flat_map(|path| {
                path_to_source
                    .iter()
                    .filter(|(source_path, _)| path.starts_with(source_path))
                    .map(|(_, index)| index)
                    .cloned()
            })
            .collect();

        // If we have changes, reload the affected sources
        let changed: Vec<_> = sources
            .iter()
            .enumerate()
            .filter(|(index, _)| changed.contains(index))
            .map(|(_, source)| source.borrow())
            .collect();

        if changed.is_empty() {
            continue;
        }

        let _ = remote.load(changed, logging.clone());
        reloads += 1;

        if count.is_some_and(|count| reloads >= count) {
            return Ok(());
        }
    }
}

const EVENT_DEDUP_WINDOW: Duration = Duration::from_millis(50);

type NotifyResult = notify::Result<Event>;

struct WatchHandler {
    tx: mpsc::Sender<NotifyResult>,
    recently_emitted: HashMap<PathBuf, Instant>,
}

impl WatchHandler {
    fn new() -> (Self, mpsc::Receiver<NotifyResult>) {
        let (tx, rx) = mpsc::channel();
        (
            WatchHandler {
                tx,
                recently_emitted: HashMap::new(),
            },
            rx,
        )
    }
}

impl EventHandler for WatchHandler {
    fn handle_event(&mut self, event: notify::Result<Event>) {
        match event {
            Ok(mut event) => {
                if matches!(event.kind, EventKind::Access(_)) {
                    return;
                }

                let now = Instant::now();
                self.recently_emitted
                    .retain(|_, emitted_at| now.duration_since(*emitted_at) < EVENT_DEDUP_WINDOW);

                event
                    .paths
                    .retain(|path| !self.recently_emitted.contains_key(path));

                if event.paths.is_empty() {
                    return;
                }

                for path in &event.paths {
                    self.recently_emitted.insert(path.clone(), now);
                }

                // Emit event as soon as possible, this allows wasvy cli to hit the package cache before the editor
                // Results in faster-feeling hot reloading
                let _ = self.tx.send(Ok(event));
            }
            Err(err) => {
                let _ = self.tx.send(Err(err));
            }
        }
    }
}
