use rustjvm_compiler::{
    analyze_source, scan_base_packages, under_scan_bases, BeanKind, BeanOrigin, EvalError,
    MethodImpl, Part, Scope,
};
use std::collections::HashMap;

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

#[test]
fn extracts_beans_with_dependencies() {
    let analysis = analyze_source(DI_APP).unwrap();
    let beans = &analysis.beans;
    assert_eq!(beans.len(), 2);

    let service = beans.iter().find(|b| b.class_name == "GreetingService").unwrap();
    assert_eq!(service.name, "greetingService");
    assert_eq!(service.kind, BeanKind::Service);
    assert_eq!(service.scope, Scope::Singleton);
    assert_eq!(service.package.as_deref(), Some("com.example.di"));
    assert!(service.dependencies.is_empty());
    assert_eq!(service.methods.len(), 1);
    assert_eq!(service.methods[0].params, vec!["name".to_string()]);

    let controller = beans
        .iter()
        .find(|b| b.class_name == "HelloController")
        .unwrap();
    assert_eq!(controller.kind, BeanKind::Controller);
    assert_eq!(controller.dependencies.len(), 1);
    assert_eq!(controller.dependencies[0].field_name, "greetingService");
    assert_eq!(controller.dependencies[0].type_name, "GreetingService");
}

#[test]
fn service_call_compiles_to_call_part() {
    let analysis = analyze_source(DI_APP).unwrap();
    let greet = analysis.routes.iter().find(|r| r.path == "/greet").unwrap();
    match &greet.implementation {
        MethodImpl::Template(parts) => {
            assert!(matches!(
                &parts[0],
                Part::Call { receiver, method, .. }
                    if receiver == "greetingService" && method == "greet"
            ));
        }
        other => panic!("expected template with call, got {other:?}"),
    }
}

#[test]
fn call_without_bean_context_is_graceful_error() {
    let analysis = analyze_source(DI_APP).unwrap();
    let greet = analysis.routes.iter().find(|r| r.path == "/greet").unwrap();
    let err = greet
        .implementation
        .eval(&HashMap::from([("name".to_string(), "x".to_string())]))
        .unwrap_err();
    assert!(matches!(err, EvalError::Unimplemented(_)));
}

#[test]
fn call_on_non_autowired_receiver_is_rejected() {
    let src = r#"
@RestController
public class C {
    private NotInjected plain;

    @GetMapping("/x")
    public String x() {
        return plain.go();
    }
}
"#;
    let analysis = analyze_source(src).unwrap();
    assert!(matches!(
        analysis.routes[0].implementation,
        MethodImpl::Unimplemented(_)
    ));
}

#[test]
fn call_with_local_variable_arg_is_rejected() {
    let src = r#"
@RestController
public class C {
    @Autowired
    private Svc svc;

    @GetMapping("/x")
    public String x() {
        String computed = "nope";
        return svc.go(computed);
    }
}
"#;
    let analysis = analyze_source(src).unwrap();
    assert!(matches!(
        analysis.routes[0].implementation,
        MethodImpl::Unimplemented(_)
    ));
}

#[test]
fn scope_annotation_sets_prototype_and_request() {
    let src = r#"
@Service
@Scope("prototype")
public class Fresh { public String x() { return "x"; } }

@Service
@Scope("request")
public class PerReq { public String x() { return "x"; } }
"#;
    let analysis = analyze_source(src).unwrap();
    let fresh = analysis.beans.iter().find(|b| b.class_name == "Fresh").unwrap();
    assert_eq!(fresh.scope, Scope::Prototype);
    let per_req = analysis.beans.iter().find(|b| b.class_name == "PerReq").unwrap();
    assert_eq!(per_req.scope, Scope::Request);
}

#[test]
fn explicit_bean_name_overrides_default() {
    let src = r#"@Service("mail") public class MailService { public String x() { return "x"; } }"#;
    let analysis = analyze_source(src).unwrap();
    assert_eq!(analysis.beans[0].name, "mail");
    assert_eq!(analysis.beans[0].class_name, "MailService");
}

#[test]
fn bean_factory_methods_are_extracted_with_param_dependencies() {
    let src = r#"
@Configuration
public class AppConfig {
    @Bean
    public PrefixService prefixService() {
        return new PrefixService();
    }

    @Bean(name = "loud", scope = "prototype")
    public LoudService loudService(PrefixService prefixService) {
        return new LoudService(prefixService);
    }
}
"#;
    let analysis = analyze_source(src).unwrap();
    let factory_beans: Vec<_> = analysis
        .beans
        .iter()
        .filter(|b| matches!(b.origin, BeanOrigin::FactoryMethod { .. }))
        .collect();
    assert_eq!(factory_beans.len(), 2);

    let prefix = factory_beans
        .iter()
        .find(|b| b.name == "prefixService")
        .unwrap();
    assert_eq!(prefix.class_name, "PrefixService");
    assert_eq!(prefix.scope, Scope::Singleton);
    assert!(prefix.dependencies.is_empty());

    let loud = factory_beans.iter().find(|b| b.name == "loud").unwrap();
    assert_eq!(loud.class_name, "LoudService");
    assert_eq!(loud.scope, Scope::Prototype);
    assert_eq!(loud.dependencies.len(), 1);
    assert_eq!(loud.dependencies[0].type_name, "PrefixService");
}

#[test]
fn component_scan_bases_are_collected() {
    let src = r#"
package com.example.app;

@RustJVMApplication
public class App {
}
"#;
    let analysis = analyze_source(src).unwrap();
    let bases = scan_base_packages(&analysis.beans).unwrap();
    assert_eq!(bases, vec!["com.example.app".to_string()]);

    assert!(under_scan_bases(
        &Some("com.example.app.web".to_string()),
        &Some(bases.clone())
    ));
    assert!(!under_scan_bases(
        &Some("org.other".to_string()),
        &Some(bases)
    ));
    // No scan annotation → accept everything.
    assert!(under_scan_bases(&Some("anything".to_string()), &None));
}
