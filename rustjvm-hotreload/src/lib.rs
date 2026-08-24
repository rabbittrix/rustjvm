//! LiveRust — hot-reload without restart.
//!
//! Watches Java sources, recompiles what changed, and atomically swaps both
//! the affected routes and — when the bean graph changed — the whole DI
//! registry. Consistency rule: if either the file fails to compile or the
//! new bean graph fails to build, nothing is swapped; the previous version
//! keeps serving.

use arc_swap::ArcSwap;
use notify::RecursiveMode;
use notify_debouncer_mini::{new_debouncer, DebounceEventResult, DebouncedEvent, Debouncer};
use rustjvm_compiler::{BeanSpec, ClassDecl};
use rustjvm_runtime::{
    assemble_registry, BeanRegistry, DispatchTable, InstallOutcome, NativeDef, Telemetry,
};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};
use tracing::{info, warn};

/// What one source file contributes, cached for registry rebuilds.
#[derive(Debug, Clone, Default)]
pub struct CachedFile {
    pub beans: Vec<BeanSpec>,
    pub classes: Vec<ClassDecl>,
}

pub struct HotReloader {
    root: PathBuf,
    table: Arc<DispatchTable>,
    beans: Arc<ArcSwap<BeanRegistry>>,
    telemetry: Arc<Telemetry>,
    /// Latest known-good scan output per source file.
    cache: Arc<RwLock<HashMap<PathBuf, CachedFile>>>,
    /// Rust-native beans survive every reload — they're re-attached to each
    /// rebuilt registry.
    natives: Arc<Vec<NativeDef>>,
}

impl HotReloader {
    pub fn new(
        root: PathBuf,
        table: Arc<DispatchTable>,
        beans: Arc<ArcSwap<BeanRegistry>>,
        telemetry: Arc<Telemetry>,
        initial: HashMap<PathBuf, CachedFile>,
        natives: Vec<NativeDef>,
    ) -> Self {
        // Normalize cache keys: watcher events and directory scans produce
        // differently-shaped paths for the same file (relative vs absolute,
        // mixed separators). Canonicalizing at the boundary keeps one cache
        // entry per file.
        let initial = initial.into_iter().map(|(p, f)| (key_for(&p), f)).collect();
        Self {
            root,
            table,
            beans,
            telemetry,
            cache: Arc::new(RwLock::new(initial)),
            natives: Arc::new(natives),
        }
    }

    /// Starts watching. The returned debouncer is a guard — keep it alive for
    /// as long as hot-reload should run.
    pub fn spawn(self) -> notify::Result<Debouncer<notify::RecommendedWatcher>> {
        let ctx = ReloadContext {
            table: self.table,
            beans: self.beans,
            telemetry: self.telemetry,
            cache: self.cache,
            natives: self.natives,
        };
        let mut debouncer = new_debouncer(
            Duration::from_millis(120),
            move |result: DebounceEventResult| match result {
                Ok(events) => handle_events(&ctx, events),
                Err(errors) => warn!("watch error: {errors:?}"),
            },
        )?;
        debouncer.watcher().watch(&self.root, RecursiveMode::Recursive)?;
        info!("LiveRust watching {}", self.root.display());
        Ok(debouncer)
    }
}

struct ReloadContext {
    table: Arc<DispatchTable>,
    beans: Arc<ArcSwap<BeanRegistry>>,
    telemetry: Arc<Telemetry>,
    cache: Arc<RwLock<HashMap<PathBuf, CachedFile>>>,
    natives: Arc<Vec<NativeDef>>,
}

fn handle_events(ctx: &ReloadContext, events: Vec<DebouncedEvent>) {
    for event in events {
        let path = &event.path;
        if path.extension().and_then(|e| e.to_str()) != Some("java") {
            continue;
        }
        reload_file(ctx, path);
    }
}

/// Canonical cache key for a source path (falls back to the raw path if the
/// file vanished between event and read).
fn key_for(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn reload_file(ctx: &ReloadContext, path: &Path) {
    let started = Instant::now();
    let key = key_for(path);
    let src = match std::fs::read_to_string(path) {
        Ok(src) => src,
        Err(e) => {
            warn!("hot-reload: cannot read {}: {e}", path.display());
            return;
        }
    };

    match rustjvm_compiler::analyze_source(&src) {
        Ok(analysis) => {
            // Build the candidate registry first. If the new bean graph is
            // broken (cycle, missing dependency), swap NOTHING — routes and
            // registry stay consistent on the previous version.
            let (candidate, bases) = {
                let mut cache = ctx.cache.write().expect("scan cache poisoned");
                let previous = cache.insert(
                    key.clone(),
                    CachedFile {
                        beans: analysis.beans,
                        classes: analysis.classes,
                    },
                );
                let files: Vec<(Vec<BeanSpec>, Vec<ClassDecl>)> = cache
                    .values()
                    .map(|f| (f.beans.clone(), f.classes.clone()))
                    .collect();
                let natives = clone_natives(&ctx.natives);
                match assemble_registry(&files, natives) {
                    Ok(registry) => {
                        let bases = rustjvm_compiler::scan_base_packages(
                            &files.iter().flat_map(|(b, _)| b.clone()).collect::<Vec<_>>(),
                        );
                        (registry, bases)
                    }
                    Err(e) => {
                        // Roll the cache back to the last known-good state.
                        match previous {
                            Some(old) => {
                                cache.insert(key.clone(), old);
                            }
                            None => {
                                cache.remove(&key);
                            }
                        }
                        warn!(
                            "hot-reload: keeping previous version of {} (bean graph: {e})",
                            path.display()
                        );
                        return;
                    }
                }
            };

            let mut swapped = 0usize;
            let mut registered = 0usize;
            for route in analysis.routes {
                // Respect the @ComponentScan filter, same as boot.
                if !rustjvm_compiler::under_scan_bases(&route.package, &bases) {
                    continue;
                }
                match ctx.table.install(route) {
                    InstallOutcome::Swapped => swapped += 1,
                    InstallOutcome::Registered => registered += 1,
                }
            }
            ctx.beans.store(Arc::new(candidate));
            let elapsed = started.elapsed();
            ctx.telemetry.record_reload(elapsed.as_micros());
            info!(
                "LiveRust swap: {} — {swapped} route(s) swapped, {registered} new, registry rewired in {:?}",
                path.display(),
                elapsed
            );
        }
        // State preservation: a broken edit keeps the previous version live.
        Err(e) => warn!(
            "hot-reload: keeping previous version of {} ({e})",
            path.display()
        ),
    }
}

/// Native method tables are `Arc`-shared closures — cheap to clone.
fn clone_natives(natives: &[NativeDef]) -> Vec<NativeDef> {
    natives
        .iter()
        .map(|n| NativeDef {
            name: n.name.clone(),
            class_name: n.class_name.clone(),
            methods: n.methods.clone(),
        })
        .collect()
}
