use std::{
    borrow::Borrow,
    collections::{HashMap, HashSet},
    fs,
    path::PathBuf,
    sync::mpsc::{self, RecvTimeoutError},
    time::{Duration, Instant},
};

use crate::{command::Logging, diagnostics, named::Named, remote::Remote, source::Source};
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
    diagnostics::log(format!(
        "watch: creating watcher timeout={timeout:?}, count={count:?}"
    ));
    let mut watcher = recommended_watcher(handler).context("Creating file watcher")?;

    let sources: Vec<_> = sources.into_iter().collect();
    diagnostics::log(format!("watch: received {} sources", sources.len()));
    let mut path_to_source: HashMap<PathBuf, usize> = HashMap::new();

    let mut errors = Errors::new();
    for (index, source) in sources.iter().enumerate() {
        let source = source.borrow();
        diagnostics::log(format!(
            "watch: configuring source index={index}, source={source:?}"
        ));
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
            diagnostics::log(format!(
                "watch: registering source index={index}, source={source}, path={path:?}"
            ));
            errors.collect(
                watcher
                    .watch(&path, RecursiveMode::Recursive)
                    .with_context(|| anyhow!("Failed to watch path {path:?}")),
            );

            path_to_source.insert(path, index);
        }
    }
    diagnostics::log(format!("watch: path_to_source={path_to_source:?}"));
    if !errors.is_empty() {
        diagnostics::log(format!("watch: watcher registration errors={errors:?}"));
    }

    loop {
        let Some(remaining) = timeout.checked_sub(started_at.elapsed()) else {
            diagnostics::log(format!(
                "watch: exiting because timeout elapsed after {:?}, reloads={reloads}",
                started_at.elapsed()
            ));
            return Ok(());
        };
        diagnostics::log(format!(
            "watch: waiting for event remaining={remaining:?}, reloads={reloads}"
        ));

        let Event { paths, .. } = match rx.recv_timeout(remaining) {
            Ok(Ok(event)) => {
                diagnostics::log(format!("watch: received event={event:?}"));
                event
            }
            Ok(Err(err)) => {
                diagnostics::log(format!("watch: received notify error={err:?}"));
                return Err(err.into());
            }
            Err(RecvTimeoutError::Timeout) => {
                diagnostics::log(format!(
                    "watch: recv timed out after {:?}, reloads={reloads}",
                    started_at.elapsed()
                ));
                return Ok(());
            }
            Err(RecvTimeoutError::Disconnected) => {
                diagnostics::log("watch: event channel disconnected");
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
        diagnostics::log(format!(
            "watch: event paths={paths:?}, changed_indices={changed:?}"
        ));

        // If we have changes, reload the affected sources
        let changed: Vec<_> = sources
            .iter()
            .enumerate()
            .filter(|(index, _)| changed.contains(index))
            .map(|(_, source)| source.borrow())
            .collect();

        if changed.is_empty() {
            diagnostics::log("watch: no sources matched event paths");
            continue;
        }

        diagnostics::log(format!(
            "watch: reloading sources {:?}",
            changed
                .iter()
                .map(|source| source.name())
                .collect::<Vec<_>>()
        ));
        let load_result = remote.load(changed, logging.clone());
        diagnostics::log(format!("watch: reload result={load_result:?}"));
        reloads += 1;

        if count.is_some_and(|max_reloads| reloads >= max_reloads) {
            diagnostics::log(format!(
                "watch: exiting because count reached count={count:?}, reloads={reloads}"
            ));
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
                diagnostics::log(format!("watch-handler: raw event={event:?}"));
                if matches!(event.kind, EventKind::Access(_)) {
                    diagnostics::log("watch-handler: ignored access event");
                    return;
                }

                let now = Instant::now();
                self.recently_emitted
                    .retain(|_, emitted_at| now.duration_since(*emitted_at) < EVENT_DEDUP_WINDOW);

                event
                    .paths
                    .retain(|path| !self.recently_emitted.contains_key(path));

                if event.paths.is_empty() {
                    diagnostics::log("watch-handler: ignored event after dedup removed all paths");
                    return;
                }

                for path in &event.paths {
                    self.recently_emitted.insert(path.clone(), now);
                }

                // Emit event as soon as possible, this allows wasvy cli to hit the package cache before the editor
                // Results in faster-feeling hot reloading
                if let Err(err) = self.tx.send(Ok(event)) {
                    diagnostics::log(format!("watch-handler: failed to send event: {err:?}"));
                }
            }
            Err(err) => {
                diagnostics::log(format!("watch-handler: notify error={err:?}"));
                if let Err(err) = self.tx.send(Err(err)) {
                    diagnostics::log(format!("watch-handler: failed to send error: {err:?}"));
                }
            }
        }
    }
}
