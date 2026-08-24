//! Migration report model and rendering.

use std::fmt::Write as _;
use std::path::PathBuf;

/// A feature found in the Spring Boot project, with its RustJVM fate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    /// What was found, e.g. "spring-boot-starter-web" or "@RestController".
    pub feature: String,
    /// Where it was found (file or pom.xml).
    pub location: PathBuf,
    /// Compatibility verdict.
    pub compat: Compat,
    /// What to do about it (required for Partial/Unsupported).
    pub note: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Compat {
    /// Works on RustJVM as-is or with automated conversion.
    Compatible,
    /// Partially supported — needs manual attention.
    Partial,
    /// Not supported — an alternative must be chosen.
    Unsupported,
}

/// A source edit the converter will apply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conversion {
    pub file: PathBuf,
    /// e.g. "@SpringBootApplication → @RustJVMApplication"
    pub description: String,
}

/// A file the converter generates (rustjvm.toml, README-MIGRATION.md, ...).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedFile {
    pub path: PathBuf,
    pub contents: String,
}

#[derive(Debug, Default)]
pub struct MigrationReport {
    pub findings: Vec<Finding>,
    pub conversions: Vec<Conversion>,
    pub generated: Vec<GeneratedFile>,
}

impl MigrationReport {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_finding(&mut self, finding: Finding) {
        // One verdict per feature kind is enough for a readable report.
        if self
            .findings
            .iter()
            .any(|f| f.feature == finding.feature && f.compat == finding.compat)
        {
            return;
        }
        self.findings.push(finding);
    }

    pub fn add_conversion(&mut self, conversion: Conversion) {
        self.conversions.push(conversion);
    }

    pub fn add_generated(&mut self, path: impl Into<PathBuf>, contents: impl Into<String>) {
        self.generated.push(GeneratedFile {
            path: path.into(),
            contents: contents.into(),
        });
    }

    pub fn by_compat(&self, compat: Compat) -> impl Iterator<Item = &Finding> {
        self.findings.iter().filter(move |f| f.compat == compat)
    }

    /// Human-readable markdown report.
    pub fn to_markdown(&self) -> String {
        let mut out = String::new();
        out.push_str("# RustJVM Migration Report\n\n");

        let mut section = |title: &str, compat: Compat, marker: &str| {
            let items: Vec<_> = self.by_compat(compat).collect();
            let _ = writeln!(out, "## {title} ({})", items.len());
            for f in items {
                let _ = writeln!(
                    out,
                    "- {marker} `{}` — {} _(in {})_",
                    f.feature,
                    f.note,
                    f.location.display()
                );
            }
            out.push('\n');
        };

        section("Fully compatible", Compat::Compatible, "✅");
        section("Needs attention", Compat::Partial, "⚠️");
        section("Unsupported", Compat::Unsupported, "❌");

        if !self.conversions.is_empty() {
            let _ = writeln!(out, "## Automated conversions ({})", self.conversions.len());
            for c in &self.conversions {
                let _ = writeln!(out, "- `{}`: {}", c.file.display(), c.description);
            }
            out.push('\n');
        }

        if !self.generated.is_empty() {
            let _ = writeln!(out, "## Generated files ({})", self.generated.len());
            for g in &self.generated {
                let _ = writeln!(out, "- `{}`", g.path.display());
            }
        }

        out
    }
}
