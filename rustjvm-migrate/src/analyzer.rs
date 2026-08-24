//! Scans a Spring Boot project and builds a [`MigrationReport`].

use crate::converter;
use crate::report::{Compat, Conversion, Finding, MigrationReport};
use std::path::{Path, PathBuf};

/// Dependency-level verdicts, matched against pom.xml / build.gradle text.
const DEPENDENCY_RULES: &[(&str, Compat, &str)] = &[
    (
        "spring-boot-starter-web",
        Compat::Compatible,
        "REST controllers map directly to RustJVM routes.",
    ),
    (
        "spring-boot-starter-actuator",
        Compat::Compatible,
        "RustJVM exposes /health and /metrics out of the box.",
    ),
    (
        "spring-boot-starter-validation",
        Compat::Compatible,
        "@RequestParam required/optional semantics are native.",
    ),
    (
        "spring-boot-starter-data-jpa",
        Compat::Partial,
        "Spring Data repositories become RustJVM native DB calls (Phase 2 DB pool). \
         Entities and derived queries need manual porting.",
    ),
    (
        "spring-boot-starter-test",
        Compat::Partial,
        "JUnit tests run via `rustjvm:test`, but Spring test slices \
         (@WebMvcTest, @DataJpaTest) need rewriting.",
    ),
    (
        "spring-ai",
        Compat::Partial,
        "RustJVM Phase 3 provides ChatClient/VectorStore natively — \
         expect a lighter, faster equivalent.",
    ),
    (
        "spring-boot-starter-security",
        Compat::Unsupported,
        "Use RustJVM middleware or an external auth proxy (e.g. oauth2-proxy).",
    ),
    (
        "spring-boot-starter-batch",
        Compat::Unsupported,
        "No batch framework yet — extract jobs into scheduled RustJVM tasks \
         or keep them on Spring Batch in a sidecar.",
    ),
    (
        "projectlombok",
        Compat::Unsupported,
        "No annotation processing — use Java records and explicit constructors.",
    ),
];

/// Annotation-level verdicts, matched against Java source text.
const ANNOTATION_RULES: &[(&str, Compat, &str)] = &[
    ("@SpringBootApplication", Compat::Compatible, "→ @RustJVMApplication"),
    ("@RestController", Compat::Compatible, "→ rustjvm.spring.web.RestController"),
    ("@GetMapping", Compat::Compatible, "mapped as-is"),
    ("@PostMapping", Compat::Compatible, "mapped as-is"),
    ("@PutMapping", Compat::Compatible, "mapped as-is"),
    ("@DeleteMapping", Compat::Compatible, "mapped as-is"),
    ("@RequestMapping", Compat::Compatible, "mapped as-is"),
    ("@RequestParam", Compat::Compatible, "mapped as-is"),
    ("@Service", Compat::Compatible, "→ rustjvm.spring.context.Service"),
    ("@Component", Compat::Compatible, "→ rustjvm.spring.context.Component"),
    ("@Autowired", Compat::Compatible, "→ rustjvm.spring.context.Autowired"),
    ("@Configuration", Compat::Compatible, "mapped as-is"),
    ("@Bean", Compat::Compatible, "mapped as-is"),
    ("@ComponentScan", Compat::Compatible, "mapped as-is"),
    ("@Scope", Compat::Compatible, "singleton/prototype/request supported"),
    (
        "@RequestBody",
        Compat::Partial,
        "JSON body mapping lands in Phase 2 remainder — bind query params for now.",
    ),
    (
        "@PathVariable",
        Compat::Partial,
        "Path templates land with JSON body support — use @RequestParam meanwhile.",
    ),
    (
        "@Transactional",
        Compat::Partial,
        "Transactions arrive with the native DB pool — demarcate manually for now.",
    ),
    (
        "@Scheduled",
        Compat::Unsupported,
        "No scheduler yet — trigger via an external cron hitting an endpoint.",
    ),
    (
        "@Entity",
        Compat::Partial,
        "JPA entities port to plain records + native DB queries.",
    ),
];

pub struct SpringBootAnalyzer;

impl SpringBootAnalyzer {
    /// Analyzes a Spring Boot project tree and produces a full report,
    /// including the generated `rustjvm.toml` preview.
    pub fn analyze(project_path: &Path) -> MigrationReport {
        let mut report = MigrationReport::new();

        for java_file in find_files(project_path, "java") {
            let rel = relative(project_path, &java_file);
            let Ok(source) = std::fs::read_to_string(&java_file) else {
                continue;
            };
            for (token, compat, note) in ANNOTATION_RULES {
                if source.contains(token) {
                    report.add_finding(Finding {
                        feature: token.to_string(),
                        location: rel.clone(),
                        compat: *compat,
                        note: note.to_string(),
                    });
                }
            }
            let conv = converter::convert_source(&source);
            for change in conv.changes {
                report.add_conversion(Conversion {
                    file: rel.clone(),
                    description: change,
                });
            }
        }

        for build_file in ["pom.xml", "build.gradle", "build.gradle.kts"] {
            let path = project_path.join(build_file);
            let Ok(contents) = std::fs::read_to_string(&path) else {
                continue;
            };
            for (dep, compat, note) in DEPENDENCY_RULES {
                if contents.contains(dep) {
                    report.add_finding(Finding {
                        feature: dep.to_string(),
                        location: relative(project_path, &path),
                        compat: *compat,
                        note: note.to_string(),
                    });
                }
            }
        }

        for props_file in [
            "src/main/resources/application.properties",
            "application.properties",
        ] {
            let path = project_path.join(props_file);
            if let Ok(props) = std::fs::read_to_string(&path) {
                report.add_generated("rustjvm.toml", converter::convert_properties(&props));
                break;
            }
        }

        report
    }
}

/// Full conversion: writes the migrated project into `to`.
/// Returns the report (including the README-MIGRATION.md contents).
pub fn convert_project(from: &Path, to: &Path) -> std::io::Result<MigrationReport> {
    let mut report = SpringBootAnalyzer::analyze(from);

    for java_file in find_files(from, "java") {
        let rel = relative(from, &java_file);
        let source = std::fs::read_to_string(&java_file)?;
        let converted = converter::convert_source(&source);
        let dest = to.join(&rel);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&dest, converted.source)?;
    }

    // Materialize generated files (rustjvm.toml).
    let generated = report.generated.clone();
    for g in &generated {
        let dest = to.join(&g.path);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&dest, &g.contents)?;
    }

    let readme = migration_readme(&report);
    std::fs::write(to.join("README-MIGRATION.md"), &readme)?;
    report.add_generated("README-MIGRATION.md", readme);

    Ok(report)
}

fn migration_readme(report: &MigrationReport) -> String {
    let mut out = String::from("# Migration to RustJVM\n\n");
    out.push_str("## What was done automatically\n\n");
    out.push_str(&report.to_markdown());
    out.push_str("\n## Manual steps remaining\n\n");
    let mut any = false;
    for f in report.by_compat(Compat::Partial).chain(report.by_compat(Compat::Unsupported)) {
        out.push_str(&format!("- **{}**: {}\n", f.feature, f.note));
        any = true;
    }
    if !any {
        out.push_str("None — this project converted cleanly.\n");
    }
    out.push_str("\n## Run it\n\n```bash\nrustjvm run --src <this-directory>/src/main/java\n```\n");
    out
}

pub fn find_files(root: &Path, ext: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    find_files_into(root, ext, &mut out);
    out.sort();
    out
}

fn find_files_into(dir: &Path, ext: &str, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        // Skip build output and VCS directories.
        if path.is_dir() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if matches!(name, "target" | "build" | ".git" | ".idea" | "node_modules") {
                continue;
            }
            find_files_into(&path, ext, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some(ext) {
            out.push(path);
        }
    }
}

fn relative(root: &Path, path: &Path) -> PathBuf {
    path.strip_prefix(root).unwrap_or(path).to_path_buf()
}
