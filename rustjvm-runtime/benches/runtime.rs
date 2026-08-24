//! Performance gates tracked in CI:
//!   bench_cold_start  — compile + install the example app  (< 50ms target)
//!   bench_hot_swap    — atomic dispatch-table swap         (< 1ms guard)
//!   bench_request     — full HTTP round-trip on loopback   (p99 < 100µs target)

use criterion::{criterion_group, criterion_main, Criterion};
use rustjvm_runtime::{serve_on, AppState, DispatchTable};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

const HELLO: &str = r#"
@RestController
public class HelloController {
    @GetMapping("/hello")
    public String hello(@RequestParam String name) {
        return "Hello, " + name + "!";
    }

    @GetMapping("/ping")
    public String ping() {
        return "pong";
    }
}
"#;

fn bench_cold_start(c: &mut Criterion) {
    // Proxy for boot cost: parse + compile + install every route.
    c.bench_function("cold_start", |b| {
        b.iter(|| {
            let table = DispatchTable::new();
            for route in rustjvm_compiler::compile_source(HELLO).unwrap() {
                table.install(route);
            }
            table
        });
    });
}

fn bench_hot_swap(c: &mut Criterion) {
    let table = DispatchTable::new();
    for route in rustjvm_compiler::compile_source(HELLO).unwrap() {
        table.install(route);
    }
    let v2 = rustjvm_compiler::compile_source(
        &HELLO.replace("Hello, ", "Hiya, "),
    )
    .unwrap();

    c.bench_function("hot_swap", |b| {
        b.iter(|| {
            for route in &v2 {
                table.install(route.clone());
            }
        });
    });
}

async fn raw_get(addr: SocketAddr, path: &str) {
    let mut stream = TcpStream::connect(addr).await.unwrap();
    stream
        .write_all(
            format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
                .as_bytes(),
        )
        .await
        .unwrap();
    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).await.unwrap();
    std::hint::black_box(buf);
}

fn bench_request(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let addr = rt.block_on(async {
        let table = Arc::new(DispatchTable::new());
        for route in rustjvm_compiler::compile_source(HELLO).unwrap() {
            table.install(route);
        }
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(serve_on(listener, AppState::new(table)));
        addr
    });

    let mut group = c.benchmark_group("http");
    // Per-request latency over real TCP loopback. Criterion's distribution
    // stats (incl. upper bounds) stand in for p99 tracking in CI.
    group.bench_function("request", |b| {
        b.iter_custom(|iters| {
            let mut total = Duration::ZERO;
            for _ in 0..iters {
                let start = Instant::now();
                rt.block_on(raw_get(addr, "/hello?name=bench"));
                total += start.elapsed();
            }
            total
        });
    });
    group.finish();
}

criterion_group!(benches, bench_cold_start, bench_hot_swap, bench_request);
criterion_main!(benches);
