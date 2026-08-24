# RustJVM

**Java's soul. Rust's muscle. Zero compromises.**

RustJVM is a next-generation application runtime and framework for Java. It replaces the traditional JVM with a Rust-powered core that eliminates garbage collection pauses, cold-start latency, and memory bloat — while preserving the Spring Boot developer experience you love.

- ⚡ **Sub-5ms cold start** (vs. 3–5s for Spring Boot)
- 🔥 **Sub-1ms hot reload** with state preservation (no JVM restart)
- 🧠 **Zero-GC deterministic memory** via Rust ownership + arena allocators
- 🤖 **AI-native**: Built-in RAG, LLM streaming, and self-healing error correction
- 🦀 **Rust core, ☕ Java surface**: You write Java. Rust makes it fast.

---

## Table of Contents

- [Installation](#installation)
  - [Quick Install (Recommended)](#quick-install-recommended)
  - [Maven](#maven)
  - [Gradle](#gradle)
  - [Manual JAR](#manual-jar)
  - [Rust Runtime](#rust-runtime)
- [Quick Start](#quick-start)
- [Core Concepts](#core-concepts)
  - [Dependency Injection](#dependency-injection)
  - [Hot Reload](#hot-reload)
  - [Memory Model](#memory-model)
- [Annotations Reference](#annotations-reference)
- [AI & RAG Integration](#ai--rag-integration)
- [Configuration](#configuration)
- [Building & Running](#building--running)
- [Ecosystem & Tooling](#ecosystem--tooling)
- [Testing](#testing)
- [Architecture](#architecture)
- [Roadmap](#roadmap)
- [Contributing](#contributing)
- [License](#license)

---

## Installation

RustJVM has two parts:

1. **The Java API JAR** — annotations and interfaces you compile against.
2. **The Rust Runtime** — the native binary that executes your app.

**The JAR is for compilation. The runtime is for execution.** Your IDE and `javac` need the annotations on the classpath; at runtime the Rust engine reads your *source* and provides every implementation. The JAR's classes are never loaded by a JVM, because there is no JVM.

### Quick Install (Recommended)

```bash
# 1. Install the Rust runtime (one-time)
curl -fsSL https://rustjvm.dev/install.sh | bash

# Windows PowerShell:
#   irm https://rustjvm.dev/install.ps1 | iex

# 2. Add the JAR to your project (see Maven/Gradle below)
```

### Maven

Add the dependency to your `pom.xml`:

```xml
<dependency>
    <groupId>io.rustjvm</groupId>
    <artifactId>rustjvm-spring-api</artifactId>
    <version>0.1.0-alpha</version>
</dependency>
```

### Gradle

Add to your `build.gradle` or `build.gradle.kts`:

```kotlin
dependencies {
    implementation("io.rustjvm:rustjvm-spring-api:0.1.0-alpha")
}
```

> **Pre-release note:** `0.1.0-alpha` is published to Maven Central on tag push via
> `.github/workflows/publish-jar.yml`. Until the first tag, install it into your
> local repository once:
>
> ```bash
> mvn -f rustjvm-spring-api/pom.xml clean install   # or: scripts/build-jar.sh
> ```

### Manual JAR

Once published, download the JAR directly:

```bash
wget https://repo1.maven.org/maven2/io/rustjvm/rustjvm-spring-api/0.1.0-alpha/rustjvm-spring-api-0.1.0-alpha.jar
```

Then add it to your classpath:

```bash
javac -cp rustjvm-spring-api-0.1.0-alpha.jar MyApp.java
```

### Rust Runtime

The JAR provides annotations and APIs, but you need the Rust runtime to execute.

**Option A: Cargo (recommended for developers)**

```bash
cargo install rustjvm-cli     # after crates.io publish
cargo install --path rustjvm-cli   # from a source checkout, today
rustjvm --version
```

**Option B: One-line installer**

```bash
# Linux / macOS
curl -fsSL https://rustjvm.dev/install.sh | bash

# Windows PowerShell
irm https://rustjvm.dev/install.ps1 | iex
```

The installers fetch the prebuilt binary from GitHub Releases (published with the first `v*` tag) and add `~/.rustjvm/bin` to your `PATH`.

**Option C: Build from source**

```bash
git clone https://github.com/rustjvm/rustjvm.git
cd rustjvm
cargo install --path rustjvm-cli
```

Verify:

```bash
rustjvm --version
# rustjvm 0.1.0
```

---

## Quick Start

### 1. Create a new project

```bash
rustjvm new hello-world
cd hello-world
```

This scaffolds:

```
hello-world/
├── pom.xml                    # Maven project with the rustjvm dependency
├── src/
│   ├── App.java               # @RustJVMApplication entry point
│   ├── HelloController.java
│   └── HelloService.java
├── rustjvm.toml
└── README.md
```

### 2. Look at your Java

`src/HelloService.java`:

```java
package com.example;

import rustjvm.spring.context.Service;

@Service
public class HelloService {
    public String greet(String name) {
        return "Hello, " + name + "!";
    }
}
```

`src/HelloController.java`:

```java
package com.example;

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
```

`src/App.java`:

```java
package com.example;

import rustjvm.RustJVMBootstrap;
import rustjvm.spring.RustJVMApplication;

@RustJVMApplication
public class App {
    public static void main(String[] args) {
        System.exit(RustJVMBootstrap.run(App.class, args));
    }
}
```

`RustJVMBootstrap` locates the runtime binary (`RUSTJVM_HOME/bin` → `~/.cargo/bin` → `PATH`) and delegates to `rustjvm run --src src --main com.example.App`. The `--main` flag restricts component scanning to the application's package, exactly like Spring Boot.

### 3. Run

```bash
rustjvm run --src src --port 8080
```

Output:

```
  GET    /hello                   -> HelloController.hello
INFO 1 route(s) installed from src
INFO 3 bean(s) wired
INFO LiveRust watching src
INFO RustJVM listening on http://127.0.0.1:8080 (cold start: 3.9ms)
```

Test it:

```bash
curl "http://localhost:8080/hello?name=RustJVM"
# Hello, RustJVM!
```

### 4. Hot reload

Edit `HelloController.java` while the server is running:

```java
@GetMapping("/hello")
public String hello(@RequestParam String name) {
    return helloService.greet(name) + " 🔥 from RustJVM";
}
```

Save. The change is live in ~350µs — no restart:

```bash
curl "http://localhost:8080/hello?name=RustJVM"
# Hello, RustJVM! 🔥 from RustJVM
```

In-flight requests finish on the old code; state, singletons, and connections survive.

---

## Core Concepts

### Dependency Injection

RustJVM's DI container is implemented in Rust and wired into your Java code at runtime. It supports:

| Feature | Status | Notes |
|---|---|---|
| `@Service` / `@Component` | ✅ | Singleton by default |
| `@Autowired` (field) | ✅ | Resolved by type, or by name when ambiguous |
| `@Autowired` (constructor) | 🚧 | Planned for v0.2 — use field injection today |
| `@Configuration` + `@Bean` | ✅ | Factory methods, parameters become dependencies |
| `@Scope` (`singleton`/`prototype`/`request`) | ✅ | Request-scoped beans live in the request arena |
| `@ComponentScan` | ✅ | Explicit base packages; defaults to the app package |
| Circular dependency detection | ✅ | Fails fast at bootstrap with the cycle path |

Example — request-scoped bean:

```java
import rustjvm.spring.context.Scope;
import rustjvm.spring.context.Service;

@Service
@Scope("request")
public class RequestContext {
    // A fresh instance per HTTP request, allocated in the request arena
    // and freed en masse when the response completes. No GC involved.
}
```

Example — factory bean:

```java
import rustjvm.spring.context.Bean;
import rustjvm.spring.context.Configuration;

@Configuration
public class AppConfig {
    @Bean                       // PrefixService is a plain class — no annotation needed
    public PrefixService prefixService() {
        return new PrefixService();
    }
}
```

### Hot Reload

RustJVM uses differential patching:

1. You save a `.java` file.
2. The Rust runtime detects the change via `notify`.
3. Only the modified file is recompiled.
4. The route table and bean registry (`ArcSwap`) are updated **atomically** — routes and DI always swap together, never half-wired.
5. In-flight requests hold an `Arc` to the old implementation and drain naturally.
6. New requests hit the new code immediately.

Safety guarantees:

- Broken edits (compilation errors, unresolvable beans) are **rejected**; the old code continues serving and the error is logged.
- Signature changes trigger dependent bean re-wiring.
- Singleton state (DB pools, caches) is preserved across reloads.

### Memory Model

```
┌─────────────────────────────────────┐
│      Java Application Layer         │
│   @RestController, @Service, etc.   │
└─────────────┬───────────────────────┘
              │ zero-copy FFI
┌─────────────▼───────────────────────┐
│        Rust Core Runtime            │
│  ┌─────────────────────────────┐    │
│  │   Arena Allocator (per-req) │    │
│  │   - Request-scoped beans    │    │
│  │   - Response buffers        │    │
│  └─────────────────────────────┘    │
│  ┌─────────────────────────────┐    │
│  │   Object Pool (singletons)  │    │
│  │   - @Service instances      │    │
│  │   - DB connection pools     │    │
│  └─────────────────────────────┘    │
│  ┌─────────────────────────────┐    │
│  │   Vector Store (RAG)        │    │
│  │   - Memory-mapped index     │    │  (Phase 3)
│  └─────────────────────────────┘    │
└─────────────────────────────────────┘
```

No garbage collector. Memory is freed deterministically:

- Singletons live for the application lifetime.
- Request arenas are dropped when the HTTP response completes.
- Prototypes are reference-counted and freed when the last handle drops.

---

## Annotations Reference

All annotations live under the `rustjvm.spring` package tree and ship in the `rustjvm-spring-api` JAR.

### Core

| Annotation | Target | Description |
|---|---|---|
| `@RustJVMApplication` | TYPE | Entry point. Triggers component scan and bootstrap. |
| `@ComponentScan` | TYPE | Configures base packages to scan. |

### DI (`rustjvm.spring.context`)

| Annotation | Target | Description |
|---|---|---|
| `@Service` | TYPE | Marks a business-layer bean (optional name value). |
| `@Component` | TYPE | Generic managed component. |
| `@Autowired` | FIELD | Declares an injected dependency. |
| `@Configuration` | TYPE | Marks a class that produces beans via `@Bean` methods. |
| `@Bean` | METHOD | Declares a factory method inside a `@Configuration` class. |
| `@Scope` | TYPE | `"singleton"` (default), `"prototype"`, or `"request"`. |

### Web (`rustjvm.spring.web`)

| Annotation | Target | Description |
|---|---|---|
| `@RestController` | TYPE | Marks an HTTP request handler. |
| `@GetMapping` | METHOD | Binds a method to HTTP GET. |
| `@PostMapping` | METHOD | Binds a method to HTTP POST. |
| `@RequestMapping` | TYPE/METHOD | Base path mapping. |
| `@RequestParam` | PARAMETER | Binds a query parameter to a method argument. |
| `@RequestBody` | PARAMETER | ⏳ Planned — binds the request body. |

### AI (`rustjvm.spring.ai`)

| Annotation | Target | Description |
|---|---|---|
| `@Prompt` | METHOD | ⏳ Phase 3 — auto-implements a method via LLM prompt. |
| `@Tool` | METHOD | ⏳ Phase 3 — exposes a method as an LLM tool call. |

---

## AI & RAG Integration

> **Phase 3 — preview.** The annotation surface (`@Prompt`) is already in the API
> JAR; the Rust-backed engine lands with Phase 3. The APIs below are the design,
> shown so you can see where RustJVM is headed. Nothing in this section runs yet.

RustJVM will have first-class AI support — no external services required for basic RAG.

**Built-in vector store** (Rust-native, memory-mapped, zero-copy):

```java
import rustjvm.spring.ai.VectorStore;
import rustjvm.spring.context.Autowired;
import rustjvm.spring.context.Service;

@Service
public class DocumentService {

    @Autowired
    private VectorStore vectorStore;   // auto-chunked + embedded, in-process

    public List<String> search(String query) {
        return vectorStore.similaritySearch(query, 5);
    }
}
```

**Prompt-as-code:**

```java
@Prompt("Generate a creative greeting for {name} in the style of {style}")
@GetMapping("/creative-hello")
public String creativeHello(String name, String style) {
    return null;   // method body provided by the LLM at runtime
}
```

---

## Configuration

### `rustjvm.toml`

`rustjvm.toml` is RustJVM's configuration format. `rustjvm new` scaffolds a
minimal one, and `rustjvm migrate convert` generates it from a Spring Boot
`application.properties`. The full schema:

```toml
[server]
port = 8080
host = "0.0.0.0"
workers = 4

[reload]
watch = true
exclude = ["target/", ".git/"]

[ai]
local_model_path = "./models/mistral-7b.q4.gguf"

[ai.openai]
api_key = "${OPENAI_API_KEY}"
model = "gpt-4o"

[rag]
vector_store_path = "./data/vectors"
embedding_model = "nomic-embed-text"
chunk_size = 512

[logging]
level = "info"
format = "json"
```

> **Status:** today `rustjvm run` takes configuration from flags and environment
> variables; automatic `rustjvm.toml` loading lands with the config-mapped
> sections that need it (`[ai]`, `[rag]`) in Phase 3.

### Environment Variables

| Variable | Status | Description |
|---|---|---|
| `RUSTJVM_HOME` | ✅ | Runtime install directory, used by `RustJVMBootstrap` and the installers |
| `RUSTJVM_PORT` | ✅ | Default port for `rustjvm run` (the `--port` flag wins) |
| `RUSTJVM_LOG` | ✅ | Log filter (`trace`, `debug`, `info`, `warn`, `error`, or a full `tracing` directive) |
| `RUSTJVM_AI_API_KEY` | ⏳ Phase 3 | API key for a remote LLM provider |

---

## Building & Running

### Development

```bash
rustjvm run --src src --port 8080                  # serve + watch for changes
rustjvm run --src src --main com.example.App       # scan only the app package
rustjvm routes --src src                           # print the route table and exit
```

With the build-tool plugins (built from this repo until published — see
[rustjvm-maven-plugin/](rustjvm-maven-plugin) and
[rustjvm-gradle-plugin/](rustjvm-gradle-plugin)):

```bash
mvn rustjvm:run
./gradlew rustjvmRun
```

### Production

```bash
# Validate the whole tree (routes compile, DI graph wires) and write a
# build manifest to target/rustjvm/manifest.json — the CI packaging gate.
rustjvm build --src src

# Liveness probe for containers (exit 0/1)
rustjvm healthcheck --port 8080
```

### Docker

```dockerfile
FROM debian:stable-slim
COPY --from=builder /target/release/rustjvm /usr/local/bin/rustjvm
COPY src/ /app/src/
EXPOSE 8080
HEALTHCHECK CMD ["rustjvm", "healthcheck", "--port", "8080"]
CMD ["rustjvm", "run", "--src", "/app/src", "--port", "8080"]
```

```bash
docker build -t my-rustjvm-app .
docker run -p 8080:8080 my-rustjvm-app
```

---

## Ecosystem & Tooling

Shipped with the runtime:

| Tool | What it does |
|---|---|
| `rustjvm migrate analyze --from <spring-app>` | Spring Boot compatibility report (compatible / partial / unsupported) |
| `rustjvm migrate convert --from <app> --to <dir>` | Rewrites imports + annotations, generates `rustjvm.toml` and `README-MIGRATION.md` |
| `rustjvm k8s generate --app <name> --image <img>` | Kubernetes bundle: `RustJVMApp` CRD, Deployment, Service, ConfigMap |
| `rustjvm docs generate --src src` | OpenAPI 3.0 spec (`docs/openapi.json`) + bean dependency graph |
| `GET /health` | Built-in liveness endpoint |
| `GET /metrics` | Built-in Prometheus metrics (requests, hot reloads, arena memory) |

Try the migration on the bundled fixture:

```bash
rustjvm migrate analyze --from examples/spring-petstore-lite
```

---

## Testing

```bash
# Unit + integration tests (compiler, DI, HTTP lifecycle, hot swap under load)
cargo test --workspace

# HTTP integration tests only
cargo test -p rustjvm-runtime --test http_integration

# Java API JAR tests
mvn -f rustjvm-spring-api/pom.xml test          # or: just jar

# Performance gates (criterion)
cargo bench -p rustjvm-runtime                  # or: just bench
```

Key benchmarks tracked as gates:

- `bench_cold_start` — must stay < 50ms
- `bench_hot_swap` — must stay < 1ms
- `bench_request_p99` — must stay < 100µs

---

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                     Java Application                        │
│    (Your code: @RestController, @Service, @Autowired)       │
│                                                             │
│  Compile-time dependency:                                   │
│  io.rustjvm:rustjvm-spring-api:0.1.0-alpha (JAR)            │
└──────────────────────────┬──────────────────────────────────┘
                           │ zero-copy FFI
┌──────────────────────────▼──────────────────────────────────┐
│                    rustjvm-runtime                          │
│  ┌─────────────┐  ┌───────────────┐  ┌──────────────────┐   │
│  │ HTTP Server │  │ DI Container  │  │ Arena Allocator  │   │
│  │  (Axum)     │  │ (BeanRegistry)│  │ (per-request)    │   │
│  └─────────────┘  └───────────────┘  └──────────────────┘   │
│  ┌─────────────┐  ┌───────────────┐  ┌──────────────────┐   │
│  │ Hot Reload  │  │ AI Engine ⏳   │  │ ML Runtime ⏳     │   │
│  │ (ArcSwap)   │  │ (RAG + LLM)   │  │ (candle/burn)    │   │
│  └─────────────┘  └───────────────┘  └──────────────────┘   │
└─────────────────────────────────────────────────────────────┘
        Native binary: rustjvm (installed via cargo / installer)
```

⏳ = Phase 3/4 components; everything else is live today.

---

## Roadmap

| Phase | Status | Deliverables |
|---|---|---|
| Phase 1: Foundation | ✅ | HTTP server, hot reload, `@RestController`, 3.9ms cold start |
| Phase 2: Spring Parity | ✅ | DI container, `@Autowired`, `@Configuration`/`@Bean`, scopes, `@ComponentScan` |
| Phase 3: AI-Native | ⏳ | RAG pipeline, Spring AI API, LLM streaming, `@Prompt` |
| Phase 4: ML & Self-Healing | ⏳ | Tensor ops, RustFix auto-correction, model inference |
| Phase 5: Ecosystem | 🔄 | Migration tool, K8s manifests, docs generator, Maven/Gradle plugins ✅; Central/crates.io publishing + K8s operator pending |

---

## Contributing

We welcome contributors who are passionate about Java and Rust.

```bash
git clone https://github.com/rustjvm/rustjvm.git
cd rustjvm
cargo test --workspace
```

Development chat prompt — if you're using Cursor or another AI pair-programmer, ground it with:

```
You are building RustJVM, a Rust-powered runtime for Java that replaces Spring Boot.
Key constraints:
- Rust handles memory, I/O, and concurrency. No JVM. No GC.
- Java is the developer-facing language. Spring annotations must work.
- Hot-reload must preserve state and be <200ms.
- AI/ML features are first-class, not bolted-on.
- Every design decision prioritizes latency and memory efficiency.
```

---

## License

RustJVM is licensed under the Apache License 2.0. See [LICENSE](LICENSE) for details.

---

<div align="center">

Built with 🦀 and ☕ by developers who believe Java deserves better.

[Website](https://rustjvm.dev) · [Docs](https://github.com/rustjvm/rustjvm#readme)

</div>
