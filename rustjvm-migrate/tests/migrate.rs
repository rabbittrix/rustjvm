use rustjvm_migrate::{
    convert_project, convert_properties, convert_source, Compat, SpringBootAnalyzer,
};
use std::path::Path;

const SPRING_APP: &str = r#"
package com.example.demo;

import org.springframework.boot.autoconfigure.SpringBootApplication;
import org.springframework.web.bind.annotation.GetMapping;
import org.springframework.web.bind.annotation.RequestParam;
import org.springframework.web.bind.annotation.RestController;
import org.springframework.stereotype.Service;
import org.springframework.beans.factory.annotation.Autowired;

@SpringBootApplication
public class DemoApplication {
}

@RestController
class HelloController {

    @Autowired
    private HelloService helloService;

    @GetMapping("/hello")
    public String hello(@RequestParam String name) {
        return helloService.greet(name);
    }
}

@Service
class HelloService {
    public String greet(String name) {
        return "Hello, " + name + "!";
    }
}
"#;

const SPRING_POM: &str = r#"
<project>
  <dependencies>
    <dependency><artifactId>spring-boot-starter-web</artifactId></dependency>
    <dependency><artifactId>spring-boot-starter-data-jpa</artifactId></dependency>
    <dependency><artifactId>spring-boot-starter-security</artifactId></dependency>
  </dependencies>
</project>
"#;

const SPRING_PROPS: &str = "server.port=8081\n\
    spring.datasource.url=jdbc:postgresql://localhost:5432/mydb\n\
    spring.datasource.username=admin\n\
    spring.datasource.password=secret\n\
    logging.level.root=INFO\n\
    spring.main.lazy-initialization=true\n";

fn write_fixture(root: &Path) {
    let java_dir = root.join("src/main/java/com/example/demo");
    std::fs::create_dir_all(&java_dir).unwrap();
    std::fs::write(java_dir.join("DemoApplication.java"), SPRING_APP).unwrap();
    std::fs::write(root.join("pom.xml"), SPRING_POM).unwrap();
    let res_dir = root.join("src/main/resources");
    std::fs::create_dir_all(&res_dir).unwrap();
    std::fs::write(res_dir.join("application.properties"), SPRING_PROPS).unwrap();
}

#[test]
fn analyzer_detects_spring_boot_app() {
    let tmp = tempfile::tempdir().unwrap();
    write_fixture(tmp.path());

    let report = SpringBootAnalyzer::analyze(tmp.path());

    // The headline conversion is detected.
    assert!(report
        .findings
        .iter()
        .any(|f| f.feature == "@SpringBootApplication" && f.compat == Compat::Compatible));

    // rustjvm.toml is generated from application.properties.
    let toml = report
        .generated
        .iter()
        .find(|g| g.path == Path::new("rustjvm.toml"))
        .expect("rustjvm.toml generated");
    assert!(toml.contents.contains("port = 8081"));
    assert!(toml
        .contents
        .contains("url = \"postgresql://localhost:5432/mydb\""));

    // Dependency verdicts: web compatible, jpa partial, security unsupported.
    assert!(report
        .findings
        .iter()
        .any(|f| f.feature == "spring-boot-starter-web" && f.compat == Compat::Compatible));
    assert!(report
        .findings
        .iter()
        .any(|f| f.feature == "spring-boot-starter-data-jpa" && f.compat == Compat::Partial));
    assert!(report
        .findings
        .iter()
        .any(|f| f.feature == "spring-boot-starter-security" && f.compat == Compat::Unsupported));

    // Conversions are planned for the Java sources.
    assert!(report
        .conversions
        .iter()
        .any(|c| c.description.contains("@RustJVMApplication")));

    // The markdown report renders all three sections.
    let md = report.to_markdown();
    assert!(md.contains("Fully compatible"));
    assert!(md.contains("Needs attention"));
    assert!(md.contains("Unsupported"));
}

#[test]
fn converter_generates_valid_toml() {
    let toml = convert_properties(SPRING_PROPS);
    assert!(toml.contains("[server]"));
    assert!(toml.contains("port = 8081"));
    assert!(toml.contains("[database]"));
    assert!(toml.contains("url = \"postgresql://localhost:5432/mydb\""));
    assert!(toml.contains("username = \"admin\""));
    assert!(toml.contains("password = \"secret\""));
    assert!(toml.contains("level = \"info\""));
    // Unmapped keys are preserved as TODOs, never silently dropped.
    assert!(toml.contains("TODO(migrate): spring.main.lazy-initialization"));
}

#[test]
fn converter_rewrites_annotations_and_imports() {
    let converted = convert_source(SPRING_APP);
    assert!(converted.source.contains("import rustjvm.spring.RustJVMApplication;"));
    assert!(converted.source.contains("import rustjvm.spring.web.RestController;"));
    assert!(converted.source.contains("import rustjvm.spring.context.Service;"));
    assert!(converted.source.contains("import rustjvm.spring.context.Autowired;"));
    assert!(!converted.source.contains("org.springframework"));
    assert!(converted.changes.iter().any(|c| c.contains("@RustJVMApplication")));
}

#[test]
fn convert_project_writes_runnable_tree() {
    let from = tempfile::tempdir().unwrap();
    let to = tempfile::tempdir().unwrap();
    write_fixture(from.path());

    let report = convert_project(from.path(), to.path()).unwrap();

    // The report reflects the applied conversions.
    assert!(report
        .conversions
        .iter()
        .any(|c| c.description.contains("@RustJVMApplication")));

    // Converted Java source landed in the output tree.
    let migrated = std::fs::read_to_string(
        to.path()
            .join("src/main/java/com/example/demo/DemoApplication.java"),
    )
    .unwrap();
    assert!(migrated.contains("@RustJVMApplication"));
    assert!(!migrated.contains("org.springframework"));

    // rustjvm.toml + README-MIGRATION.md generated.
    let toml = std::fs::read_to_string(to.path().join("rustjvm.toml")).unwrap();
    assert!(toml.contains("port = 8081"));
    let readme = std::fs::read_to_string(to.path().join("README-MIGRATION.md")).unwrap();
    assert!(readme.contains("Manual steps remaining"));
    assert!(readme.contains("spring-boot-starter-security"));

    // The migrated sources actually compile on the RustJVM compiler —
    // the real proof that conversion produced a runnable app.
    let analysis = rustjvm_compiler_harness(&migrated);
    assert_eq!(analysis.routes.len(), 1);
    assert_eq!(analysis.routes[0].path, "/hello");
    assert!(analysis.beans.len() >= 2, "controller + service beans");
}

fn rustjvm_compiler_harness(src: &str) -> rustjvm_compile_check::Analysis {
    rustjvm_compile_check::analyze(src)
}

// Tiny shim so the test crate doesn't depend on the whole workspace wiring;
// the real compiler is pulled in as a dev-dependency below.
mod rustjvm_compile_check {
    pub use rustjvm_compiler::Analysis;
    pub fn analyze(src: &str) -> Analysis {
        rustjvm_compiler::analyze_source(src).expect("migrated source parses and compiles")
    }
}
