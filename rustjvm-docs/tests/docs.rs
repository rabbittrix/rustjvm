use rustjvm_compiler::analyze_source;
use rustjvm_docs::{generate_bean_graph, generate_openapi};

const APP: &str = r#"
package com.example.di;

@Service
public class GreetingService {
    public String greet(String name) { return "Hello, " + name + "!"; }
}

@RestController
public class HelloController {

    @Autowired
    private GreetingService greetingService;

    @GetMapping("/greet")
    public String greet(@RequestParam String name) {
        return greetingService.greet(name);
    }

    @GetMapping("/ping")
    public String ping() { return "pong"; }
}
"#;

#[test]
fn openapi_covers_every_route_with_params() {
    let analysis = analyze_source(APP).unwrap();
    let doc = generate_openapi("di-app", &analysis.routes);
    let parsed: serde_json::Value = serde_json::from_str(&doc).unwrap();

    assert_eq!(parsed["openapi"], "3.0.3");

    let greet = &parsed["paths"]["/greet"]["get"];
    assert_eq!(greet["operationId"], "HelloController.greet");
    assert_eq!(greet["parameters"][0]["name"], "name");
    assert_eq!(greet["parameters"][0]["required"], true);

    let ping = &parsed["paths"]["/ping"]["get"];
    assert_eq!(ping["parameters"].as_array().unwrap().len(), 0);

    // Responses document the runtime's real status codes.
    assert!(greet["responses"]["200"].is_object());
    assert!(greet["responses"]["400"].is_object());
    assert!(greet["responses"]["501"].is_object());
}

#[test]
fn bean_graph_lists_beans_and_edges() {
    let analysis = analyze_source(APP).unwrap();
    let md = generate_bean_graph(&analysis.beans);

    assert!(md.contains("`helloController`"));
    assert!(md.contains("`greetingService`"));
    assert!(md.contains("`greetingService` (GreetingService)"));
    assert!(md.contains("helloController --greetingService--> GreetingService"));
}
