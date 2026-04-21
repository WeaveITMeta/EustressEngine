// File watcher — the engine behind `resources/updated` notifications.
//
// MCP clients subscribe to a resource URI; when the underlying file changes on
// disk, we emit a notification and the client refreshes its pinned copy. This
// is the killer-feature part of resources: it makes them **subscriptions**,
// not RPCs.
//
// One watcher per active Universe. When the default Universe swaps mid-session
// (via `eustress_set_default_universe`), the SubscriptionManager tears down
// the old watcher and starts a new one lazily — but only if any subscriptions
// exist. No subscribers = no watcher, keeping just-launched-server cost at
// near zero.

use notify::{RecursiveMode, Watcher};
use notify_debouncer_full::{new_debouncer, DebounceEventResult, Debouncer, FileIdMap};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::resources::path_to_uri;

pub type UpdateEmitter = Arc<dyn Fn(String) + Send + Sync>;

pub struct UniverseWatcher {
    pub universe: PathBuf,
    _debouncer: Debouncer<notify::RecommendedWatcher, FileIdMap>,
}

impl UniverseWatcher {
    pub fn start(universe: PathBuf, on_update: UpdateEmitter) -> notify::Result<Self> {
        let universe_for_callback = universe.clone();
        // 120 ms debounce mirrors the TS chokidar config (awaitWriteFinish 120ms
        // stabilityThreshold) so editors that save via write-then-rename don't
        // emit spurious events for intermediate states.
        let mut debouncer = new_debouncer(
            Duration::from_millis(120),
            None,
            move |res: DebounceEventResult| {
                let events = match res {
                    Ok(events) => events,
                    Err(_) => return,
                };
                for event in events {
                    for path in &event.event.paths {
                        // Filter: only text-ish files we actually resolve to a
                        // URI. notify watches the entire subtree, so we have
                        // to gatekeep here rather than at subscribe time.
                        if !is_interesting(path) {
                            continue;
                        }
                        if let Some(uri) = path_to_uri(&universe_for_callback, path) {
                            on_update(uri);
                        }
                    }
                }
            },
        )?;

        // Watch Spaces/ for content files + .eustress/knowledge/sessions/ for
        // Workshop history. Recursive mode is required — notify doesn't re-add
        // watchers as subdirectories are created unless we ask for it.
        let spaces = universe.join("Spaces");
        if spaces.is_dir() {
            debouncer.watcher().watch(&spaces, RecursiveMode::Recursive)?;
        }
        let sessions = universe.join(".eustress").join("knowledge").join("sessions");
        if sessions.is_dir() {
            debouncer
                .watcher()
                .watch(&sessions, RecursiveMode::NonRecursive)?;
        }

        Ok(Self {
            universe,
            _debouncer: debouncer,
        })
    }
}

fn is_interesting(path: &std::path::Path) -> bool {
    // Drop .git internals + any path containing an ignored segment.
    let s = path.to_string_lossy();
    if s.contains("/.git/") || s.contains("\\.git\\") {
        return false;
    }
    if s.contains("/node_modules/") || s.contains("\\node_modules\\") {
        return false;
    }
    if s.contains("/target/") || s.contains("\\target\\") {
        return false;
    }
    let ext = match path.extension().and_then(|s| s.to_str()) {
        Some(e) => e.to_lowercase(),
        None => return false,
    };
    matches!(
        ext.as_str(),
        "rune" | "luau" | "soul" | "md" | "toml" | "json",
    )
}

/// Tracks which URIs are actively subscribed and ensures exactly one watcher
/// runs per Universe whenever at least one subscription exists.
pub struct SubscriptionManager {
    state: Arc<Mutex<ManagerState>>,
    on_notify: Arc<dyn Fn(String) + Send + Sync>,
}

struct ManagerState {
    subscribed: HashSet<String>,
    watcher: Option<UniverseWatcher>,
}

impl SubscriptionManager {
    pub fn new<F>(on_notify: F) -> Self
    where
        F: Fn(String) + Send + Sync + 'static,
    {
        Self {
            state: Arc::new(Mutex::new(ManagerState {
                subscribed: HashSet::new(),
                watcher: None,
            })),
            on_notify: Arc::new(on_notify),
        }
    }

    /// Call when `state.currentUniverse` changes. Tears down the old watcher
    /// (if any) and starts a new one on the new Universe — but only if there's
    /// anyone to notify.
    pub fn retarget_universe(&self, universe: Option<PathBuf>) {
        let mut state = self.state.lock().unwrap();
        let same = state
            .watcher
            .as_ref()
            .map(|w| Some(&w.universe) == universe.as_ref())
            .unwrap_or(false);
        if same {
            return;
        }
        state.watcher = None;
        if let Some(u) = universe {
            if !state.subscribed.is_empty() {
                state.watcher = self.spawn_watcher(u);
            }
        }
    }

    pub fn subscribe(&self, uri: String, universe: Option<PathBuf>) {
        let mut state = self.state.lock().unwrap();
        let was_empty = state.subscribed.is_empty();
        state.subscribed.insert(uri);
        if was_empty && state.watcher.is_none() {
            if let Some(u) = universe {
                state.watcher = self.spawn_watcher(u);
            }
        }
    }

    pub fn unsubscribe(&self, uri: &str) {
        let mut state = self.state.lock().unwrap();
        state.subscribed.remove(uri);
        if state.subscribed.is_empty() {
            state.watcher = None;
        }
    }

    pub fn shutdown(&self) {
        let mut state = self.state.lock().unwrap();
        state.subscribed.clear();
        state.watcher = None;
    }

    fn spawn_watcher(&self, universe: PathBuf) -> Option<UniverseWatcher> {
        let subscribed = Arc::clone(&self.state);
        let on_notify = Arc::clone(&self.on_notify);
        let emitter: UpdateEmitter = Arc::new(move |uri: String| {
            // Emit only for URIs the client actually asked about. The watcher
            // callback can fire many times per edit — we drop the noise here.
            let guard = subscribed.lock().unwrap();
            if guard.subscribed.contains(&uri) {
                on_notify(uri);
            }
        });
        match UniverseWatcher::start(universe, emitter) {
            Ok(w) => Some(w),
            Err(e) => {
                tracing::warn!("failed to start watcher: {e}");
                None
            }
        }
    }
}
