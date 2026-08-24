use rustjvm_compiler::compile_source;
use rustjvm_runtime::{DispatchTable, InstallOutcome};

fn controller(greeting: &str) -> String {
    format!(
        r#"@RestController
public class C {{
    @GetMapping("/x")
    public String x() {{
        return "{greeting}";
    }}
}}"#
    )
}

fn only_route(src: &str) -> rustjvm_compiler::CompiledRoute {
    compile_source(src).unwrap().into_iter().next().unwrap()
}

#[test]
fn swap_is_atomic_and_in_flight_requests_drain() {
    let table = DispatchTable::new();

    assert!(matches!(
        table.install(only_route(&controller("one"))),
        InstallOutcome::Registered
    ));

    // A request resolves the old version and is still holding it...
    let in_flight = table.resolve("GET", "/x").unwrap();
    assert_eq!(
        in_flight.implementation.eval(&Default::default()).unwrap(),
        "one"
    );

    // ...when LiveRust swaps in the new version atomically.
    assert!(matches!(
        table.install(only_route(&controller("two"))),
        InstallOutcome::Swapped
    ));

    // New requests get the new code immediately.
    let fresh = table.resolve("GET", "/x").unwrap();
    assert_eq!(
        fresh.implementation.eval(&Default::default()).unwrap(),
        "two"
    );

    // The in-flight request still drains against the old code — never yanked.
    assert_eq!(
        in_flight.implementation.eval(&Default::default()).unwrap(),
        "one"
    );

    assert_eq!(table.route_count(), 1);
}

#[test]
fn any_method_routes_match_all_verbs() {
    let table = DispatchTable::new();
    let src = r#"
@RestController
public class C {
    @RequestMapping("/multi")
    public String multi() { return "ok"; }
}
"#;
    table.install(only_route(src));
    for verb in ["GET", "POST", "DELETE"] {
        assert!(table.resolve(verb, "/multi").is_some());
    }
    assert!(table.resolve("GET", "/nope").is_none());
}
