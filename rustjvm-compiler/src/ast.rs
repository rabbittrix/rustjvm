#[derive(Debug, Clone, Default)]
pub struct JavaFile {
    pub package: Option<String>,
    pub classes: Vec<ClassDecl>,
}

#[derive(Debug, Clone)]
pub struct ClassDecl {
    pub name: String,
    pub annotations: Vec<Annotation>,
    pub methods: Vec<MethodDecl>,
    pub fields: Vec<FieldDecl>,
}

#[derive(Debug, Clone)]
pub struct FieldDecl {
    pub name: String,
    pub ty: String,
    pub annotations: Vec<Annotation>,
}

impl FieldDecl {
    /// `@Autowired` marks this field as a DI injection point.
    pub fn is_injection_point(&self) -> bool {
        self.annotations.iter().any(|a| a.name == "Autowired")
    }
}

#[derive(Debug, Clone)]
pub struct MethodDecl {
    pub name: String,
    pub return_type: String,
    pub params: Vec<Param>,
    pub annotations: Vec<Annotation>,
    /// First top-level `return` expression, if the body is simple enough for
    /// the Phase 1/2 interpreter.
    pub body: Option<Expr>,
}

#[derive(Debug, Clone)]
pub struct Param {
    pub name: String,
    pub ty: String,
    pub annotations: Vec<Annotation>,
}

/// Result of parsing one class member.
#[derive(Debug, Clone)]
pub enum Member {
    Method(MethodDecl),
    Field(FieldDecl),
    /// Initializer blocks, constructors, nested types — parsed past, not kept.
    Skipped,
}

#[derive(Debug, Clone)]
pub struct Annotation {
    /// Simple name (last segment of a possibly-qualified name).
    pub name: String,
    pub args: Vec<AnnArg>,
}

#[derive(Debug, Clone)]
pub enum AnnArg {
    Value(String),
    Named(String, String),
}

impl Annotation {
    pub fn first_value(&self) -> Option<String> {
        self.args.iter().find_map(|a| match a {
            AnnArg::Value(v) => Some(v.clone()),
            _ => None,
        })
    }

    /// All positional values — `@GetMapping({"/a", "/b"})` yields both.
    pub fn values(&self) -> Vec<String> {
        self.args
            .iter()
            .filter_map(|a| match a {
                AnnArg::Value(v) => Some(v.clone()),
                _ => None,
            })
            .collect()
    }

    pub fn named(&self, key: &str) -> Option<String> {
        self.args.iter().find_map(|a| match a {
            AnnArg::Named(k, v) if k == key => Some(v.clone()),
            _ => None,
        })
    }

    /// All values for a key — `basePackages = {"com.a", "com.b"}` yields both.
    pub fn named_all(&self, key: &str) -> Vec<String> {
        self.args
            .iter()
            .filter_map(|a| match a {
                AnnArg::Named(k, v) if k == key => Some(v.clone()),
                _ => None,
            })
            .collect()
    }
}

#[derive(Debug, Clone)]
pub enum Expr {
    /// Concatenation chain of literals, variable references and method calls.
    /// A single operand is just a one-element chain.
    Concat(Vec<Operand>),
    /// `return null;`, a missing return, or a body too complex for the
    /// interpreter subset.
    None,
}

#[derive(Debug, Clone)]
pub enum Operand {
    Lit(String),
    Var(String),
    /// `receiver.method(arg, ...)` — a call on another object, e.g. an
    /// injected service field.
    Call {
        receiver: String,
        method: String,
        args: Vec<CallArg>,
    },
}

#[derive(Debug, Clone)]
pub enum CallArg {
    Lit(String),
    Var(String),
}
