use crate::arena::{ArenaMetrics, RequestArena};
use crate::di::{BeanRegistry, RequestContext};
use crate::dispatch::DispatchTable;
use crate::telemetry::Telemetry;
use arc_swap::ArcSwap;
use axum::{
    extract::{Query, State},
    http::{Method, StatusCode, Uri},
    response::{IntoResponse, Response},
    Router,
};
use rustjvm_compiler::EvalError;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tracing::{debug, info};

#[derive(Debug, Clone)]
pub struct RuntimeConfig {
    pub port: u16,
    /// When the process started, so we can report true cold-start latency.
    pub started_at: Instant,
}

/// Everything a request needs. Cheap to clone (all handles are `Arc`).
///
/// The bean registry sits behind an `ArcSwap` so LiveRust can rewire the
/// whole object graph atomically: in-flight requests keep the registry they
/// started with, new requests see the new one.
#[derive(Clone)]
pub struct AppState {
    pub table: Arc<DispatchTable>,
    pub metrics: Arc<ArenaMetrics>,
    pub telemetry: Arc<Telemetry>,
    pub beans: Arc<ArcSwap<BeanRegistry>>,
}

impl AppState {
    pub fn new(table: Arc<DispatchTable>) -> Self {
        Self::with_beans(table, BeanRegistry::empty())
    }

    pub fn with_beans(table: Arc<DispatchTable>, registry: BeanRegistry) -> Self {
        Self {
            table,
            metrics: ArenaMetrics::new(),
            telemetry: Telemetry::new(),
            beans: Arc::new(ArcSwap::from_pointee(registry)),
        }
    }
}

pub fn router(state: AppState) -> Router {
    Router::new().fallback(handle).with_state(state)
}

pub async fn serve(state: AppState, config: RuntimeConfig) -> std::io::Result<()> {
    let addr = format!("127.0.0.1:{}", config.port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    info!(
        "RustJVM listening on http://{addr} (cold start: {:?})",
        config.started_at.elapsed()
    );
    serve_on(listener, state).await
}

/// Serves on an already-bound listener — tests bind port 0 to get an
/// ephemeral address.
pub async fn serve_on(listener: tokio::net::TcpListener, state: AppState) -> std::io::Result<()> {
    axum::serve(listener, router(state)).await
}

async fn handle(
    State(state): State<AppState>,
    method: Method,
    uri: Uri,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    let started = Instant::now();
    let path = uri.path();

    let Some(route) = state.table.resolve(method.as_str(), path) else {
        // Built-in operational endpoints are served only when the app
        // itself didn't register the path — app routes always win.
        if path == "/health" {
            state.telemetry.record_request(method.as_str(), "/health", 200, started.elapsed().as_micros());
            return (StatusCode::OK, "ok").into_response();
        }
        if path == "/metrics" {
            let body = state.telemetry.render_prometheus(&state.metrics);
            state.telemetry.record_request(method.as_str(), "/metrics", 200, started.elapsed().as_micros());
            return (
                StatusCode::OK,
                [(axum::http::header::CONTENT_TYPE, "text/plain; version=0.0.4")],
                body,
            )
                .into_response();
        }
        state.telemetry.record_request(method.as_str(), "<unmatched>", 404, started.elapsed().as_micros());
        return (
            StatusCode::NOT_FOUND,
            format!("no route: {method} {path}\n"),
        )
            .into_response();
    };

    // Per-request arena: every allocation tied to this request is made
    // against it and reclaimed deterministically when the request ends.
    let arena = RequestArena::metered(state.metrics.clone());

    let mut args = HashMap::with_capacity(route.params.len());
    for binding in &route.params {
        match query.get(&binding.query_key) {
            Some(value) => {
                args.insert(
                    arena.alloc_str(&binding.param).to_owned(),
                    arena.alloc_str(value).to_owned(),
                );
            }
            None if binding.required => {
                return (
                    StatusCode::BAD_REQUEST,
                    format!(
                        "missing required query parameter '{}'\n",
                        binding.query_key
                    ),
                )
                    .into_response();
            }
            None => {}
        }
    }

    // Resolve the controller through a per-request context: singletons come
    // from the registry, request-scoped beans are constructed and cached
    // against this request's arena. Calls on @Autowired fields then resolve
    // through the bean graph (including Rust-native beans).
    let registry = state.beans.load_full();
    let ctx = RequestContext::new(&arena, &registry);
    let controller = ctx.resolve_by_type(&route.class_name).ok();
    let result = match &controller {
        Some(bean) => route.implementation.eval_with_ctx(&args, bean.as_ref(), 0),
        None => route.implementation.eval(&args),
    };

    let response = match result {
        // The body is staged in the arena, then handed to the HTTP layer.
        // Phase 2 removes this boundary copy with zero-copy response buffers
        // pinned to the arena's lifetime.
        Ok(body) => {
            let staged = arena.alloc_str(&body);
            (
                StatusCode::OK,
                [(axum::http::header::CONTENT_TYPE, "text/plain; charset=utf-8")],
                staged.to_owned(),
            )
                .into_response()
        }
        Err(EvalError::MissingParam(p)) => (
            StatusCode::BAD_REQUEST,
            format!("missing parameter '{p}'\n"),
        )
            .into_response(),
        Err(EvalError::Unimplemented(why)) => (
            StatusCode::NOT_IMPLEMENTED,
            format!("501 — method outside the compiler subset: {why}\n"),
        )
            .into_response(),
    };

    debug!(
        route = %format!("{}.{}", route.class_name, route.method_name),
        arena_bytes = arena.allocated_bytes(),
        latency_us = started.elapsed().as_micros(),
        "request served"
    );
    state.telemetry.record_request(
        method.as_str(),
        &route.path,
        response.status().as_u16(),
        started.elapsed().as_micros(),
    );
    response
}
