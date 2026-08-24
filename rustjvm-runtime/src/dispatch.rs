use arc_swap::ArcSwap;
use rustjvm_compiler::CompiledRoute;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

#[derive(Debug, Hash, PartialEq, Eq)]
struct RouteKey {
    method: String,
    path: String,
}

impl RouteKey {
    fn new(method: &str, path: &str) -> Self {
        Self {
            method: method.to_uppercase(),
            path: path.to_string(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallOutcome {
    /// First version of this route.
    Registered,
    /// An existing route was atomically pointer-swapped (LiveRust).
    Swapped,
}

/// The method dispatch table — the heart of LiveRust hot-reload.
///
/// Each route lives behind an [`ArcSwap`], so replacing a method
/// implementation is a single atomic pointer store. Requests already in
/// flight hold an `Arc` to the *old* implementation and drain naturally;
/// the old code is freed by refcount the instant the last drain finishes.
/// No stop-the-world, no restart, no state loss.
pub struct DispatchTable {
    routes: RwLock<HashMap<RouteKey, Arc<ArcSwap<CompiledRoute>>>>,
}

impl DispatchTable {
    pub fn new() -> Self {
        Self {
            routes: RwLock::new(HashMap::new()),
        }
    }

    pub fn install(&self, route: CompiledRoute) -> InstallOutcome {
        let key = RouteKey::new(&route.http_method, &route.path);
        {
            let map = self.routes.read().expect("dispatch table poisoned");
            if let Some(slot) = map.get(&key) {
                // The atomic pointer swap. Readers never block.
                slot.store(Arc::new(route));
                return InstallOutcome::Swapped;
            }
        }
        self.routes
            .write()
            .expect("dispatch table poisoned")
            .insert(key, Arc::new(ArcSwap::from_pointee(route)));
        InstallOutcome::Registered
    }

    /// Resolves a request to the current implementation. `ANY` routes (from
    /// `@RequestMapping` without a verb) match every method.
    pub fn resolve(&self, method: &str, path: &str) -> Option<Arc<CompiledRoute>> {
        let map = self.routes.read().expect("dispatch table poisoned");
        map.get(&RouteKey::new(method, path))
            .or_else(|| map.get(&RouteKey::new("ANY", path)))
            .map(|slot| slot.load_full())
    }

    pub fn route_count(&self) -> usize {
        self.routes.read().expect("dispatch table poisoned").len()
    }

    pub fn snapshot(&self) -> Vec<Arc<CompiledRoute>> {
        self.routes
            .read()
            .expect("dispatch table poisoned")
            .values()
            .map(|slot| slot.load_full())
            .collect()
    }
}

impl Default for DispatchTable {
    fn default() -> Self {
        Self::new()
    }
}
