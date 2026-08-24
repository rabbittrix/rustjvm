use bumpalo::Bump;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;

/// Process-wide arena statistics, used to prove the memory contract:
/// arenas are created per request and fully reclaimed at request end.
#[derive(Debug, Default)]
pub struct ArenaMetrics {
    /// Arenas currently alive. Must return to 0 once requests drain.
    active: AtomicUsize,
    /// Requests whose arena has been dropped.
    completed: AtomicU64,
    /// Cumulative bytes ever arena-allocated.
    total_bytes: AtomicU64,
}

impl ArenaMetrics {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn active(&self) -> usize {
        self.active.load(Ordering::SeqCst)
    }

    pub fn completed(&self) -> u64 {
        self.completed.load(Ordering::SeqCst)
    }

    pub fn total_bytes(&self) -> u64 {
        self.total_bytes.load(Ordering::SeqCst)
    }
}

/// Per-request bump arena.
///
/// Every allocation tied to a request lives here and dies here: when the
/// request completes the arena is dropped in one shot — deterministic,
/// GC-free memory reclamation. There is no sweep, no pause, no promotion.
pub struct RequestArena {
    bump: Bump,
    metrics: Option<Arc<ArenaMetrics>>,
}

impl RequestArena {
    pub fn new() -> Self {
        Self {
            bump: Bump::new(),
            metrics: None,
        }
    }

    /// An arena that reports its lifecycle into the shared metrics.
    pub fn metered(metrics: Arc<ArenaMetrics>) -> Self {
        metrics.active.fetch_add(1, Ordering::SeqCst);
        Self {
            bump: Bump::new(),
            metrics: Some(metrics),
        }
    }

    pub fn alloc_str(&self, s: &str) -> &str {
        self.bump.alloc_str(s)
    }

    /// Total bytes allocated in this arena so far. Surfaced per-request as a
    /// metric so we can hold the "< 10KB overhead per request" line.
    pub fn allocated_bytes(&self) -> usize {
        self.bump.allocated_bytes()
    }

    /// Rewinds the arena for reuse (future arena-pool optimization).
    #[allow(dead_code)]
    pub fn reset(&mut self) {
        self.bump.reset();
    }
}

impl Drop for RequestArena {
    fn drop(&mut self) {
        if let Some(m) = &self.metrics {
            m.total_bytes
                .fetch_add(self.bump.allocated_bytes() as u64, Ordering::SeqCst);
            m.completed.fetch_add(1, Ordering::SeqCst);
            m.active.fetch_sub(1, Ordering::SeqCst);
        }
    }
}

impl Default for RequestArena {
    fn default() -> Self {
        Self::new()
    }
}
