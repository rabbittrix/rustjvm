use crate::ast::{Annotation, CallArg, ClassDecl, Expr, JavaFile, MethodDecl, Operand, Param};
use std::collections::HashMap;
use thiserror::Error;

/// A route ready to be installed into the runtime's dispatch table.
#[derive(Debug, Clone)]
pub struct CompiledRoute {
    pub http_method: String,
    pub path: String,
    pub class_name: String,
    pub method_name: String,
    /// Declaring class's package — used by @ComponentScan filtering.
    pub package: Option<String>,
    pub params: Vec<ParamBinding>,
    pub implementation: MethodImpl,
}

#[derive(Debug, Clone)]
pub struct ParamBinding {
    /// Java parameter name used inside the method body.
    pub param: String,
    /// Query-string key it is bound from.
    pub query_key: String,
    pub required: bool,
}

/// The executable form of a Java method body. Swapping one of these into the
/// dispatch table is the LiveRust unit of hot-reload.
#[derive(Debug, Clone)]
pub enum MethodImpl {
    /// `return "constant";` (or a literal-only concatenation).
    Constant(String),
    /// `return "a" + param + service.call(x) + "b";`
    Template(Vec<Part>),
    /// Declared but outside the interpreter subset (complex body,
    /// `return null`, annotation-driven stubs like `@Prompt`). Serves HTTP
    /// 501 until a later phase compiles it for real.
    Unimplemented(String),
}

#[derive(Debug, Clone)]
pub enum Part {
    Lit(String),
    Param(String),
    /// A call on an injected field, resolved through the bean context.
    Call {
        receiver: String,
        method: String,
        args: Vec<CallArg>,
    },
}

#[derive(Debug, Error)]
pub enum EvalError {
    #[error("missing required parameter '{0}'")]
    MissingParam(String),
    #[error("{0}")]
    Unimplemented(String),
}

/// How a method under evaluation reaches other beans. The runtime's DI
/// registry implements this; the compiler crate stays dependency-free.
pub trait BeanContext {
    fn call(
        &self,
        receiver: &str,
        method: &str,
        args: &[String],
        depth: usize,
    ) -> Result<String, EvalError>;
}

/// Evaluation context with no beans — plain routes never fail to compile
/// just because DI isn't wired.
pub struct NoBeans;

impl BeanContext for NoBeans {
    fn call(
        &self,
        receiver: &str,
        method: &str,
        _args: &[String],
        _depth: usize,
    ) -> Result<String, EvalError> {
        Err(EvalError::Unimplemented(format!(
            "{receiver}.{method}(...): no bean context available"
        )))
    }
}

/// Guard against unbounded recursion through mutually-calling beans.
const MAX_CALL_DEPTH: usize = 64;

impl MethodImpl {
    /// Evaluates without a bean context (plain controllers, tests).
    pub fn eval(&self, args: &HashMap<String, String>) -> Result<String, EvalError> {
        self.eval_with_ctx(args, &NoBeans, 0)
    }

    pub fn eval_with_ctx(
        &self,
        args: &HashMap<String, String>,
        ctx: &dyn BeanContext,
        depth: usize,
    ) -> Result<String, EvalError> {
        if depth > MAX_CALL_DEPTH {
            return Err(EvalError::Unimplemented(
                "bean call depth limit exceeded (circular call chain?)".into(),
            ));
        }
        match self {
            MethodImpl::Constant(s) => Ok(s.clone()),
            MethodImpl::Template(parts) => {
                let mut out = String::new();
                for part in parts {
                    match part {
                        Part::Lit(s) => out.push_str(s),
                        Part::Param(name) => {
                            let value = args
                                .get(name)
                                .ok_or_else(|| EvalError::MissingParam(name.clone()))?;
                            out.push_str(value);
                        }
                        Part::Call {
                            receiver,
                            method,
                            args: call_args,
                        } => {
                            let mut resolved = Vec::with_capacity(call_args.len());
                            for a in call_args {
                                match a {
                                    CallArg::Lit(s) => resolved.push(s.clone()),
                                    CallArg::Var(p) => resolved.push(
                                        args.get(p)
                                            .ok_or_else(|| EvalError::MissingParam(p.clone()))?
                                            .clone(),
                                    ),
                                }
                            }
                            out.push_str(&ctx.call(receiver, method, &resolved, depth + 1)?);
                        }
                    }
                }
                Ok(out)
            }
            MethodImpl::Unimplemented(why) => Err(EvalError::Unimplemented(why.clone())),
        }
    }
}

// ---------------------------------------------------------------------------
// Beans
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BeanKind {
    Controller,
    Service,
    Component,
    Configuration,
}

/// Bean lifecycle scope, from `@Scope("...")` / `@Bean(scope = "...")`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// One instance, built at boot, shared by all requests.
    Singleton,
    /// A fresh instance at every injection point / resolution.
    Prototype,
    /// One instance per HTTP request, cached in the request's arena context.
    Request,
}

/// Where a bean definition came from.
#[derive(Debug, Clone)]
pub enum BeanOrigin {
    /// A stereotype-annotated class (@Service, @Component, ...).
    Class,
    /// An `@Bean` factory method on a configuration class. The produced
    /// bean's methods/fields are expanded from the returned class later.
    FactoryMethod { config_class: String, method: String },
}

/// Everything the runtime's DI container needs to instantiate and wire one
/// bean, extracted from a single class.
#[derive(Debug, Clone)]
pub struct BeanSpec {
    /// Bean name: explicit annotation value, @Bean method name, or the
    /// Spring-style default (decapitalized class name).
    pub name: String,
    pub class_name: String,
    pub package: Option<String>,
    pub kind: BeanKind,
    pub scope: Scope,
    pub origin: BeanOrigin,
    pub methods: Vec<CompiledMethod>,
    pub dependencies: Vec<DependencySpec>,
    /// Set when the class carries @ComponentScan / @RustJVMApplication:
    /// `Some(bases)`; empty `bases` means "the class's own package".
    pub component_scan: Option<Vec<String>>,
}

#[derive(Debug, Clone)]
pub struct CompiledMethod {
    pub name: String,
    pub params: Vec<String>,
    pub implementation: MethodImpl,
}

/// An `@Autowired` field: inject the bean whose type matches `type_name`.
#[derive(Debug, Clone)]
pub struct DependencySpec {
    pub field_name: String,
    pub type_name: String,
}

pub fn extract_beans(file: &JavaFile) -> Vec<BeanSpec> {
    let mut out = Vec::new();
    for class in &file.classes {
        let find = |name: &str| class.annotations.iter().find(|a| a.name == name);

        let component_scan = if let Some(app) = find("RustJVMApplication") {
            // Meta-annotated with @ComponentScan; may override explicitly.
            Some(scan_bases_from(app).unwrap_or_default())
        } else {
            find("ComponentScan").map(|cs| {
                scan_bases_from(cs).unwrap_or_default()
            })
        };

        let kind = class.annotations.iter().find_map(|a| match a.name.as_str() {
            "RestController" => Some(BeanKind::Controller),
            "Service" => Some(BeanKind::Service),
            "Component" => Some(BeanKind::Component),
            "Configuration" => Some(BeanKind::Configuration),
            "RustJVMApplication" => Some(BeanKind::Configuration),
            _ => None,
        });

        if let Some(kind) = kind {
            // Explicit name wins: @Service("mail") / @Component("mail").
            let stereotype = class.annotations.iter().find(|a| {
                matches!(
                    a.name.as_str(),
                    "RestController" | "Service" | "Component" | "Configuration"
                )
            });
            let name = stereotype
                .and_then(|a| a.first_value().or_else(|| a.named("value")))
                .filter(|v| !v.is_empty())
                .unwrap_or_else(|| decapitalize(&class.name));
            let scope = find("Scope")
                .and_then(|s| s.first_value().or_else(|| s.named("value")))
                .map(|v| parse_scope(&v))
                .unwrap_or(Scope::Singleton);

            out.push(BeanSpec {
                name,
                class_name: class.name.clone(),
                package: file.package.clone(),
                kind,
                scope,
                origin: BeanOrigin::Class,
                methods: class
                    .methods
                    .iter()
                    .map(|m| CompiledMethod {
                        name: m.name.clone(),
                        params: m.params.iter().map(|p| p.name.clone()).collect(),
                        implementation: compile_body(class, m),
                    })
                    .collect(),
                dependencies: class
                    .fields
                    .iter()
                    .filter(|f| f.is_injection_point())
                    .map(|f| DependencySpec {
                        field_name: f.name.clone(),
                        type_name: f.ty.clone(),
                    })
                    .collect(),
                component_scan,
            });
        }

        // @Bean factory methods — honored on any scanned stereotype class.
        if kind.is_some() {
            for m in &class.methods {
                let Some(bean_ann) = m.annotations.iter().find(|a| a.name == "Bean") else {
                    continue;
                };
                let name = bean_ann
                    .named("name")
                    .or_else(|| bean_ann.first_value())
                    .filter(|v| !v.is_empty())
                    .unwrap_or_else(|| m.name.clone());
                let scope = bean_ann
                    .named("scope")
                    .map(|v| parse_scope(&v))
                    .unwrap_or(Scope::Singleton);
                out.push(BeanSpec {
                    name,
                    class_name: m.return_type.clone(),
                    package: file.package.clone(),
                    kind: BeanKind::Component,
                    scope,
                    origin: BeanOrigin::FactoryMethod {
                        config_class: class.name.clone(),
                        method: m.name.clone(),
                    },
                    // Expanded from the produced class during registry assembly.
                    methods: Vec::new(),
                    // Factory-method parameters are container-provided deps.
                    dependencies: m
                        .params
                        .iter()
                        .map(|p| DependencySpec {
                            field_name: p.name.clone(),
                            type_name: p.ty.clone(),
                        })
                        .collect(),
                    component_scan: None,
                });
            }
        }
    }
    out
}

fn scan_bases_from(ann: &Annotation) -> Option<Vec<String>> {
    let mut bases = ann.named_all("basePackages");
    if bases.is_empty() {
        bases = ann.values();
    }
    if bases.is_empty() {
        None
    } else {
        Some(bases)
    }
}

fn parse_scope(value: &str) -> Scope {
    match value.to_ascii_lowercase().as_str() {
        "prototype" => Scope::Prototype,
        "request" => Scope::Request,
        _ => Scope::Singleton,
    }
}

/// Base packages declared via @ComponentScan / @RustJVMApplication across the
/// scanned specs. `None` means "no scan annotation anywhere — accept all".
pub fn scan_base_packages(specs: &[BeanSpec]) -> Option<Vec<String>> {
    let mut bases = Vec::new();
    for s in specs {
        if let Some(cs) = &s.component_scan {
            if cs.is_empty() {
                if let Some(p) = &s.package {
                    bases.push(p.clone());
                }
            } else {
                bases.extend(cs.iter().cloned());
            }
        }
    }
    if bases.is_empty() {
        None
    } else {
        bases.sort();
        bases.dedup();
        Some(bases)
    }
}

/// Whether a package passes the scan filter (no filter → everything passes).
pub fn under_scan_bases(package: &Option<String>, bases: &Option<Vec<String>>) -> bool {
    match bases {
        None => true,
        Some(bases) => package
            .as_ref()
            .map(|p| bases.iter().any(|b| p.starts_with(b.as_str())))
            .unwrap_or(false),
    }
}

/// Expands a `@Bean` factory-method spec from the produced class's
/// declaration: its methods become the bean's methods, its `@Autowired`
/// fields become additional dependencies.
pub fn expand_factory_bean(spec: &mut BeanSpec, produced: &ClassDecl) {
    spec.methods = produced
        .methods
        .iter()
        .map(|m| CompiledMethod {
            name: m.name.clone(),
            params: m.params.iter().map(|p| p.name.clone()).collect(),
            implementation: compile_body(produced, m),
        })
        .collect();
    let known: std::collections::HashSet<&str> = spec
        .dependencies
        .iter()
        .map(|d| d.type_name.as_str())
        .collect();
    let extra: Vec<DependencySpec> = produced
        .fields
        .iter()
        .filter(|f| f.is_injection_point() && !known.contains(f.ty.as_str()))
        .map(|f| DependencySpec {
            field_name: f.name.clone(),
            type_name: f.ty.clone(),
        })
        .collect();
    spec.dependencies.extend(extra);
}

fn decapitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_lowercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

// ---------------------------------------------------------------------------
// Routes
// ---------------------------------------------------------------------------

pub fn compile_routes(file: &JavaFile) -> Vec<CompiledRoute> {
    let mut routes = Vec::new();
    for class in &file.classes {
        if !class.annotations.iter().any(|a| a.name == "RestController") {
            continue;
        }
        let base = class
            .annotations
            .iter()
            .find(|a| a.name == "RequestMapping")
            .and_then(|a| a.first_value().or_else(|| a.named("value")))
            .unwrap_or_default();

        for method in &class.methods {
            for ann in &method.annotations {
                let http_method = match ann.name.as_str() {
                    "GetMapping" => "GET",
                    "PostMapping" => "POST",
                    "PutMapping" => "PUT",
                    "DeleteMapping" => "DELETE",
                    "PatchMapping" => "PATCH",
                    "RequestMapping" => "ANY",
                    _ => continue,
                };
                let sub = ann
                    .first_value()
                    .or_else(|| ann.named("value"))
                    .unwrap_or_default();
                routes.push(CompiledRoute {
                    http_method: http_method.to_string(),
                    path: normalize_path(&base, &sub),
                    class_name: class.name.clone(),
                    method_name: method.name.clone(),
                    package: file.package.clone(),
                    params: method.params.iter().map(bind_param).collect(),
                    implementation: compile_body(class, method),
                });
            }
        }
    }
    routes
}

fn bind_param(p: &Param) -> ParamBinding {
    match p.annotations.iter().find(|a| a.name == "RequestParam") {
        Some(ann) => {
            let query_key = ann
                .first_value()
                .or_else(|| ann.named("value"))
                .filter(|v| !v.is_empty())
                .unwrap_or_else(|| p.name.clone());
            let required = ann.named("required").map(|v| v != "false").unwrap_or(true);
            ParamBinding {
                param: p.name.clone(),
                query_key,
                required,
            }
        }
        // Unannotated parameters bind to the query string by name.
        None => ParamBinding {
            param: p.name.clone(),
            query_key: p.name.clone(),
            required: false,
        },
    }
}

fn compile_body(class: &ClassDecl, m: &MethodDecl) -> MethodImpl {
    let qualify = |reason: &str| format!("{}.{}: {}", class.name, m.name, reason);
    match &m.body {
        None => MethodImpl::Unimplemented(qualify("no method body")),
        Some(Expr::None) => MethodImpl::Unimplemented(qualify("body outside the interpreter subset")),
        Some(Expr::Concat(ops)) => {
            // Soundness gates — refuse rather than evaluate incorrectly:
            for op in ops {
                match op {
                    // A `Var` must be a method parameter bound from the
                    // request (or call site); anything else is a local
                    // computed by code we don't interpret.
                    Operand::Var(v) if !m.params.iter().any(|p| &p.name == v) => {
                        return MethodImpl::Unimplemented(qualify(&format!(
                            "'{v}' is a local variable outside the interpreter subset"
                        )));
                    }
                    // A call receiver must be an @Autowired field — that's
                    // what the DI container can inject and route through.
                    Operand::Call { receiver, args, .. } => {
                        let injectable = class
                            .fields
                            .iter()
                            .any(|f| &f.name == receiver && f.is_injection_point());
                        if !injectable {
                            return MethodImpl::Unimplemented(qualify(&format!(
                                "'{receiver}' is not an @Autowired field"
                            )));
                        }
                        for a in args {
                            if let CallArg::Var(v) = a {
                                if !m.params.iter().any(|p| &p.name == v) {
                                    return MethodImpl::Unimplemented(qualify(&format!(
                                        "call argument '{v}' is not a method parameter"
                                    )));
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
            if ops.iter().all(|o| matches!(o, Operand::Lit(_))) {
                let s: String = ops
                    .iter()
                    .map(|o| match o {
                        Operand::Lit(s) => s.as_str(),
                        _ => unreachable!(),
                    })
                    .collect();
                MethodImpl::Constant(s)
            } else {
                MethodImpl::Template(
                    ops.iter()
                        .map(|o| match o {
                            Operand::Lit(s) => Part::Lit(s.clone()),
                            Operand::Var(v) => Part::Param(v.clone()),
                            Operand::Call {
                                receiver,
                                method,
                                args,
                            } => Part::Call {
                                receiver: receiver.clone(),
                                method: method.clone(),
                                args: args.clone(),
                            },
                        })
                        .collect(),
                )
            }
        }
    }
}

fn normalize_path(base: &str, sub: &str) -> String {
    let joined = format!("{}/{}", base.trim_end_matches('/'), sub.trim_start_matches('/'));
    let joined = joined.trim_end_matches('/');
    if joined.is_empty() {
        "/".to_string()
    } else if joined.starts_with('/') {
        joined.to_string()
    } else {
        format!("/{joined}")
    }
}
