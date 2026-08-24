use rustjvm_runtime::{
    assemble_registry, serve_on, AppState, BeanRegistry, DispatchTable,
};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Minimal HTTP/1.1 client over a raw socket.
fn raw_get(port: u16, path: &str) -> (u16, String) {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .write_all(
            format!(
                "GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"
            )
            .as_bytes(),
        )
        .unwrap();
    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).unwrap();
    let text = String::from_utf8_lossy(&buf);
    let mut parts = text.splitn(2, "\r\n\r\n");
    let head = parts.next().unwrap_or("");
    let body = parts.next().unwrap_or("").to_string();
    let status = head
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(0);
    (status, body)
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

/// Scans a source tree the same way the CLI does: routes into the dispatch
/// table, per-file analysis collected for the DI assembler.
fn scan_tree(dir: &Path, table: &DispatchTable) -> Vec<(Vec<rustjvm_compiler::BeanSpec>, Vec<rustjvm_compiler::ClassDecl>)> {
    let mut files = Vec::new();
    collect(dir, table, &mut files);
    files
}

fn collect(
    dir: &Path,
    table: &DispatchTable,
    files: &mut Vec<(Vec<rustjvm_compiler::BeanSpec>, Vec<rustjvm_compiler::ClassDecl>)>,
) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            collect(&path, table, files);
        } else if path.extension().and_then(|e| e.to_str()) == Some("java") {
            let src = std::fs::read_to_string(&path).unwrap();
            let analysis = rustjvm_compiler::analyze_source(&src).unwrap();
            for route in analysis.routes {
                table.install(route);
            }
            files.push((analysis.beans, analysis.classes));
        }
    }
}

struct TestServer {
    port: u16,
    state: AppState,
    task: tokio::task::JoinHandle<()>,
}

impl TestServer {
    fn stop(self) {
        self.task.abort();
    }
}

async fn boot(dir: &str) -> TestServer {
    let table = Arc::new(DispatchTable::new());
    let files = scan_tree(&workspace_root().join(dir), &table);
    let registry = assemble_registry(&files, Vec::new()).unwrap();
    boot_with(table, registry).await
}

async fn boot_with(table: Arc<DispatchTable>, registry: BeanRegistry) -> TestServer {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let state = AppState::with_beans(table, registry);
    let task = tokio::spawn({
        let state = state.clone();
        async move {
            serve_on(listener, state).await.unwrap();
        }
    });
    TestServer { port, state, task }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn full_request_lifecycle() {
    let server = boot("examples/hello-app/src").await;

    let (status, body) = raw_get(server.port, "/hello?name=rustjvm");
    assert_eq!(status, 200);
    assert_eq!(body, "Hello, rustjvm!");

    let (status, body) = raw_get(server.port, "/ping");
    assert_eq!(status, 200);
    assert_eq!(body, "pong");

    let (status, _) = raw_get(server.port, "/missing");
    assert_eq!(status, 404);

    server.stop();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn arenas_fully_drain_after_requests() {
    let server = boot("examples/hello-app/src").await;
    for i in 0..50 {
        let (status, body) = raw_get(server.port, &format!("/hello?name=u{i}"));
        assert_eq!(status, 200);
        assert_eq!(body, format!("Hello, u{i}!"));
    }
    assert_eq!(server.state.metrics.active(), 0, "every arena dropped");
    assert_eq!(server.state.metrics.completed(), 50);
    server.stop();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn di_app_serves_through_injected_service() {
    let server = boot("examples/di-app/src").await;
    let (status, body) = raw_get(server.port, "/greet?name=world");
    assert_eq!(status, 200);
    // Controller → @Service → @Bean-produced plain class.
    assert_eq!(body, "[rust] Hello, world!");
    server.stop();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn observability_endpoints_expose_prometheus_metrics() {
    let server = boot("examples/di-app/src").await;

    // Generate some traffic first.
    raw_get(server.port, "/greet?name=metrics");
    raw_get(server.port, "/greet?name=metrics");

    // /health answers for the K8s probes.
    let (status, body) = raw_get(server.port, "/health");
    assert_eq!(status, 200);
    assert_eq!(body, "ok");

    // /metrics renders Prometheus text format with request + arena counters.
    let (status, body) = raw_get(server.port, "/metrics");
    assert_eq!(status, 200);
    assert!(
        body.contains("rustjvm_http_requests_total{method=\"GET\",path=\"/greet\",status=\"200\"} 2"),
        "request counter present: {body}"
    );
    assert!(body.contains("rustjvm_arena_active 0"), "arena gauge: {body}");
    assert!(body.contains("rustjvm_arena_completed_total"));
    assert!(body.contains("rustjvm_hot_reloads_total"));

    // App routes still win over built-ins when they define the same path:
    // the di-app has no /health, so the built-in answered above.
    server.stop();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn rust_native_bean_serves_over_http() {
    // Java sources reference a VectorStore that exists only in Rust.
    let src = r#"
@Service
public class SearchService {
    @Autowired
    private VectorStore vectorStore;

    public String find(String q) {
        return vectorStore.search(q);
    }
}

@RestController
public class SearchController {
    @Autowired
    private SearchService searchService;

    @GetMapping("/search")
    public String search(@RequestParam String q) {
        return searchService.find(q);
    }
}
"#;
    let mut methods: HashMap<String, rustjvm_runtime::NativeFn> = HashMap::new();
    methods.insert(
        "search".to_string(),
        Arc::new(|args: &[String]| Ok(format!("vector::{}", args[0]))),
    );
    let natives = vec![rustjvm_runtime::NativeDef {
        name: "vectorStore".to_string(),
        class_name: "VectorStore".to_string(),
        methods,
    }];

    let table = Arc::new(DispatchTable::new());
    let analysis = rustjvm_compiler::analyze_source(src).unwrap();
    for route in analysis.routes {
        table.install(route);
    }
    let registry = assemble_registry(&[(analysis.beans, analysis.classes)], natives).unwrap();
    let server = boot_with(table, registry).await;

    let (status, body) = raw_get(server.port, "/search?q=cats");
    assert_eq!(status, 200);
    assert_eq!(body, "vector::cats");
    server.stop();
}
