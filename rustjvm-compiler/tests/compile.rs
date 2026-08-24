use rustjvm_compiler::{compile_source, EvalError, MethodImpl};
use std::collections::HashMap;

const HELLO: &str = r#"
package com.example.hello;

import rustjvm.spring.web.GetMapping;
import rustjvm.spring.web.RequestParam;
import rustjvm.spring.web.RestController;

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

fn args(pairs: &[(&str, &str)]) -> HashMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

#[test]
fn discovers_all_routes() {
    let routes = compile_source(HELLO).unwrap();
    assert_eq!(routes.len(), 2);
    let hello = routes.iter().find(|r| r.path == "/hello").unwrap();
    assert_eq!(hello.http_method, "GET");
    assert_eq!(hello.class_name, "HelloController");
    assert_eq!(hello.method_name, "hello");
}

#[test]
fn evaluates_template_with_bound_param() {
    let routes = compile_source(HELLO).unwrap();
    let hello = routes.iter().find(|r| r.path == "/hello").unwrap();
    let out = hello.implementation.eval(&args(&[("name", "RustJVM")])).unwrap();
    assert_eq!(out, "Hello, RustJVM!");
}

#[test]
fn constant_route_needs_no_params() {
    let routes = compile_source(HELLO).unwrap();
    let ping = routes.iter().find(|r| r.path == "/ping").unwrap();
    assert!(matches!(ping.implementation, MethodImpl::Constant(_)));
    assert_eq!(ping.implementation.eval(&HashMap::new()).unwrap(), "pong");
}

#[test]
fn missing_param_is_an_error_not_a_panic() {
    let routes = compile_source(HELLO).unwrap();
    let hello = routes.iter().find(|r| r.path == "/hello").unwrap();
    let err = hello.implementation.eval(&HashMap::new()).unwrap_err();
    assert!(matches!(err, EvalError::MissingParam(p) if p == "name"));
}

#[test]
fn class_level_request_mapping_prefixes_paths() {
    let src = r#"
@RestController
@RequestMapping("/api")
public class ApiController {
    @GetMapping("/status")
    public String status() { return "ok"; }
}
"#;
    let routes = compile_source(src).unwrap();
    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].path, "/api/status");
}

#[test]
fn explicit_request_param_name_wins() {
    let src = r#"
@RestController
public class C {
    @GetMapping("/greet")
    public String greet(@RequestParam("who") String person) {
        return "hi " + person;
    }
}
"#;
    let routes = compile_source(src).unwrap();
    assert_eq!(routes[0].params[0].param, "person");
    assert_eq!(routes[0].params[0].query_key, "who");
}

#[test]
fn dead_locals_still_allow_literal_returns() {
    let src = r#"
@RestController
public class C {
    @GetMapping("/calc")
    public String calc() {
        int x = 40 + 2;
        return "answer";
    }
}
"#;
    let routes = compile_source(src).unwrap();
    assert_eq!(
        routes[0].implementation.eval(&HashMap::new()).unwrap(),
        "answer"
    );
}

#[test]
fn complex_bodies_become_unimplemented_never_crash() {
    let src = r#"
@RestController
public class C {
    @GetMapping("/local")
    public String local() {
        String msg = "computed";
        return "value: " + msg;
    }

    @GetMapping("/nothing")
    public String nothing() {
        return null;
    }
}
"#;
    let routes = compile_source(src).unwrap();
    assert_eq!(routes.len(), 2);
    for r in &routes {
        assert!(
            matches!(r.implementation, MethodImpl::Unimplemented(_)),
            "{} should be unimplemented",
            r.path
        );
        let err = r.implementation.eval(&HashMap::new()).unwrap_err();
        assert!(matches!(err, EvalError::Unimplemented(_)));
    }
}

#[test]
fn fields_constructors_and_autowired_noise_are_skipped() {
    let src = r#"
@RestController
public class Noisy {

    private final String prefix = "x";

    @Autowired
    private GreetingService service;

    public Noisy(GreetingService service) {
        this.service = service;
    }

    @GetMapping("/ok")
    public String ok() {
        return "ok";
    }
}
"#;
    let routes = compile_source(src).unwrap();
    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].path, "/ok");
}
