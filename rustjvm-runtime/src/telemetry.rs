//! Built-in observability: request/arena/reload metrics rendered in
//! Prometheus text format at `/metrics`, and a liveness probe at `/health`.
//!
//! Cost on the hot path: one short mutex for the per-route counters —
//! nanoseconds, well under the 100µs opt-in guardrail.

use crate::arena::ArenaMetrics;
use std::collections::HashMap;
use std::fmt::Write as _;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct HttpKey {
    method: String,
    path: String,
    status: u16,
}

#[derive(Debug, Default)]
struct HttpStats {
    count: u64,
    micros_total: u64,
    micros_max: u64,
}

/// Process-wide telemetry handle. Cheap to clone (Arc inside).
#[derive(Debug, Default)]
pub struct Telemetry {
    http: Mutex<HashMap<HttpKey, HttpStats>>,
    hot_reloads: AtomicU64,
    hot_reload_micros_total: AtomicU64,
}

impl Telemetry {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn record_request(&self, method: &str, path: &str, status: u16, micros: u128) {
        let micros = micros.min(u64::MAX as u128) as u64;
        let key = HttpKey {
            method: method.to_string(),
            path: path.to_string(),
            status,
        };
        let mut http = self.http.lock().expect("telemetry poisoned");
        let stats = http.entry(key).or_default();
        stats.count += 1;
        stats.micros_total += micros;
        stats.micros_max = stats.micros_max.max(micros);
    }

    pub fn record_reload(&self, micros: u128) {
        self.hot_reloads.fetch_add(1, Ordering::SeqCst);
        self.hot_reload_micros_total
            .fetch_add(micros.min(u64::MAX as u128) as u64, Ordering::SeqCst);
    }

    pub fn hot_reloads(&self) -> u64 {
        self.hot_reloads.load(Ordering::SeqCst)
    }

    /// Prometheus text exposition format.
    pub fn render_prometheus(&self, arena: &ArenaMetrics) -> String {
        let mut out = String::new();

        out.push_str("# HELP rustjvm_http_requests_total Total HTTP requests served.\n");
        out.push_str("# TYPE rustjvm_http_requests_total counter\n");
        let http = self.http.lock().expect("telemetry poisoned");
        let mut rows: Vec<(&HttpKey, &HttpStats)> = http.iter().collect();
        rows.sort_by(|a, b| (&a.0.path, &a.0.method, a.0.status).cmp(&(&b.0.path, &b.0.method, b.0.status)));
        for (key, stats) in &rows {
            let _ = writeln!(
                out,
                "rustjvm_http_requests_total{{method=\"{}\",path=\"{}\",status=\"{}\"}} {}",
                key.method, key.path, key.status, stats.count
            );
        }

        out.push_str("# HELP rustjvm_http_request_duration_microseconds Request latency.\n");
        out.push_str("# TYPE rustjvm_http_request_duration_microseconds summary\n");
        for (key, stats) in &rows {
            let labels = format!("method=\"{}\",path=\"{}\"", key.method, key.path);
            let _ = writeln!(
                out,
                "rustjvm_http_request_duration_microseconds_count{{{labels}}} {}",
                stats.count
            );
            let _ = writeln!(
                out,
                "rustjvm_http_request_duration_microseconds_sum{{{labels}}} {}",
                stats.micros_total
            );
            let _ = writeln!(
                out,
                "rustjvm_http_request_duration_microseconds_max{{{labels}}} {}",
                stats.micros_max
            );
        }
        drop(http);

        out.push_str("# HELP rustjvm_arena_active Request arenas currently alive.\n");
        out.push_str("# TYPE rustjvm_arena_active gauge\n");
        let _ = writeln!(out, "rustjvm_arena_active {}", arena.active());
        out.push_str("# HELP rustjvm_arena_completed_total Requests whose arena was reclaimed.\n");
        out.push_str("# TYPE rustjvm_arena_completed_total counter\n");
        let _ = writeln!(out, "rustjvm_arena_completed_total {}", arena.completed());
        out.push_str("# HELP rustjvm_arena_bytes_total Cumulative arena-allocated bytes.\n");
        out.push_str("# TYPE rustjvm_arena_bytes_total counter\n");
        let _ = writeln!(out, "rustjvm_arena_bytes_total {}", arena.total_bytes());

        out.push_str("# HELP rustjvm_hot_reloads_total Successful LiveRust swaps.\n");
        out.push_str("# TYPE rustjvm_hot_reloads_total counter\n");
        let _ = writeln!(
            out,
            "rustjvm_hot_reloads_total {}",
            self.hot_reloads.load(Ordering::SeqCst)
        );
        let _ = writeln!(
            out,
            "rustjvm_hot_reload_duration_microseconds_sum {}",
            self.hot_reload_micros_total.load(Ordering::SeqCst)
        );

        out
    }
}
