use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use rustjvm_compiler::{scan_base_packages, under_scan_bases};
use rustjvm_hotreload::{CachedFile, HotReloader};
use rustjvm_runtime::{assemble_registry, serve, AppState, DispatchTable, RuntimeConfig};
use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use tracing::{info, warn};

#[derive(Parser)]
#[command(
    name = "rustjvm",
    version,
    about = "RustJVM — Java's soul, Rust's muscle, zero compromises."
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Scaffold a new RustJVM application (sources, pom.xml, rustjvm.toml).
    New {
        /// Project directory to create.
        name: PathBuf,
        /// Java package for the generated sources.
        #[arg(long, default_value = "com.example")]
        package: String,
    },
    /// Compile a Java source tree and serve it with hot-reload enabled.
    Run {
        /// Root of the Java source tree to serve.
        #[arg(long, default_value = "examples/hello-app/src")]
        src: PathBuf,
        /// Port to listen on (env: RUSTJVM_PORT).
        #[arg(long)]
        port: Option<u16>,
        /// Fully-qualified @RustJVMApplication class. Restricts component
        /// scanning to that class's package (Spring Boot semantics).
        #[arg(long)]
        main: Option<String>,
    },
    /// List the routes a Java source tree would register, then exit.
    Routes {
        #[arg(long, default_value = "examples/hello-app/src")]
        src: PathBuf,
    },
    /// Validate the whole tree (routes compile, DI graph wires) and write a
    /// build manifest to target/rustjvm/. The CI/ packaging gate.
    Build {
        #[arg(long, default_value = "examples/hello-app/src")]
        src: PathBuf,
        /// Output directory for the build manifest.
        #[arg(long, default_value = "target/rustjvm")]
        output: PathBuf,
    },
    /// Liveness probe used inside containers: exits 0 if the local runtime
    /// answers GET /health, 1 otherwise.
    Healthcheck {
        #[arg(long, default_value_t = 8080)]
        port: u16,
    },
    /// Migrate a Spring Boot project to RustJVM.
    Migrate {
        #[command(subcommand)]
        action: MigrateAction,
    },
    /// Kubernetes manifest generation.
    K8s {
        #[command(subcommand)]
        action: K8sAction,
    },
    /// Generate API documentation from source metadata.
    Docs {
        #[command(subcommand)]
        action: DocsAction,
    },
}

#[derive(Subcommand)]
enum MigrateAction {
    /// Scan a Spring Boot project and print a compatibility report.
    Analyze {
        /// Path to the Spring Boot project root.
        #[arg(long)]
        from: PathBuf,
    },
    /// Write a converted RustJVM copy of the project.
    Convert {
        /// Path to the Spring Boot project root.
        #[arg(long)]
        from: PathBuf,
        /// Output directory for the migrated project.
        #[arg(long)]
        to: PathBuf,
    },
}

#[derive(Subcommand)]
enum K8sAction {
    /// Generate a Kubernetes manifest bundle (CRD + app + Deployment +
    /// Service + ConfigMap) and print it to stdout or --output.
    Generate {
        /// Application name.
        #[arg(long)]
        app: String,
        /// Container image to deploy.
        #[arg(long)]
        image: String,
        /// Port the app listens on.
        #[arg(long, default_value_t = 8080)]
        port: u16,
        /// Replica count.
        #[arg(long, default_value_t = 3)]
        replicas: u32,
        /// Write the bundle to a file instead of stdout.
        #[arg(long)]
        output: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
enum DocsAction {
    /// Generate OpenAPI JSON and a bean dependency graph.
    Generate {
        /// Root of the Java source tree.
        #[arg(long, default_value = "examples/hello-app/src")]
        src: PathBuf,
        /// Output directory (created if missing).
        #[arg(long, default_value = "docs")]
        output: PathBuf,
        /// API title for the OpenAPI document.
        #[arg(long, default_value = "rustjvm-app")]
        title: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let started_at = Instant::now();
    let log_filter = tracing_subscriber::EnvFilter::try_from_env("RUSTJVM_LOG")
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    tracing_subscriber::fmt()
        .with_target(false)
        .with_env_filter(log_filter)
        .init();

    let cli = Cli::parse();
    match cli.command {
        Commands::New { name, package } => scaffold_new(&name, &package),
        Commands::Run { src, port, main } => {
            run_server(src, resolve_port(port), main, started_at).await
        }
        Commands::Routes { src } => {
            let scan = scan_tree(&src)
                .with_context(|| format!("cannot scan {}", src.display()))?;
            print_routes(&scan.routes);
            Ok(())
        }
        Commands::Migrate { action } => migrate(action),
        Commands::K8s { action } => k8s(action),
        Commands::Docs { action } => docs(action),
        Commands::Build { src, output } => build(src, output),
        Commands::Healthcheck { port } => healthcheck(port),
    }
}

fn build(src: PathBuf, output: PathBuf) -> Result<()> {
    let scan = scan_tree(&src).with_context(|| format!("cannot scan {}", src.display()))?;
    let files: Vec<(Vec<_>, Vec<_>)> = scan
        .files
        .values()
        .map(|f| (f.beans.clone(), f.classes.clone()))
        .collect();
    let registry = assemble_registry(&files, Vec::new())
        .map_err(|e| anyhow::anyhow!("dependency injection failed: {e}"))?;

    std::fs::create_dir_all(&output)
        .with_context(|| format!("cannot create {}", output.display()))?;
    let manifest = serde_json::json!({
        "routes": scan.routes.iter().map(|r| serde_json::json!({
            "method": r.http_method,
            "path": r.path,
            "handler": format!("{}.{}", r.class_name, r.method_name),
        })).collect::<Vec<_>>(),
        "beans": files.iter().flat_map(|(beans, _)| beans.iter()).map(|b| serde_json::json!({
            "name": b.name,
            "type": b.class_name,
            "kind": format!("{:?}", b.kind),
            "scope": format!("{:?}", b.scope),
        })).collect::<Vec<_>>(),
    });
    let manifest_path = output.join("manifest.json");
    std::fs::write(&manifest_path, serde_json::to_string_pretty(&manifest)?)?;

    println!(
        "BUILD OK — {} route(s), {} bean(s) wired. Manifest: {}",
        scan.routes.len(),
        registry.len(),
        manifest_path.display()
    );
    Ok(())
}

/// Port resolution order: --port flag, then $RUSTJVM_PORT, then 8080.
fn resolve_port(flag: Option<u16>) -> u16 {
    flag.or_else(|| {
        std::env::var("RUSTJVM_PORT")
            .ok()
            .and_then(|v| v.parse().ok())
    })
    .unwrap_or(8080)
}

/// Synchronous by design: runs inside minimal containers with no runtime.
fn healthcheck(port: u16) -> Result<()> {
    use std::io::{Read, Write};
    let mut stream = std::net::TcpStream::connect(("127.0.0.1", port))
        .with_context(|| format!("cannot connect to 127.0.0.1:{port}"))?;
    stream
        .write_all(b"GET /health HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")?;
    let mut buf = String::new();
    stream.read_to_string(&mut buf)?;
    if buf.starts_with("HTTP/1.1 200") {
        std::process::exit(0);
    }
    std::process::exit(1);
}

async fn run_server(
    src: PathBuf,
    port: u16,
    main: Option<String>,
    started_at: Instant,
) -> Result<()> {
    let table = Arc::new(DispatchTable::new());
    let scan = scan_tree(&src).with_context(|| format!("cannot scan {}", src.display()))?;

    // Install routes, honoring the @ComponentScan filter — or the package of
    // the --main application class, which overrides discovered scan bases.
    let beans: Vec<_> = scan.files.values().flat_map(|f| f.beans.clone()).collect();
    let bases = match &main {
        Some(fqcn) => Some(main_scan_base(fqcn, &scan)),
        None => scan_base_packages(&beans),
    };
    let mut installed = Vec::new();
    for route in scan.routes {
        if under_scan_bases(&route.package, &bases) {
            installed.push(route);
        }
    }
    print_routes(&installed);
    let route_count = installed.len();
    for route in installed {
        table.install(route);
    }
    info!("{route_count} route(s) installed from {}", src.display());

    // Boot-time DI: fail fast on cycles, missing beans, or
    // singleton → request scope violations.
    let files: Vec<(Vec<_>, Vec<_>)> = scan
        .files
        .values()
        .map(|f| (f.beans.clone(), f.classes.clone()))
        .collect();
    let registry = assemble_registry(&files, Vec::new())
        .map_err(|e| anyhow::anyhow!("dependency injection failed: {e}"))?;
    info!("{} bean(s) wired", registry.len());
    let state = AppState::with_beans(table.clone(), registry);

    // Guard: dropping this stops the watcher.
    let _watcher = HotReloader::new(
        src.clone(),
        table,
        state.beans.clone(),
        state.telemetry.clone(),
        scan.files,
        Vec::new(),
    )
    .spawn()?;

    serve(state, RuntimeConfig { port, started_at })
        .await
        .with_context(|| {
            format!(
                "cannot bind port {port} — is another rustjvm instance already running? \
                 (stop it, or pass --port <other>)"
            )
        })
}

fn migrate(action: MigrateAction) -> Result<()> {
    match action {
        MigrateAction::Analyze { from } => {
            let report = rustjvm_migrate::SpringBootAnalyzer::analyze(&from);
            print!("{}", report.to_markdown());
            if let Some(toml) = report.generated.iter().find(|g| g.path == Path::new("rustjvm.toml")) {
                println!("\n## rustjvm.toml (preview)\n\n```toml\n{}```", toml.contents);
            }
            Ok(())
        }
        MigrateAction::Convert { from, to } => {
            let report = rustjvm_migrate::convert_project(&from, &to)
                .with_context(|| format!("cannot convert {}", from.display()))?;
            let files_touched: std::collections::HashSet<_> =
                report.conversions.iter().map(|c| &c.file).collect();
            println!(
                "Applied {} change(s) across {} file(s). Report: {}",
                report.conversions.len(),
                files_touched.len(),
                to.join("README-MIGRATION.md").display()
            );
            print!("\n{}", report.to_markdown());
            Ok(())
        }
    }
}

fn k8s(action: K8sAction) -> Result<()> {
    match action {
        K8sAction::Generate {
            app,
            image,
            port,
            replicas,
            output,
        } => {
            let mut spec = rustjvm_k8s::AppSpec::new(app, image);
            spec.port = port;
            spec.replicas = replicas;
            let yaml = rustjvm_k8s::generate_manifests(&spec);
            match output {
                Some(path) => {
                    std::fs::write(&path, &yaml)
                        .with_context(|| format!("cannot write {}", path.display()))?;
                    println!("Wrote {}", path.display());
                    println!("Apply with: kubectl apply -f {}", path.display());
                }
                None => println!("{yaml}"),
            }
            Ok(())
        }
    }
}

fn docs(action: DocsAction) -> Result<()> {
    match action {
        DocsAction::Generate { src, output, title } => {
            let scan = scan_tree(&src)
                .with_context(|| format!("cannot scan {}", src.display()))?;
            std::fs::create_dir_all(&output)
                .with_context(|| format!("cannot create {}", output.display()))?;

            let openapi = rustjvm_docs::generate_openapi(&title, &scan.routes);
            let openapi_path = output.join("openapi.json");
            std::fs::write(&openapi_path, openapi)?;

            let all_beans: Vec<_> = scan
                .files
                .values()
                .flat_map(|f| f.beans.clone())
                .collect();
            let graph = rustjvm_docs::generate_bean_graph(&all_beans);
            let graph_path = output.join("bean-graph.md");
            std::fs::write(&graph_path, graph)?;

            println!("Wrote {}", openapi_path.display());
            println!("Wrote {}", graph_path.display());
            Ok(())
        }
    }
}

struct Scan {
    routes: Vec<rustjvm_compiler::CompiledRoute>,
    /// Per-file bean specs + class declarations, keyed for the reloader.
    files: HashMap<PathBuf, CachedFile>,
}

/// Recursively analyzes every `.java` file under `dir`. A broken file is
/// skipped with a warning — one bad controller must never keep the rest of
/// the application down.
fn scan_tree(dir: &Path) -> Result<Scan> {
    let mut scan = Scan {
        routes: Vec::new(),
        files: HashMap::new(),
    };
    scan_tree_into(dir, &mut scan)?;
    Ok(scan)
}

fn scan_tree_into(dir: &Path, scan: &mut Scan) -> Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            scan_tree_into(&path, scan)?;
        } else if path.extension().and_then(|e| e.to_str()) == Some("java") {
            match std::fs::read_to_string(&path) {
                Ok(src) => match rustjvm_compiler::analyze_source(&src) {
                    Ok(analysis) => {
                        scan.routes.extend(analysis.routes);
                        scan.files.insert(
                            path.clone(),
                            CachedFile {
                                beans: analysis.beans,
                                classes: analysis.classes,
                            },
                        );
                    }
                    Err(e) => warn!("skipping {}: {e}", path.display()),
                },
                Err(e) => warn!("cannot read {}: {e}", path.display()),
            }
        }
    }
    Ok(())
}

/// Creates a runnable hello-world project: Java sources, a consumer pom.xml,
/// and a rustjvm.toml. The generated app boots with `rustjvm run --src <dir>/src`.
fn scaffold_new(dir: &Path, package: &str) -> Result<()> {
    if !package.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_')
        || package.split('.').any(str::is_empty)
    {
        anyhow::bail!("invalid Java package name: '{package}'");
    }
    if dir.exists() && dir.read_dir()?.next().is_some() {
        anyhow::bail!("{} already exists and is not empty", dir.display());
    }
    let src = dir.join("src");
    std::fs::create_dir_all(&src).with_context(|| format!("cannot create {}", src.display()))?;

    let write = |name: &str, template: &str| -> Result<()> {
        let path = dir.join(name);
        std::fs::write(&path, template.replace("{package}", package))
            .with_context(|| format!("cannot write {}", path.display()))
    };

    write(
        "src/HelloService.java",
        r#"package {package};

import rustjvm.spring.context.Service;

@Service
public class HelloService {
    public String greet(String name) {
        return "Hello, " + name + "!";
    }
}
"#,
    )?;
    write(
        "src/HelloController.java",
        r#"package {package};

import rustjvm.spring.context.Autowired;
import rustjvm.spring.web.GetMapping;
import rustjvm.spring.web.RequestParam;
import rustjvm.spring.web.RestController;

@RestController
public class HelloController {

    @Autowired
    private HelloService helloService;

    @GetMapping("/hello")
    public String hello(@RequestParam String name) {
        return helloService.greet(name);
    }
}
"#,
    )?;
    write(
        "src/App.java",
        r#"package {package};

import rustjvm.RustJVMBootstrap;
import rustjvm.spring.RustJVMApplication;

@RustJVMApplication
public class App {
    public static void main(String[] args) {
        System.exit(RustJVMBootstrap.run(App.class, args));
    }
}
"#,
    )?;
    write(
        "rustjvm.toml",
        r#"[server]
port = 8080

[reload]
watch = true

[logging]
level = "info"
"#,
    )?;
    write(
        "pom.xml",
        r#"<?xml version="1.0" encoding="UTF-8"?>
<project xmlns="http://maven.apache.org/POM/4.0.0"
         xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"
         xsi:schemaLocation="http://maven.apache.org/POM/4.0.0
                             http://maven.apache.org/xsd/maven-4.0.0.xsd">
    <modelVersion>4.0.0</modelVersion>

    <groupId>{package}</groupId>
    <artifactId>app</artifactId>
    <version>0.1.0-SNAPSHOT</version>
    <packaging>jar</packaging>

    <properties>
        <maven.compiler.release>21</maven.compiler.release>
        <project.build.sourceEncoding>UTF-8</project.build.sourceEncoding>
    </properties>

    <dependencies>
        <!-- Compile-time only: the Rust runtime provides the implementations. -->
        <dependency>
            <groupId>io.rustjvm</groupId>
            <artifactId>rustjvm-spring-api</artifactId>
            <version>0.1.0-alpha</version>
        </dependency>
    </dependencies>
</project>
"#,
    )?;
    write(
        "README.md",
        r#"# {package}

Scaffolded by `rustjvm new`.

```bash
rustjvm run --src src --port 8080
curl "http://127.0.0.1:8080/hello?name=world"
```

To compile against the API in your IDE, install the Java API JAR once:
`mvn -f <rustjvm-repo>/rustjvm-spring-api/pom.xml clean install`, then
`mvn -f pom.xml compile` here.
"#,
    )?;

    println!("Created {}", dir.display());
    println!("  src/App.java, src/HelloController.java, src/HelloService.java");
    println!("  pom.xml, rustjvm.toml, README.md");
    println!("\nRun it:");
    println!("  rustjvm run --src {}", src.display());
    Ok(())
}

/// Derives the component-scan base from a --main fully-qualified class name.
/// Warns (but still serves) when the tree contains nothing under that
/// package — a typo'd main class must not be a silent empty server.
fn main_scan_base(fqcn: &str, scan: &Scan) -> Vec<String> {
    let (package, simple_name) = match fqcn.rsplit_once('.') {
        Some((pkg, name)) => (pkg.to_string(), name),
        None => (String::new(), fqcn),
    };

    let class_seen = scan
        .files
        .values()
        .flat_map(|f| f.classes.iter())
        .any(|c| c.name == simple_name);
    if !class_seen {
        warn!("--main {fqcn}: no class named '{simple_name}' found under the source root");
    }

    let package_seen = scan
        .routes
        .iter()
        .filter_map(|r| r.package.as_deref())
        .chain(
            scan.files
                .values()
                .flat_map(|f| f.beans.iter())
                .filter_map(|b| b.package.as_deref()),
        )
        .any(|p| p == package);
    if !package_seen {
        warn!("--main {fqcn}: no scanned routes or beans in package '{package}'");
    }

    info!("--main {fqcn}: component scanning restricted to '{package}'");
    vec![package]
}

fn print_routes(routes: &[rustjvm_compiler::CompiledRoute]) {
    let mut routes: Vec<_> = routes.to_vec();
    routes.sort_by(|a, b| a.path.cmp(&b.path));
    for r in routes {
        println!(
            "  {:<6} {:<24} -> {}.{}",
            r.http_method, r.path, r.class_name, r.method_name
        );
    }
}
