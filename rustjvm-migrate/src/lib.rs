//! `rustjvm-migrate` — Spring Boot → RustJVM migration tooling.
//!
//! - [`SpringBootAnalyzer::analyze`] scans a Spring Boot project and reports
//!   what converts cleanly, what needs attention, and what is unsupported.
//! - [`convert_project`] writes a migrated copy of the project: annotations
//!   rewritten, `rustjvm.toml` generated, README-MIGRATION.md with the
//!   remaining manual steps.

mod analyzer;
mod converter;
mod report;

pub use analyzer::{convert_project, find_files, SpringBootAnalyzer};
pub use converter::{convert_properties, convert_source, SourceConversion};
pub use report::{Compat, Conversion, Finding, GeneratedFile, MigrationReport};
