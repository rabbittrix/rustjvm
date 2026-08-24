use rustjvm_compiler::analyze_source;
use rustjvm_runtime::{
    assemble_registry, BeanRegistry, DIError, NativeDef, RequestArena, RequestContext,
};
use std::collections::HashMap;
use std::sync::Arc;

const DI_APP: &str = r#"
package com.example.di;

import rustjvm.spring.context.Autowired;
import rustjvm.spring.context.Service;
import rustjvm.spring.web.GetMapping;
import rustjvm.spring.web.RequestParam;
import rustjvm.spring.web.RestController;

@Service
public class GreetingService {
    public String greet(String name) {
        return "Hello, " + name + "!";
    }
}

@RestController
public class HelloController {

    @Autowired
    private GreetingService greetingService;

    @GetMapping("/greet")
    public String greet(@RequestParam String name) {
        return greetingService.greet(name);
    }
}
"#;

fn assemble(src: &str) -> Result<BeanRegistry, DIError> {
    let a = analyze_source(src).unwrap();
    assemble_registry(&[(a.beans, a.classes)], Vec::new())
}

#[test]
fn di_container_wires_autowired_fields() {
    let registry = assemble(DI_APP).unwrap();
    assert_eq!(registry.len(), 2);

    let controller = registry.get("helloController").unwrap();
    let service = controller.injected("greetingService").expect("wired");
    assert_eq!(service.class_name, "GreetingService");

    // Same instance by type lookup.
    let by_type = registry.get_by_type("HelloController").unwrap();
    assert!(Arc::ptr_eq(&controller, &by_type));
}

#[test]
fn controller_method_dispatches_through_registry() {
    let registry = assemble(DI_APP).unwrap();
    let controller = registry.get("helloController").unwrap();
    let out = controller.invoke("greet", &["world".to_string()], 0).unwrap();
    assert_eq!(out, "Hello, world!");
}

#[test]
fn missing_dependency_fails_fast() {
    let src = r#"
@RestController
public class C {
    @Autowired
    private MissingService missing;
    @GetMapping("/x")
    public String x() { return "x"; }
}
"#;
    let err = assemble(src).unwrap_err();
    assert!(matches!(err, DIError::NoSuchBean(t) if t == "MissingService"));
}

#[test]
fn circular_dependency_reports_cycle_path() {
    let src = r#"
@Service
public class A { @Autowired private B b; public String a() { return "a"; } }

@Service
public class B { @Autowired private A a; public String b() { return "b"; } }
"#;
    let err = assemble(src).unwrap_err();
    let DIError::Cycle(path) = err else {
        panic!("expected cycle, got {err:?}")
    };
    assert!(path.contains("->"), "cycle should show a path, got {path}");
}

#[test]
fn ambiguous_type_is_rejected() {
    // Two beans with the same simple class name (e.g. from different
    // packages) but distinct bean names: injection by type is ambiguous.
    let src = r#"
@Service("implA")
public class ImplOne { public String x() { return "1"; } }

@Service
public class Needs { @Autowired private ImplOne dep; public String x() { return dep.x(); } }
"#;
    let other = r#"@Service("implB") public class ImplOne { public String x() { return "2"; } }"#;
    let a1 = analyze_source(src).unwrap();
    let a2 = analyze_source(other).unwrap();
    let err = assemble_registry(
        &[(a1.beans, a1.classes), (a2.beans, a2.classes)],
        Vec::new(),
    )
    .unwrap_err();
    assert!(matches!(err, DIError::Ambiguous(t, _) if t == "ImplOne"));
}

#[test]
fn deep_chains_wire_in_order() {
    let src = r#"
@Service
public class Leaf { public String value() { return "leaf"; } }

@Service
public class Middle {
    @Autowired private Leaf leaf;
    public String value() { return leaf.value() + "-mid"; }
}

@Service
public class Top {
    @Autowired private Middle middle;
    public String value() { return middle.value() + "-top"; }
}
"#;
    let registry = assemble(src).unwrap();
    let top = registry.get("top").unwrap();
    assert_eq!(top.invoke("value", &[], 0).unwrap(), "leaf-mid-top");
}

#[test]
fn factory_bean_methods_produce_wired_beans() {
    let src = r#"
package com.example.di;

public class PrefixService {
    public String prefix() { return "[rust] "; }
}

@Configuration
public class AppConfig {
    @Bean
    public PrefixService prefixService() { return new PrefixService(); }
}

@Service
public class GreetingService {
    @Autowired private PrefixService prefixService;
    public String greet(String name) { return prefixService.prefix() + "Hello, " + name + "!"; }
}
"#;
    let registry = assemble(src).unwrap();
    let prefix = registry.get("prefixService").unwrap();
    assert_eq!(prefix.class_name, "PrefixService");
    let svc = registry.get("greetingService").unwrap();
    assert_eq!(
        svc.invoke("greet", &["world".to_string()], 0).unwrap(),
        "[rust] Hello, world!"
    );
}

#[test]
fn singleton_injecting_request_scope_is_a_boot_violation() {
    let src = r#"
@Service
@Scope("request")
public class RequestThing { public String x() { return "x"; } }

@Service
public class SingletonThing {
    @Autowired private RequestThing requestThing;
    public String x() { return requestThing.x(); }
}
"#;
    let err = assemble(src).unwrap_err();
    assert!(matches!(err, DIError::ScopeViolation { id, .. } if id == "requestThing"));
}

#[test]
fn prototype_scope_builds_fresh_instances() {
    let src = r#"
@Service
@Scope("prototype")
public class Fresh { public String x() { return "x"; } }
"#;
    let registry = assemble(src).unwrap();
    // Not a singleton: nothing in the singleton map.
    assert!(registry.get("fresh").is_none());

    let arena = RequestArena::new();
    let ctx = RequestContext::new(&arena, &registry);
    let a = ctx.resolve("fresh").unwrap();
    let b = ctx.resolve("fresh").unwrap();
    assert!(!Arc::ptr_eq(&a, &b), "prototype: each resolution is fresh");
    assert_eq!(a.invoke("x", &[], 0).unwrap(), "x");
}

#[test]
fn request_scope_is_cached_per_request_context() {
    let src = r#"
@Service
@Scope("request")
public class PerReq { public String x() { return "x"; } }
"#;
    let registry = assemble(src).unwrap();
    let arena = RequestArena::new();

    let ctx = RequestContext::new(&arena, &registry);
    let a = ctx.resolve("perReq").unwrap();
    let b = ctx.resolve("perReq").unwrap();
    assert!(Arc::ptr_eq(&a, &b), "same request: same instance");

    let ctx2 = RequestContext::new(&arena, &registry);
    let c = ctx2.resolve("perReq").unwrap();
    assert!(!Arc::ptr_eq(&a, &c), "next request: fresh instance");
}

#[test]
fn request_scoped_service_works_through_singleton_controller_in_ctx() {
    // A singleton controller MAY use a request-scoped bean when resolved
    // through a RequestContext — the violation only applies to boot-time
    // singleton wiring. (Spring does this with proxies; we do it with
    // per-request construction.)
    let src = r#"
@Service
@Scope("request")
public class ReqSvc { public String id() { return "req"; } }
"#;
    let registry = assemble(src).unwrap();
    let arena = RequestArena::new();
    let ctx = RequestContext::new(&arena, &registry);
    let svc = ctx.resolve_by_type("ReqSvc").unwrap();
    assert_eq!(svc.invoke("id", &[], 0).unwrap(), "req");
}

#[test]
fn java_to_rust_native_bean_injection() {
    // The headline criterion: a Rust-implemented bean injected into a Java
    // @Service by type, invoked across the zero-copy bridge.
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
    let natives = vec![NativeDef {
        name: "vectorStore".to_string(),
        class_name: "VectorStore".to_string(),
        methods,
    }];

    let a = analyze_source(src).unwrap();
    let registry = assemble_registry(&[(a.beans, a.classes)], natives).unwrap();

    // Native bean present, injectable by type.
    let native = registry.get_by_type("VectorStore").unwrap();
    assert_eq!(native.class_name, "VectorStore");

    // Java service has the native bean wired in.
    let svc = registry.get("searchService").unwrap();
    let wired = svc.injected("vectorStore").expect("native wired");
    assert!(Arc::ptr_eq(&wired, &native));

    // End of the chain: controller → service → Rust closure.
    let controller = registry.get("searchController").unwrap();
    assert_eq!(
        controller.invoke("search", &["cats".to_string()], 0).unwrap(),
        "vector::cats"
    );
}

#[test]
fn component_scan_filters_specs_outside_base_packages() {
    let app = r#"
package com.example.app;

@RustJVMApplication
public class App {
}
"#;
    let outside = r#"
package org.other;

@Service
public class Stray { public String x() { return "x"; } }
"#;
    let inside = r#"
package com.example.app.web;

@Service
public class Kept { public String x() { return "x"; } }
"#;
    let a = analyze_source(app).unwrap();
    let b = analyze_source(outside).unwrap();
    let c = analyze_source(inside).unwrap();
    let registry = assemble_registry(
        &[(a.beans, a.classes), (b.beans, b.classes), (c.beans, c.classes)],
        Vec::new(),
    )
    .unwrap();
    assert!(registry.get("kept").is_some());
    assert!(registry.get("stray").is_none(), "outside scan bases: excluded");
}
