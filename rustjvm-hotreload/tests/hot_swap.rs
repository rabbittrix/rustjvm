use rustjvm_hotreload::{CachedFile, HotReloader};
use rustjvm_runtime::{assemble_registry, serve_on, AppState, DispatchTable};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

const HELLO_V1: &str = r#"
import rustjvm.spring.web.GetMapping;
import rustjvm.spring.web.RequestParam;
import rustjvm.spring.web.RestController;

@RestController
public class HelloController {

    @GetMapping("/hello")
    public String hello(@RequestParam String name) {
        return "Hello, " + name + "!";
    }
}
"#;

const HELLO_V2: &str = r#"
import rustjvm.spring.web.GetMapping;
import rustjvm.spring.web.RequestParam;
import rustjvm.spring.web.RestController;

@RestController
public class HelloController {

    @GetMapping("/hello")
    public String hello(@RequestParam String name) {
        return "Hi, " + name + "!";
    }
}
"#;

const HELLO_BROKEN: &str = "public class HelloController { {{{ not java";

const DI_SERVICE_V1: &str = r#"
import rustjvm.spring.context.Service;

@Service
public class GreetingService {
    public String greet(String name) {
        return "Hello, " + name + "!";
    }
}
"#;

const DI_SERVICE_V2: &str = r#"
import rustjvm.spring.context.Service;

@Service
public class GreetingService {
    public String greet(String name) {
        return "Howdy, " + name + "!";
    }
}
"#;

const DI_CONTROLLER: &str = r#"
import rustjvm.spring.context.Autowired;
import rustjvm.spring.web.GetMapping;
import rustjvm.spring.web.RequestParam;
import rustjvm.spring.web.RestController;

@RestController
public class GreetController {

    @Autowired
    private GreetingService greetingService;

    @GetMapping("/greet")
    public String greet(@RequestParam String name) {
        return greetingService.greet(name);
    }
}
"#;

/// Service references a bean that doesn't exist — valid Java, broken graph.
const DI_SERVICE_BROKEN_GRAPH: &str = r#"
import rustjvm.spring.context.Autowired;
import rustjvm.spring.context.Service;

@Service
public class GreetingService {
    @Autowired
    private MissingThing missing;

    public String greet(String name) {
        return "broken";
    }
}
"#;

fn raw_get(port: u16, path: &str) -> (u16, String) {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .write_all(
            format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n")
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

struct TestApp {
    _tmp: tempfile::TempDir,
    src: PathBuf,
    port: u16,
    state: AppState,
    task: tokio::task::JoinHandle<()>,
    _watcher: notify_debouncer_mini::Debouncer<notify::RecommendedWatcher>,
}

impl TestApp {
    async fn start(files: &[(&str, &str)]) -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src");
        std::fs::create_dir_all(&src).unwrap();

        let table = Arc::new(DispatchTable::new());
        let mut cache = HashMap::new();
        for (name, contents) in files {
            let path = src.join(name);
            std::fs::write(&path, contents).unwrap();
            let analysis =
                rustjvm_compiler::analyze_source(contents).expect("initial source compiles");
            for route in analysis.routes {
                table.install(route);
            }
            cache.insert(
                path.clone(),
                CachedFile {
                    beans: analysis.beans,
                    classes: analysis.classes,
                },
            );
        }
        let files_vec: Vec<(Vec<_>, Vec<_>)> = cache
            .values()
            .map(|f| (f.beans.clone(), f.classes.clone()))
            .collect();
        let registry = assemble_registry(&files_vec, Vec::new()).unwrap();
        let state = AppState::with_beans(table.clone(), registry);

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let task = tokio::spawn({
            let state = state.clone();
            async move {
                serve_on(listener, state).await.unwrap();
            }
        });

        let watcher = HotReloader::new(
            src.clone(),
            table,
            state.beans.clone(),
            state.telemetry.clone(),
            cache,
            Vec::new(),
        )
        .spawn()
        .unwrap();
        TestApp {
            _tmp: tmp,
            src,
            port,
            state,
            task,
            _watcher: watcher,
        }
    }

    fn stop(self) {
        self.task.abort();
    }

    fn patch_file(&self, name: &str, contents: &str) {
        std::fs::write(self.src.join(name), contents).unwrap();
    }

    async fn wait_for_body(&self, path: &str, expected: &str) -> String {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let (_, body) = raw_get(self.port, path);
            if body == expected {
                return body;
            }
            assert!(
                Instant::now() < deadline,
                "timed out waiting for {path} to become {expected:?}; last body was {body:?}"
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hot_swap_mid_test() {
    let app = TestApp::start(&[("HelloController.java", HELLO_V1)]).await;
    assert_eq!(raw_get(app.port, "/hello?name=a").1, "Hello, a!");

    app.patch_file("HelloController.java", HELLO_V2);
    app.wait_for_body("/hello?name=a", "Hi, a!").await;

    assert_eq!(app.state.metrics.active(), 0, "arenas drained after swaps");
    app.stop();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn broken_edit_keeps_previous_version() {
    let app = TestApp::start(&[("HelloController.java", HELLO_V1)]).await;
    assert_eq!(raw_get(app.port, "/hello?name=a").1, "Hello, a!");

    app.patch_file("HelloController.java", HELLO_BROKEN);
    tokio::time::sleep(Duration::from_millis(500)).await;

    let (status, body) = raw_get(app.port, "/hello?name=a");
    assert_eq!(status, 200);
    assert_eq!(body, "Hello, a!", "broken edit must not swap anything");

    app.patch_file("HelloController.java", HELLO_V2);
    app.wait_for_body("/hello?name=a", "Hi, a!").await;
    app.stop();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hot_reload_preserves_autowired_dependencies() {
    let app = TestApp::start(&[
        ("GreetingService.java", DI_SERVICE_V1),
        ("GreetController.java", DI_CONTROLLER),
    ])
    .await;
    assert_eq!(raw_get(app.port, "/greet?name=b").1, "Hello, b!");

    // Swap the @Service mid-flight: the controller must be rewired to the
    // new bean and keep answering through the injected field.
    app.patch_file("GreetingService.java", DI_SERVICE_V2);
    app.wait_for_body("/greet?name=b", "Howdy, b!").await;

    // In-flight drain: old registry Arcs are still held by the handler
    // snapshot; new requests see the new graph. No request ever 500s.
    app.stop();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn broken_bean_graph_keeps_previous_version() {
    let app = TestApp::start(&[
        ("GreetingService.java", DI_SERVICE_V1),
        ("GreetController.java", DI_CONTROLLER),
    ])
    .await;
    assert_eq!(raw_get(app.port, "/greet?name=b").1, "Hello, b!");

    // Introduce an unsatisfiable dependency: compile succeeds, wiring fails.
    // Both routes AND registry must stay on the previous version.
    app.patch_file("GreetingService.java", DI_SERVICE_BROKEN_GRAPH);
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(
        raw_get(app.port, "/greet?name=b").1,
        "Hello, b!",
        "broken graph must not swap routes or registry"
    );

    // Recovery works.
    app.patch_file("GreetingService.java", DI_SERVICE_V2);
    app.wait_for_body("/greet?name=b", "Howdy, b!").await;
    app.stop();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hot_reload_under_load() {
    let app = TestApp::start(&[("HelloController.java", HELLO_V1)]).await;
    assert_eq!(raw_get(app.port, "/hello?name=x").1, "Hello, x!");

    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let mut workers = Vec::new();
    for _ in 0..8 {
        let stop = stop.clone();
        let port = app.port;
        workers.push(tokio::task::spawn_blocking(move || {
            let mut accepted = 0usize;
            while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                let (status, body) = raw_get(port, "/hello?name=x");
                assert_eq!(status, 200);
                assert!(
                    body == "Hello, x!" || body == "Hi, x!",
                    "mid-swap response must be a complete old or new version, got {body:?}"
                );
                accepted += 1;
            }
            accepted
        }));
    }

    tokio::time::sleep(Duration::from_millis(150)).await;
    app.patch_file("HelloController.java", HELLO_V2);
    app.wait_for_body("/hello?name=x", "Hi, x!").await;
    stop.store(true, std::sync::atomic::Ordering::Relaxed);

    let mut total = 0usize;
    for w in workers {
        total += w.await.unwrap();
    }
    assert!(total > 100, "expected sustained load during swap, got {total}");

    assert_eq!(app.state.metrics.active(), 0, "arenas drained under load");
    app.stop();
}
