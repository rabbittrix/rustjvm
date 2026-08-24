//! rustjvm-compiler — Java source → RustJVM route IR + bean metadata.
//!
//! Phase 1 parses Java *source* (bytecode → native translation arrives in a
//! later phase) and compiles the `@RestController` subset into
//! [`CompiledRoute`]s. Phase 2 adds bean extraction (`@Service`,
//! `@Configuration`, `@Component`) and `@Autowired` wiring metadata for the
//! runtime's DI container.

mod ast;
mod compile;
mod lexer;
mod parser;

pub use ast::{
    AnnArg, Annotation, CallArg, ClassDecl, Expr, FieldDecl, JavaFile, Member, MethodDecl,
    Operand, Param,
};
pub use compile::{
    compile_routes, expand_factory_bean, extract_beans, scan_base_packages, under_scan_bases,
    BeanContext, BeanKind, BeanOrigin, BeanSpec, CompiledMethod, CompiledRoute, DependencySpec,
    EvalError, MethodImpl, NoBeans, ParamBinding, Part, Scope,
};
pub use lexer::LexError;
pub use parser::{parse_source, ParseError};

/// Parse a Java source file and compile every route it declares.
pub fn compile_source(src: &str) -> Result<Vec<CompiledRoute>, ParseError> {
    let file = parse_source(src)?;
    Ok(compile_routes(&file))
}

/// Everything one source file contributes to the application.
#[derive(Debug, Clone, Default)]
pub struct Analysis {
    pub routes: Vec<CompiledRoute>,
    pub beans: Vec<BeanSpec>,
    /// All parsed classes — the DI assembler needs unannotated ones too
    /// (they can be produced by @Bean factory methods).
    pub classes: Vec<ClassDecl>,
}

/// Full analysis: routes, bean definitions, and raw class declarations.
pub fn analyze_source(src: &str) -> Result<Analysis, ParseError> {
    let file = parse_source(src)?;
    Ok(Analysis {
        routes: compile_routes(&file),
        beans: extract_beans(&file),
        classes: file.classes,
    })
}
