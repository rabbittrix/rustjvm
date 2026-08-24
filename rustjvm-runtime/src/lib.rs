//! rustjvm-runtime — the Rust core: HTTP server, per-request arenas, the
//! atomically-swapped method dispatch table, and the DI container.
//! Zero GC, async-first, hot-swappable.

pub mod arena;
pub mod di;
pub mod dispatch;
pub mod server;
pub mod telemetry;

pub use arena::{ArenaMetrics, RequestArena};
pub use di::{assemble_registry, Bean, BeanRegistry, DIError, NativeDef, NativeFn, RequestContext};
pub use dispatch::{DispatchTable, InstallOutcome};
pub use server::{router, serve, serve_on, AppState, RuntimeConfig};
pub use telemetry::Telemetry;
