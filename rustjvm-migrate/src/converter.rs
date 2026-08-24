//! Converters: `application.properties` → `rustjvm.toml`, and Spring
//! annotations/imports → RustJVM equivalents.

/// One annotation/import mapping rule.
struct Mapping {
    /// Fully-qualified Spring import or bare annotation name.
    from: &'static str,
    to: &'static str,
    import_to: Option<&'static str>,
}

const ANNOTATION_MAPPINGS: &[Mapping] = &[
    Mapping {
        from: "org.springframework.boot.autoconfigure.SpringBootApplication",
        to: "@RustJVMApplication",
        import_to: Some("rustjvm.spring.RustJVMApplication"),
    },
    Mapping {
        from: "org.springframework.stereotype.Service",
        to: "@Service",
        import_to: Some("rustjvm.spring.context.Service"),
    },
    Mapping {
        from: "org.springframework.stereotype.Component",
        to: "@Component",
        import_to: Some("rustjvm.spring.context.Component"),
    },
    Mapping {
        from: "org.springframework.beans.factory.annotation.Autowired",
        to: "@Autowired",
        import_to: Some("rustjvm.spring.context.Autowired"),
    },
    Mapping {
        from: "org.springframework.context.annotation.Configuration",
        to: "@Configuration",
        import_to: Some("rustjvm.spring.context.Configuration"),
    },
    Mapping {
        from: "org.springframework.context.annotation.Bean",
        to: "@Bean",
        import_to: Some("rustjvm.spring.context.Bean"),
    },
    Mapping {
        from: "org.springframework.context.annotation.Scope",
        to: "@Scope",
        import_to: Some("rustjvm.spring.context.Scope"),
    },
    Mapping {
        from: "org.springframework.context.annotation.ComponentScan",
        to: "@ComponentScan",
        import_to: Some("rustjvm.spring.context.ComponentScan"),
    },
    Mapping {
        from: "org.springframework.web.bind.annotation.RestController",
        to: "@RestController",
        import_to: Some("rustjvm.spring.web.RestController"),
    },
    Mapping {
        from: "org.springframework.web.bind.annotation.GetMapping",
        to: "@GetMapping",
        import_to: Some("rustjvm.spring.web.GetMapping"),
    },
    Mapping {
        from: "org.springframework.web.bind.annotation.PostMapping",
        to: "@PostMapping",
        import_to: Some("rustjvm.spring.web.PostMapping"),
    },
    Mapping {
        from: "org.springframework.web.bind.annotation.RequestMapping",
        to: "@RequestMapping",
        import_to: Some("rustjvm.spring.web.RequestMapping"),
    },
    Mapping {
        from: "org.springframework.web.bind.annotation.RequestParam",
        to: "@RequestParam",
        import_to: Some("rustjvm.spring.web.RequestParam"),
    },
];

/// What happened to one source file.
#[derive(Debug, Default)]
pub struct SourceConversion {
    pub source: String,
    pub changes: Vec<String>,
}

/// Converts one Java source file's Spring annotations/imports to RustJVM
/// equivalents. Unknown Spring annotations are left in place (the analyzer
/// flags them separately).
pub fn convert_source(src: &str) -> SourceConversion {
    let mut out = src.to_string();
    let mut changes = Vec::new();
    let mut imports_to_add: Vec<&str> = Vec::new();

    for m in ANNOTATION_MAPPINGS {
        let simple_from = m.from.rsplit('.').next().unwrap();
        let simple_to = m.to.trim_start_matches('@');

        let import_line = format!("import {};", m.from);
        let had_import = out.contains(&import_line);
        let uses_annotation = out.contains(&format!("@{simple_from}"));

        if !had_import && !uses_annotation {
            continue;
        }

        // 1. Rewrite the import (or schedule one to be added for star-imports).
        if had_import {
            out = out.replace(&import_line, &format!("import {};", m.import_to.unwrap()));
        } else if simple_from != simple_to {
            imports_to_add.push(m.import_to.unwrap());
        }

        // 2. Rewrite the annotation usage when the simple name differs
        //    (e.g. @SpringBootApplication → @RustJVMApplication).
        if simple_from != simple_to {
            out = replace_annotation(&out, simple_from, simple_to);
        }
        changes.push(format!(
            "{} (import {} → {})",
            m.to,
            m.from,
            m.import_to.unwrap()
        ));
    }

    for import in imports_to_add {
        out = insert_import(&out, import);
    }

    SourceConversion {
        source: out,
        changes,
    }
}

/// Replaces `@Old` with `@New` at usage sites (word-boundary safe: `@Service`
/// must not match `@ServiceBus`).
fn replace_annotation(src: &str, from: &str, to: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let needle = format!("@{from}");
    let mut rest = src;
    while let Some(idx) = rest.find(&needle) {
        let after = &rest[idx + needle.len()..];
        let boundary = after
            .chars()
            .next()
            .map(|c| !(c.is_alphanumeric() || c == '_'))
            .unwrap_or(true);
        out.push_str(&rest[..idx]);
        if boundary {
            out.push('@');
            out.push_str(to);
        } else {
            out.push_str(&needle);
        }
        rest = after;
    }
    out.push_str(rest);
    out
}

/// Inserts an import right after the `package ...;` line (or at the top).
fn insert_import(src: &str, import: &str) -> String {
    let line = format!("import {import};");
    let mut out: Vec<String> = Vec::new();
    let mut inserted = false;
    for l in src.lines() {
        out.push(l.to_string());
        if !inserted && l.trim_start().starts_with("package ") && l.trim_end().ends_with(';') {
            out.push(String::new());
            out.push(line.clone());
            inserted = true;
        }
    }
    if !inserted {
        out.insert(0, line);
    }
    let mut joined = out.join("\n");
    if src.ends_with('\n') && !joined.ends_with('\n') {
        joined.push('\n');
    }
    joined
}

/// Converts `application.properties` content into `rustjvm.toml` content.
/// Unmapped keys are preserved as commented TODOs so nothing is lost.
pub fn convert_properties(props: &str) -> String {
    let mut server = Vec::new();
    let mut database = Vec::new();
    let mut logging = Vec::new();
    let mut unmapped = Vec::new();

    for line in props.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with('!') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim();

        match key {
            "server.port" => server.push(format!("port = {value}")),
            "server.address" => server.push(format!("host = \"{value}\"")),
            "spring.datasource.url" => {
                // Strip the jdbc: prefix — RustJVM speaks the native protocol.
                let url = value.strip_prefix("jdbc:").unwrap_or(value);
                database.push(format!("url = \"{url}\""));
            }
            "spring.datasource.username" => {
                database.push(format!("username = \"{value}\""));
            }
            "spring.datasource.password" => {
                database.push(format!("password = \"{value}\""));
            }
            "spring.datasource.hikari.maximum-pool-size" => {
                database.push(format!("max_pool_size = {value}"));
            }
            "logging.level.root" => {
                logging.push(format!("level = \"{}\"", value.to_lowercase()));
            }
            "spring.application.name" => {
                server.push(format!("# application name: {value}"));
            }
            other => unmapped.push(format!("# TODO(migrate): {other} = {value}")),
        }
    }

    let mut toml = String::from("# Generated by rustjvm migrate convert\n\n");
    if !server.is_empty() {
        toml.push_str("[server]\n");
        for l in &server {
            toml.push_str(l);
            toml.push('\n');
        }
        toml.push('\n');
    }
    if !database.is_empty() {
        toml.push_str("[database]\n");
        for l in &database {
            toml.push_str(l);
            toml.push('\n');
        }
        toml.push('\n');
    }
    if !logging.is_empty() {
        toml.push_str("[logging]\n");
        for l in &logging {
            toml.push_str(l);
            toml.push('\n');
        }
        toml.push('\n');
    }
    if !unmapped.is_empty() {
        toml.push_str("# --- Unmapped properties (review manually) ---\n");
        for l in &unmapped {
            toml.push_str(l);
            toml.push('\n');
        }
    }
    toml
}
