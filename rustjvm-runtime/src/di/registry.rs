use rustjvm_compiler::{
    BeanContext, BeanKind, BeanSpec, ClassDecl, CompiledMethod, EvalError, Scope,
};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DIError {
    #[error("circular dependency: {0}")]
    Cycle(String),
    #[error("no bean of type '{0}'")]
    NoSuchBean(String),
    #[error("multiple beans of type '{0}': {1}")]
    Ambiguous(String, String),
    #[error("duplicate bean name '{0}'")]
    Duplicate(String),
    #[error("scope violation: cannot inject {scope:?}-scoped bean '{id}' into a singleton")]
    ScopeViolation { id: String, scope: Scope },
}

/// A Rust-native method implementation — the zero-copy bridge for
/// Java-to-Rust injection (e.g. a Rust VectorStore injected into a Java
/// @Service). No JNI, no serialization: plain `Fn` over string args.
pub type NativeFn = Arc<dyn Fn(&[String]) -> Result<String, EvalError> + Send + Sync>;

/// A bean implemented in Rust, registered directly with the container.
pub struct NativeDef {
    pub name: String,
    pub class_name: String,
    pub methods: HashMap<String, NativeFn>,
}

enum BeanCore {
    Java(HashMap<String, CompiledMethod>),
    Native(HashMap<String, NativeFn>),
}

/// A wired bean instance. Singletons are built once at boot and shared
/// immutably; prototypes and request-scoped beans are constructed from their
/// definitions on demand. Hot-reload swaps the whole registry atomically.
pub struct Bean {
    pub name: String,
    pub class_name: String,
    pub kind: BeanKind,
    pub scope: Scope,
    core: BeanCore,
    /// Field name → injected bean.
    fields: HashMap<String, Arc<Bean>>,
}

impl std::fmt::Debug for Bean {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Bean")
            .field("name", &self.name)
            .field("class_name", &self.class_name)
            .field("scope", &self.scope)
            .field("fields", &self.fields.keys().collect::<Vec<_>>())
            .finish()
    }
}

impl Bean {
    /// Invokes one of this bean's methods with positional arguments.
    pub fn invoke(&self, method: &str, args: &[String], depth: usize) -> Result<String, EvalError> {
        match &self.core {
            BeanCore::Java(methods) => {
                let m = methods.get(method).ok_or_else(|| {
                    EvalError::Unimplemented(format!("{}.{method}: no such method", self.class_name))
                })?;
                if args.len() != m.params.len() {
                    return Err(EvalError::Unimplemented(format!(
                        "{}.{method}: expected {} arg(s), got {}",
                        self.class_name,
                        m.params.len(),
                        args.len()
                    )));
                }
                let bound: HashMap<String, String> = m
                    .params
                    .iter()
                    .cloned()
                    .zip(args.iter().cloned())
                    .collect();
                m.implementation.eval_with_ctx(&bound, self, depth)
            }
            BeanCore::Native(methods) => {
                let f = methods.get(method).ok_or_else(|| {
                    EvalError::Unimplemented(format!(
                        "{}.{method}: no such native method",
                        self.class_name
                    ))
                })?;
                f(args)
            }
        }
    }

    /// The bean injected into a given field, if any.
    pub fn injected(&self, field: &str) -> Option<Arc<Bean>> {
        self.fields.get(field).cloned()
    }
}

impl BeanContext for Bean {
    fn call(
        &self,
        receiver: &str,
        method: &str,
        args: &[String],
        depth: usize,
    ) -> Result<String, EvalError> {
        let target = self.fields.get(receiver).ok_or_else(|| {
            EvalError::Unimplemented(format!(
                "{}: no injected field named '{receiver}'",
                self.class_name
            ))
        })?;
        target.invoke(method, args, depth)
    }
}

/// The bean registry. Reads are lock-free snapshots; LiveRust replaces the
/// whole registry via `ArcSwap`, so in-flight requests finish against the
/// wiring they started with.
pub struct BeanRegistry {
    singletons: HashMap<String, Arc<Bean>>,
    /// All Java bean definitions — the construction recipes for prototype
    /// and request scopes, and the hot-reload rebuild input.
    definitions: HashMap<String, BeanSpec>,
    /// Simple class name → bean names (a list: two packages may declare the
    /// same simple name; injection then fails as ambiguous).
    by_type: HashMap<String, Vec<String>>,
}

impl std::fmt::Debug for BeanRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BeanRegistry")
            .field("singletons", &self.singletons.keys().collect::<Vec<_>>())
            .field("definitions", &self.definitions.keys().collect::<Vec<_>>())
            .finish()
    }
}

impl BeanRegistry {
    pub fn empty() -> Self {
        Self {
            singletons: HashMap::new(),
            definitions: HashMap::new(),
            by_type: HashMap::new(),
        }
    }

    pub fn get(&self, name: &str) -> Option<Arc<Bean>> {
        self.singletons.get(name).cloned()
    }

    pub fn get_by_type(&self, class_name: &str) -> Option<Arc<Bean>> {
        self.by_type
            .get(class_name)
            .and_then(|names| names.first())
            .and_then(|n| self.singletons.get(n).cloned())
    }

    pub fn len(&self) -> usize {
        self.singletons.len()
    }

    pub fn is_empty(&self) -> bool {
        self.singletons.is_empty() && self.definitions.is_empty()
    }

    fn definition(&self, name: &str) -> Option<&BeanSpec> {
        self.definitions.get(name)
    }

    fn names_of_type(&self, class_name: &str) -> Option<&Vec<String>> {
        self.by_type.get(class_name)
    }

    /// Builds a fully-wired registry: indexes names/types, validates every
    /// dependency (missing / ambiguous / cycles) up front, then constructs
    /// singletons in topological order. Prototype and request-scoped beans
    /// are validated but deferred to resolution time.
    pub fn build(specs: &[BeanSpec], natives: Vec<NativeDef>) -> Result<Self, DIError> {
        let mut seen = HashSet::new();
        for s in specs {
            if !seen.insert(s.name.clone()) {
                return Err(DIError::Duplicate(s.name.clone()));
            }
        }
        for n in &natives {
            if !seen.insert(n.name.clone()) {
                return Err(DIError::Duplicate(n.name.clone()));
            }
        }

        let mut by_type: HashMap<String, Vec<String>> = HashMap::new();
        for s in specs {
            by_type
                .entry(s.class_name.clone())
                .or_default()
                .push(s.name.clone());
        }
        for n in &natives {
            by_type
                .entry(n.class_name.clone())
                .or_default()
                .push(n.name.clone());
        }

        let definitions: HashMap<String, BeanSpec> =
            specs.iter().map(|s| (s.name.clone(), s.clone())).collect();

        // Validate + topologically sort the full dependency graph — every
        // scope, so broken wiring fails at boot, not first request.
        let mut dep_graph: HashMap<String, Vec<String>> = HashMap::new();
        for s in specs {
            let mut deps = Vec::new();
            for d in &s.dependencies {
                match by_type.get(&d.type_name).map(Vec::as_slice) {
                    None => return Err(DIError::NoSuchBean(d.type_name.clone())),
                    Some([single]) => deps.push(single.clone()),
                    Some(many) => {
                        return Err(DIError::Ambiguous(d.type_name.clone(), many.join(", ")))
                    }
                }
            }
            dep_graph.insert(s.name.clone(), deps);
        }
        let order = topological_order(&dep_graph)?;

        // Native beans are ready-made singletons.
        let mut singletons: HashMap<String, Arc<Bean>> = natives
            .into_iter()
            .map(|n| {
                (
                    n.name.clone(),
                    Arc::new(Bean {
                        name: n.name,
                        class_name: n.class_name,
                        kind: BeanKind::Component,
                        scope: Scope::Singleton,
                        core: BeanCore::Native(n.methods),
                        fields: HashMap::new(),
                    }),
                )
            })
            .collect();

        // Construct singletons dependency-first. Prototype deps are built
        // fresh inline; request-scoped deps are a boot-time scope violation.
        for name in &order {
            // Natives appear in the graph (as dependency targets) but are
            // already constructed above.
            let Some(spec) = definitions.get(name) else {
                continue;
            };
            if spec.scope != Scope::Singleton {
                continue;
            }
            let bean = construct_bean(spec, &definitions, &by_type, &singletons, None)?;
            singletons.insert(name.clone(), bean);
        }

        Ok(Self {
            singletons,
            definitions,
            by_type,
        })
    }
}

/// Per-request resolution context. Request-scoped beans are constructed once
/// per request, cached here, and dropped with the request's arena — en masse
/// reclamation, no GC.
pub struct RequestContext<'a> {
    pub arena: &'a crate::arena::RequestArena,
    registry: &'a BeanRegistry,
    local: RefCell<HashMap<String, Arc<Bean>>>,
}

impl<'a> RequestContext<'a> {
    pub fn new(arena: &'a crate::arena::RequestArena, registry: &'a BeanRegistry) -> Self {
        Self {
            arena,
            registry,
            local: RefCell::new(HashMap::new()),
        }
    }

    pub fn resolve(&self, name: &str) -> Result<Arc<Bean>, DIError> {
        if let Some(b) = self.registry.get(name) {
            return Ok(b);
        }
        let spec = self
            .registry
            .definition(name)
            .ok_or_else(|| DIError::NoSuchBean(name.to_string()))?;
        match spec.scope {
            Scope::Singleton => Err(DIError::NoSuchBean(name.to_string())),
            Scope::Prototype => construct_bean(
                spec,
                &self.registry.definitions,
                &self.registry.by_type,
                &self.registry.singletons,
                Some(&self.local),
            ),
            Scope::Request => {
                if let Some(b) = self.local.borrow().get(name) {
                    return Ok(b.clone());
                }
                let bean = construct_bean(
                    spec,
                    &self.registry.definitions,
                    &self.registry.by_type,
                    &self.registry.singletons,
                    Some(&self.local),
                )?;
                self.local.borrow_mut().insert(name.to_string(), bean.clone());
                Ok(bean)
            }
        }
    }

    pub fn resolve_by_type(&self, class_name: &str) -> Result<Arc<Bean>, DIError> {
        match self.registry.names_of_type(class_name).map(Vec::as_slice) {
            None => Err(DIError::NoSuchBean(class_name.to_string())),
            Some([single]) => self.resolve(single),
            Some(many) => Err(DIError::Ambiguous(class_name.to_string(), many.join(", "))),
        }
    }
}

/// Constructs one bean from its definition, resolving each `@Autowired`
/// dependency by type. `local` is the request cache — `None` at boot, where
/// hitting a request-scoped dependency is a scope violation.
fn construct_bean(
    spec: &BeanSpec,
    definitions: &HashMap<String, BeanSpec>,
    by_type: &HashMap<String, Vec<String>>,
    singletons: &HashMap<String, Arc<Bean>>,
    local: Option<&RefCell<HashMap<String, Arc<Bean>>>>,
) -> Result<Arc<Bean>, DIError> {
    let mut fields = HashMap::new();
    for d in &spec.dependencies {
        let dep_name = match by_type.get(&d.type_name).map(Vec::as_slice) {
            None => return Err(DIError::NoSuchBean(d.type_name.clone())),
            Some([single]) => single.clone(),
            Some(many) => {
                return Err(DIError::Ambiguous(d.type_name.clone(), many.join(", ")))
            }
        };
        let dep = if let Some(b) = singletons.get(&dep_name) {
            b.clone()
        } else {
            let dep_spec = definitions
                .get(&dep_name)
                .ok_or_else(|| DIError::NoSuchBean(dep_name.clone()))?;
            match dep_spec.scope {
                Scope::Singleton => {
                    return Err(DIError::NoSuchBean(format!(
                        "{} (singleton not yet constructed — graph order bug)",
                        dep_name
                    )))
                }
                Scope::Prototype => {
                    construct_bean(dep_spec, definitions, by_type, singletons, local)?
                }
                Scope::Request => {
                    let cache = local.ok_or_else(|| DIError::ScopeViolation {
                        id: dep_name.clone(),
                        scope: Scope::Request,
                    })?;
                    if let Some(b) = cache.borrow().get(&dep_name) {
                        b.clone()
                    } else {
                        let b = construct_bean(
                            dep_spec, definitions, by_type, singletons, local,
                        )?;
                        cache.borrow_mut().insert(dep_name.clone(), b.clone());
                        b
                    }
                }
            }
        };
        fields.insert(d.field_name.clone(), dep);
    }
    Ok(Arc::new(Bean {
        name: spec.name.clone(),
        class_name: spec.class_name.clone(),
        kind: spec.kind,
        scope: spec.scope,
        core: BeanCore::Java(
            spec.methods
                .iter()
                .map(|m| (m.name.clone(), m.clone()))
                .collect(),
        ),
        fields,
    }))
}

#[derive(Clone, Copy, PartialEq)]
enum Mark {
    Visiting,
    Done,
}

fn topological_order(graph: &HashMap<String, Vec<String>>) -> Result<Vec<String>, DIError> {
    let mut marks: HashMap<&str, Mark> = HashMap::new();
    let mut order = Vec::new();
    let mut stack: Vec<&str> = Vec::new();

    fn visit<'a>(
        node: &'a str,
        graph: &'a HashMap<String, Vec<String>>,
        marks: &mut HashMap<&'a str, Mark>,
        order: &mut Vec<String>,
        stack: &mut Vec<&'a str>,
    ) -> Result<(), DIError> {
        match marks.get(node) {
            Some(Mark::Done) => return Ok(()),
            Some(Mark::Visiting) => {
                let mut path: Vec<&str> = stack
                    .iter()
                    .skip_while(|&&n| n != node)
                    .copied()
                    .collect();
                path.push(node);
                return Err(DIError::Cycle(path.join(" -> ")));
            }
            None => {}
        }
        marks.insert(node, Mark::Visiting);
        stack.push(node);
        if let Some(deps) = graph.get(node) {
            for dep in deps {
                visit(dep, graph, marks, order, stack)?;
            }
        }
        stack.pop();
        marks.insert(node, Mark::Done);
        order.push(node.to_string());
        Ok(())
    }

    for name in graph.keys() {
        visit(name, graph, &mut marks, &mut order, &mut stack)?;
    }
    Ok(order)
}

/// Assembles a registry from per-file scan output: expands `@Bean` factory
/// specs against the class index, applies the `@ComponentScan` filter, then
/// builds. Shared by the CLI (boot) and LiveRust (rebuild).
pub fn assemble_registry(
    files: &[(Vec<BeanSpec>, Vec<ClassDecl>)],
    natives: Vec<NativeDef>,
) -> Result<BeanRegistry, DIError> {
    let class_index: HashMap<&str, &ClassDecl> = files
        .iter()
        .flat_map(|(_, classes)| classes.iter())
        .map(|c| (c.name.as_str(), c))
        .collect();

    let mut specs: Vec<BeanSpec> = files
        .iter()
        .flat_map(|(beans, _)| beans.iter().cloned())
        .collect();

    // Expand factory-method specs: methods and field-deps come from the
    // produced class, which need not carry any stereotype annotation.
    for spec in specs.iter_mut() {
        if matches!(spec.origin, rustjvm_compiler::BeanOrigin::FactoryMethod { .. }) {
            let produced = class_index
                .get(spec.class_name.as_str())
                .ok_or_else(|| DIError::NoSuchBean(spec.class_name.clone()))?;
            rustjvm_compiler::expand_factory_bean(spec, produced);
        }
    }

    // @ComponentScan filter — no scan annotation anywhere means accept all.
    let bases = rustjvm_compiler::scan_base_packages(&specs);
    if let Some(bases) = &bases {
        specs.retain(|s| rustjvm_compiler::under_scan_bases(&s.package, &Some(bases.clone())));
    }

    BeanRegistry::build(&specs, natives)
}
