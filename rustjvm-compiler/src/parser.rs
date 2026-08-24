use crate::ast::*;
use crate::lexer::{lex, LexError, Token};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ParseError {
    #[error("lex error: {0}")]
    Lex(#[from] LexError),
    #[error("unexpected token {found:?}, expected {expected}")]
    Unexpected { expected: &'static str, found: Token },
    #[error("unexpected end of input")]
    Eof,
}

pub fn parse_source(src: &str) -> Result<JavaFile, ParseError> {
    Parser::new(lex(src)?).parse_file()
}

const MODIFIERS: &[&str] = &[
    "public",
    "private",
    "protected",
    "static",
    "final",
    "abstract",
    "synchronized",
    "default",
    "native",
    "strictfp",
    "transient",
    "volatile",
];

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, pos: 0 }
    }

    fn peek(&self) -> &Token {
        self.tokens.get(self.pos).unwrap_or(&Token::Eof)
    }

    fn bump(&mut self) -> Token {
        let t = self.peek().clone();
        if self.pos < self.tokens.len() {
            self.pos += 1;
        }
        t
    }

    fn eat(&mut self, t: &Token) -> bool {
        if self.peek() == t {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn expect(&mut self, t: &Token, what: &'static str) -> Result<(), ParseError> {
        if self.eat(t) {
            Ok(())
        } else {
            Err(ParseError::Unexpected {
                expected: what,
                found: self.peek().clone(),
            })
        }
    }

    fn expect_ident(&mut self) -> Result<String, ParseError> {
        match self.bump() {
            Token::Ident(s) => Ok(s),
            other => Err(ParseError::Unexpected {
                expected: "identifier",
                found: other,
            }),
        }
    }

    fn parse_file(&mut self) -> Result<JavaFile, ParseError> {
        let mut file = JavaFile::default();
        loop {
            match self.peek().clone() {
                Token::Eof => break,
                Token::Ident(kw) if kw == "package" => {
                    self.bump();
                    file.package = Some(self.parse_qualified_name()?);
                    self.expect(&Token::Semi, "';' after package declaration")?;
                }
                Token::Ident(kw) if kw == "import" => {
                    self.bump();
                    while !matches!(self.peek(), Token::Semi | Token::Eof) {
                        self.bump();
                    }
                    self.eat(&Token::Semi);
                }
                _ => {
                    let annotations = self.parse_annotations()?;
                    self.skip_modifiers();
                    match self.peek().clone() {
                        Token::Ident(kw) if kw == "class" => {
                            self.bump();
                            file.classes.push(self.parse_class(annotations)?);
                        }
                        // interface / enum / record bodies carry no routes we
                        // can compile in Phase 1; skip them gracefully.
                        _ => {
                            self.bump();
                        }
                    }
                }
            }
        }
        Ok(file)
    }

    fn parse_qualified_name(&mut self) -> Result<String, ParseError> {
        let mut name = self.expect_ident()?;
        while *self.peek() == Token::Dot {
            self.bump();
            name.push('.');
            name.push_str(&self.expect_ident()?);
        }
        Ok(name)
    }

    fn parse_annotations(&mut self) -> Result<Vec<Annotation>, ParseError> {
        let mut anns = Vec::new();
        while *self.peek() == Token::At {
            self.bump();
            let qualified = self.parse_qualified_name()?;
            let name = qualified
                .rsplit('.')
                .next()
                .unwrap_or(&qualified)
                .to_string();
            let mut args = Vec::new();
            if self.eat(&Token::LParen) {
                args = self.parse_annotation_args()?;
                self.expect(&Token::RParen, "')' after annotation arguments")?;
            }
            anns.push(Annotation { name, args });
        }
        Ok(anns)
    }

    fn parse_annotation_args(&mut self) -> Result<Vec<AnnArg>, ParseError> {
        let mut args = Vec::new();
        loop {
            match self.peek().clone() {
                Token::RParen | Token::Eof => break,
                Token::Comma => {
                    self.bump();
                }
                Token::LBrace => {
                    // Array initializer: @GetMapping({"/a", "/b"}) — keep every
                    // string element as a positional value.
                    self.bump();
                    while !matches!(self.peek(), Token::RBrace | Token::Eof) {
                        if let Token::Str(s) = self.bump() {
                            args.push(AnnArg::Value(s));
                        }
                    }
                    self.eat(&Token::RBrace);
                }
                Token::Str(s) => {
                    self.bump();
                    args.push(AnnArg::Value(s));
                }
                Token::Ident(key) => {
                    self.bump();
                    if self.eat(&Token::Eq) {
                        if self.eat(&Token::LBrace) {
                            // key = {"a", "b"} — keep every element under the key.
                            while !matches!(self.peek(), Token::RBrace | Token::Eof) {
                                match self.bump() {
                                    Token::Str(v) => args.push(AnnArg::Named(key.clone(), v)),
                                    Token::Ident(v) => args.push(AnnArg::Named(key.clone(), v)),
                                    _ => {}
                                }
                            }
                            self.eat(&Token::RBrace);
                        } else {
                            match self.bump() {
                                Token::Str(v) => args.push(AnnArg::Named(key, v)),
                                Token::Ident(v) => args.push(AnnArg::Named(key, v)),
                                other => {
                                    return Err(ParseError::Unexpected {
                                        expected: "annotation value",
                                        found: other,
                                    })
                                }
                            }
                        }
                    }
                }
                // Numbers, booleans-as-keywords handled above, everything else
                // is irrelevant to routing.
                _ => {
                    self.bump();
                }
            }
        }
        Ok(args)
    }

    fn parse_class(&mut self, annotations: Vec<Annotation>) -> Result<ClassDecl, ParseError> {
        let name = self.expect_ident()?;
        // Skip generics / extends / implements up to the body.
        while !matches!(self.peek(), Token::LBrace | Token::Eof) {
            self.bump();
        }
        self.expect(&Token::LBrace, "'{' to open class body")?;
        let mut methods = Vec::new();
        let mut fields = Vec::new();
        loop {
            match self.peek().clone() {
                Token::RBrace => {
                    self.bump();
                    break;
                }
                Token::Eof => return Err(ParseError::Eof),
                Token::Semi => {
                    self.bump();
                }
                _ => match self.parse_member(&name)? {
                    Member::Method(m) => methods.push(m),
                    Member::Field(f) => fields.push(f),
                    Member::Skipped => {}
                },
            }
        }
        Ok(ClassDecl {
            name,
            annotations,
            methods,
            fields,
        })
    }

    /// Parses one class member. Methods and fields are kept; constructors,
    /// initializer blocks and nested types are parsed past and skipped.
    fn parse_member(&mut self, class_name: &str) -> Result<Member, ParseError> {
        let annotations = self.parse_annotations()?;
        self.skip_modifiers();
        if matches!(self.peek(), Token::Other('<')) {
            self.skip_angle_brackets();
        }

        // Scan to the member's pivot: '(' (method/ctor), ';' or '=' (field),
        // '{' (initializer block / nested type).
        let mut head: Vec<Token> = Vec::new();
        loop {
            match self.peek().clone() {
                Token::LParen => {
                    self.bump();
                    let name = match head.last() {
                        Some(Token::Ident(n)) => n.clone(),
                        _ => {
                            self.skip_parens();
                            self.skip_body_or_semi();
                            return Ok(Member::Skipped);
                        }
                    };
                    let return_type = if head.len() >= 2 {
                        token_text(&head[head.len() - 2])
                    } else {
                        String::new()
                    };
                    if name == class_name && return_type.is_empty() {
                        // Constructor.
                        self.skip_parens();
                        self.skip_body_or_semi();
                        return Ok(Member::Skipped);
                    }
                    let params = self.parse_params()?;
                    // `throws Foo, Bar` clause.
                    while matches!(self.peek(), Token::Ident(_) | Token::Comma) {
                        self.bump();
                    }
                    let body = match self.peek() {
                        Token::Semi => {
                            self.bump();
                            None
                        }
                        Token::LBrace => Some(self.parse_method_body()?),
                        _ => None,
                    };
                    return Ok(Member::Method(MethodDecl {
                        name,
                        return_type,
                        params,
                        annotations,
                        body,
                    }));
                }
                Token::Semi => {
                    self.bump();
                    return Ok(field_from(annotations, &head));
                }
                Token::Eq => {
                    self.skip_field_initializer();
                    return Ok(field_from(annotations, &head));
                }
                Token::LBrace => {
                    self.skip_block();
                    return Ok(Member::Skipped);
                }
                Token::Eof => return Ok(Member::Skipped),
                _ => head.push(self.bump()),
            }
        }
    }

    fn parse_params(&mut self) -> Result<Vec<Param>, ParseError> {
        let mut params = Vec::new();
        loop {
            match self.peek().clone() {
                Token::RParen => {
                    self.bump();
                    break;
                }
                Token::Comma => {
                    self.bump();
                }
                Token::Eof => return Err(ParseError::Eof),
                _ => {
                    let annotations = self.parse_annotations()?;
                    self.skip_modifiers();
                    let mut ty_parts: Vec<String> = Vec::new();
                    loop {
                        match self.peek().clone() {
                            Token::Ident(s) => {
                                self.bump();
                                if matches!(self.peek(), Token::Comma | Token::RParen) {
                                    // This identifier is the parameter name;
                                    // everything before it was the type.
                                    params.push(Param {
                                        name: s,
                                        ty: ty_parts.join(""),
                                        annotations,
                                    });
                                    break;
                                }
                                ty_parts.push(s);
                            }
                            Token::Other(c) => {
                                self.bump();
                                ty_parts.push(c.to_string());
                            }
                            Token::Dot => {
                                self.bump();
                                ty_parts.push(".".into());
                            }
                            _ => {
                                self.bump();
                            }
                        }
                    }
                }
            }
        }
        Ok(params)
    }

    /// Captures a balanced `{ ... }` method body and reduces it to its first
    /// top-level `return` expression.
    fn parse_method_body(&mut self) -> Result<Expr, ParseError> {
        self.expect(&Token::LBrace, "'{' to open method body")?;
        let mut depth = 1usize;
        let mut body = Vec::new();
        while depth > 0 {
            match self.bump() {
                Token::LBrace => {
                    depth += 1;
                    body.push(Token::LBrace);
                }
                Token::RBrace => {
                    depth -= 1;
                    if depth > 0 {
                        body.push(Token::RBrace);
                    }
                }
                Token::Eof => return Err(ParseError::Eof),
                t => body.push(t),
            }
        }
        Ok(extract_return(&body))
    }

    fn skip_modifiers(&mut self) {
        while let Token::Ident(kw) = self.peek() {
            if MODIFIERS.contains(&kw.as_str()) {
                self.bump();
            } else {
                break;
            }
        }
    }

    fn skip_angle_brackets(&mut self) {
        let mut depth = 0i32;
        loop {
            match self.bump() {
                Token::Other('<') => depth += 1,
                Token::Other('>') => {
                    depth -= 1;
                    if depth <= 0 {
                        break;
                    }
                }
                Token::Eof => break,
                _ => {}
            }
        }
    }

    /// Assumes the opening '(' was just consumed.
    fn skip_parens(&mut self) {
        let mut depth = 1i32;
        while depth > 0 {
            match self.bump() {
                Token::LParen => depth += 1,
                Token::RParen => depth -= 1,
                Token::Eof => break,
                _ => {}
            }
        }
    }

    /// Assumes the next token is '{'.
    fn skip_block(&mut self) {
        let mut depth = 0i32;
        loop {
            match self.bump() {
                Token::LBrace => depth += 1,
                Token::RBrace => {
                    depth -= 1;
                    if depth <= 0 {
                        break;
                    }
                }
                Token::Eof => break,
                _ => {}
            }
        }
    }

    fn skip_body_or_semi(&mut self) {
        match self.peek() {
            Token::Semi => {
                self.bump();
            }
            Token::LBrace => self.skip_block(),
            _ => {}
        }
    }

    /// Field initializer: skip to ';', tolerating anonymous-class braces.
    fn skip_field_initializer(&mut self) {
        loop {
            match self.peek() {
                Token::Semi => {
                    self.bump();
                    break;
                }
                Token::LBrace => {
                    self.skip_block();
                    // Anonymous class bodies are followed by the terminating ';'.
                    self.eat(&Token::Semi);
                    break;
                }
                Token::Eof => break,
                _ => {
                    self.bump();
                }
            }
        }
    }
}

fn token_text(t: &Token) -> String {
    match t {
        Token::Ident(s) => s.clone(),
        Token::Other(c) => c.to_string(),
        Token::Dot => ".".into(),
        _ => String::new(),
    }
}

/// Builds a field declaration from the tokens scanned before a `;` or `=`.
/// Convention: the first identifier is the (simple) type, the last is the
/// field name — `private final GreetingService greetingService` and even
/// `List<String> names` both reduce correctly for DI purposes.
fn field_from(annotations: Vec<Annotation>, head: &[Token]) -> Member {
    let idents: Vec<&str> = head
        .iter()
        .filter_map(|t| match t {
            Token::Ident(s) => Some(s.as_str()),
            _ => None,
        })
        .collect();
    if idents.len() >= 2 {
        Member::Field(FieldDecl {
            name: idents[idents.len() - 1].to_string(),
            ty: idents[0].to_string(),
            annotations,
        })
    } else {
        Member::Skipped
    }
}

fn extract_return(body: &[Token]) -> Expr {
    let mut depth = 0i32;
    for (i, t) in body.iter().enumerate() {
        match t {
            Token::LBrace => depth += 1,
            Token::RBrace => depth -= 1,
            Token::Ident(kw) if kw == "return" && depth == 0 => {
                return parse_return_expr(&body[i + 1..]);
            }
            _ => {}
        }
    }
    Expr::None
}

/// Parses `operand (+ operand)* ;` where an operand is a string literal, an
/// identifier, or a `receiver.method(args...)` call. Anything more complex
/// truncates the chain — the runtime then reports the method as
/// not-yet-compilable instead of mis-evaluating it.
fn parse_return_expr(tokens: &[Token]) -> Expr {
    let mut operands: Vec<Operand> = Vec::new();
    let mut expect_operand = true;
    let mut i = 0usize;
    while i < tokens.len() {
        match &tokens[i] {
            Token::Semi => break,
            Token::Str(s) if expect_operand => {
                operands.push(Operand::Lit(s.clone()));
                expect_operand = false;
                i += 1;
            }
            Token::Ident(id) if expect_operand => {
                if id == "null" {
                    return Expr::None;
                }
                // Method call shape: ident '.' ident '(' ...
                let is_call = matches!(tokens.get(i + 1), Some(Token::Dot))
                    && matches!(tokens.get(i + 2), Some(Token::Ident(_)))
                    && matches!(tokens.get(i + 3), Some(Token::LParen));
                if is_call {
                    let receiver = id.clone();
                    let method = match &tokens[i + 2] {
                        Token::Ident(m) => m.clone(),
                        _ => unreachable!(),
                    };
                    match parse_call_args(&tokens[i + 4..]) {
                        Some((args, consumed)) => {
                            operands.push(Operand::Call {
                                receiver,
                                method,
                                args,
                            });
                            expect_operand = false;
                            i += 4 + consumed;
                        }
                        // Malformed call — degrade to opaque rather than guess.
                        None => return Expr::None,
                    }
                } else {
                    operands.push(Operand::Var(id.clone()));
                    expect_operand = false;
                    i += 1;
                }
            }
            Token::Plus if !expect_operand => {
                expect_operand = true;
                i += 1;
            }
            _ => {
                return if operands.is_empty() {
                    Expr::None
                } else {
                    Expr::Concat(operands)
                };
            }
        }
    }
    if operands.is_empty() {
        Expr::None
    } else {
        Expr::Concat(operands)
    }
}

/// Parses call arguments up to and including the closing ')'. Returns the
/// arguments and how many tokens were consumed, or `None` if the shape is
/// unsupported.
fn parse_call_args(tokens: &[Token]) -> Option<(Vec<CallArg>, usize)> {
    let mut args = Vec::new();
    let mut i = 0usize;
    loop {
        match tokens.get(i) {
            Some(Token::RParen) => return Some((args, i + 1)),
            Some(Token::Comma) => i += 1,
            Some(Token::Str(s)) => {
                args.push(CallArg::Lit(s.clone()));
                i += 1;
            }
            Some(Token::Ident(v)) => {
                args.push(CallArg::Var(v.clone()));
                i += 1;
            }
            _ => return None,
        }
    }
}
