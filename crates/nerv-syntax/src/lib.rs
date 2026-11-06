use std::fmt;

use nerv_lexer::{LexError, Lexer, Token, TokenKind};

mod source;
pub use source::{SourceFile, SourceId, SourceMap, Span};

#[derive(Debug, Clone, PartialEq)]
pub struct Module {
    pub source: SourceId,
    /// Source occurrences of types (including nested array/function types).
    /// Semantic `Type` values stay location-independent for type equality.
    pub type_uses: Vec<TypeUse>,
    pub name: Option<Vec<String>>,
    pub imports: Vec<Import>,
    pub structs: Vec<Struct>,
    pub enums: Vec<Enum>,
    pub traits: Vec<Trait>,
    pub impls: Vec<Impl>,
    pub functions: Vec<Function>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Import {
    pub span: Span,
    pub path: Vec<String>,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Function {
    pub span: Span,
    pub name: String,
    pub generics: Vec<GenericParameter>,
    pub params: Vec<Parameter>,
    pub result: Type,
    pub body: Vec<Statement>,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Struct {
    pub span: Span,
    pub name: String,
    pub generics: Vec<GenericParameter>,
    pub implements: Option<String>,
    pub fields: Vec<StructField>,
    pub methods: Vec<Function>,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructField {
    pub span: Span,
    pub name: String,
    pub ty: Type,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Enum {
    pub span: Span,
    pub name: String,
    pub generics: Vec<GenericParameter>,
    pub variants: Vec<EnumVariant>,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Trait {
    pub span: Span,
    pub name: String,
    pub methods: Vec<Function>,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Impl {
    pub span: Span,
    pub type_name: String,
    pub trait_name: Option<String>,
    pub methods: Vec<Function>,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnumVariant {
    pub span: Span,
    pub name: String,
    pub fields: Vec<Type>,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parameter {
    pub span: Span,
    pub name: String,
    pub ty: Type,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenericParameter {
    pub name: String,
    pub constraint: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Type {
    Void,
    Bool,
    Int,
    Integer {
        bits: u32,
        signed: bool,
    },
    Float,
    Float32,
    String,
    Array(Box<Type>),
    Function {
        params: Vec<Type>,
        result: Box<Type>,
    },
    Named(String),
    Applied {
        name: String,
        args: Vec<Type>,
    },
}

pub fn numeric_type(name: &str) -> Option<Type> {
    Some(match name {
        "i64" => Type::Int,
        "f64" => Type::Float,
        "f32" => Type::Float32,
        "isize" => Type::Integer {
            bits: usize::BITS,
            signed: true,
        },
        "usize" => Type::Integer {
            bits: usize::BITS,
            signed: false,
        },
        "i8" | "i16" | "i32" | "u8" | "u16" | "u32" | "u64" => Type::Integer {
            bits: name[1..].parse().unwrap(),
            signed: name.starts_with('i'),
        },
        _ => return None,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeUse {
    pub span: Span,
    pub ty: Type,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Statement {
    pub span: Span,
    pub kind: StatementKind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum StatementKind {
    Let {
        name: String,
        mutable: bool,
        ty: Option<Type>,
        value: Expr,
        line: usize,
        column: usize,
    },
    Assign {
        name: String,
        value: Expr,
    },
    AssignIndex {
        base: Expr,
        index: Expr,
        value: Expr,
    },
    AssignField {
        base: Expr,
        name: String,
        value: Expr,
    },
    While {
        condition: Expr,
        body: Vec<Statement>,
    },
    For {
        name: String,
        start: Expr,
        end: Expr,
        inclusive: bool,
        body: Vec<Statement>,
    },
    Return(Expr),
    Expr(Expr),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Expr {
    pub span: Span,
    pub kind: ExprKind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExprKind {
    Int(i64),
    Float(f64),
    String(String),
    Bool(bool),
    Array(Vec<Expr>),
    ArrayRepeat {
        value: Box<Expr>,
        length: Box<Expr>,
    },
    Lambda {
        params: Vec<Parameter>,
        result: Type,
        body: Box<Expr>,
    },
    Match {
        value: Box<Expr>,
        arms: Vec<MatchArm>,
    },
    Method {
        base: Box<Expr>,
        name: String,
        args: Vec<Expr>,
    },
    Name(String),
    Struct {
        name: String,
        fields: Vec<(String, Expr)>,
    },
    If {
        condition: Box<Expr>,
        then_body: Vec<Statement>,
        else_body: Vec<Statement>,
    },
    Call {
        callee: Box<Expr>,
        args: Vec<Expr>,
    },
    Field {
        base: Box<Expr>,
        name: String,
    },
    Index {
        base: Box<Expr>,
        index: Box<Expr>,
    },
    Unary {
        op: UnaryOp,
        value: Box<Expr>,
    },
    Binary {
        left: Box<Expr>,
        op: BinaryOp,
        right: Box<Expr>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct MatchArm {
    pub span: Span,
    pub variant: String,
    pub binding: Option<String>,
    pub body: Expr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Neg,
    Not,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Eq,
    NotEq,
    Lt,
    LtEq,
    Gt,
    GtEq,
    And,
    Or,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub span: Span,
    pub line: usize,
    pub column: usize,
    pub message: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ParseError {}

pub fn parse(source: &str) -> Result<Module, FrontendError> {
    parse_source(source, SourceId::default())
}

pub fn parse_source(source: &str, id: SourceId) -> Result<Module, FrontendError> {
    let tokens = Lexer::new(source).collect::<Result<Vec<_>, _>>()?;
    Parser::new(source, tokens, id)
        .parse_module()
        .map_err(Into::into)
}

#[derive(Debug)]
pub enum FrontendError {
    Lex(LexError),
    Parse(ParseError),
}

impl FrontendError {
    pub fn span(&self, source: SourceId) -> Span {
        match self {
            Self::Lex(error) => Span {
                source,
                start: error.span.start,
                end: error.span.end,
            },
            Self::Parse(error) => error.span,
        }
    }
}

impl From<LexError> for FrontendError {
    fn from(error: LexError) -> Self {
        Self::Lex(error)
    }
}

impl From<ParseError> for FrontendError {
    fn from(error: ParseError) -> Self {
        Self::Parse(error)
    }
}

impl fmt::Display for FrontendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Lex(error) => error.fmt(f),
            Self::Parse(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for FrontendError {}

struct Parser<'source> {
    id: SourceId,
    type_uses: Vec<TypeUse>,
    source: &'source str,
    tokens: Vec<Token>,
    index: usize,
}

impl<'source> Parser<'source> {
    fn new(source: &'source str, tokens: Vec<Token>, id: SourceId) -> Self {
        Self {
            id,
            type_uses: Vec::new(),
            source,
            tokens,
            index: 0,
        }
    }

    fn parse_module(mut self) -> Result<Module, ParseError> {
        let mut module = Module {
            source: self.id,
            type_uses: Vec::new(),
            name: None,
            imports: Vec::new(),
            structs: Vec::new(),
            enums: Vec::new(),
            traits: Vec::new(),
            impls: Vec::new(),
            functions: Vec::new(),
        };
        self.skip_newlines();
        while !self.at(TokenKind::Eof) {
            match self.kind() {
                TokenKind::Module => {
                    self.bump();
                    module.name = Some(self.path()?);
                    self.end_line()?;
                }
                TokenKind::Import => {
                    let token = self.bump();
                    let path = self.path()?;
                    module.imports.push(Import {
                        span: self.span_from(token.span.start),
                        path,
                        line: token.line,
                        column: token.column,
                    });
                    self.end_line()?;
                }
                TokenKind::Struct => module.structs.push(self.struct_decl()?),
                TokenKind::Enum => module.enums.push(self.enum_decl()?),
                TokenKind::Trait => module.traits.push(self.trait_decl()?),
                TokenKind::Impl => module.impls.push(self.impl_decl()?),
                TokenKind::Fn => module.functions.push(self.function(None, false)?),
                TokenKind::Test => module.functions.push(self.named_block("__nerv_test")?),
                TokenKind::Bench => module.functions.push(self.named_block("__nerv_bench")?),
                TokenKind::Export => {
                    self.bump();
                    module.functions.push(self.function(None, false)?);
                }
                _ => return Err(self.error("expected module, import, struct, enum, or fn")),
            }
            self.skip_newlines();
        }
        module.type_uses = self.type_uses;
        Ok(module)
    }

    fn generic_parameters(&mut self) -> Result<Vec<GenericParameter>, ParseError> {
        let mut params = Vec::new();
        if !self.consume(TokenKind::Lt) {
            return Ok(params);
        }
        loop {
            let name = self.ident()?;
            if params.iter().any(|p: &GenericParameter| p.name == name) {
                return Err(self.error("duplicate generic parameter"));
            }
            let constraint = if self.consume(TokenKind::Colon) {
                Some(self.ident()?)
            } else {
                None
            };
            params.push(GenericParameter { name, constraint });
            if !self.consume(TokenKind::Comma) {
                break;
            }
        }
        self.expect(TokenKind::Gt)?;
        Ok(params)
    }

    fn struct_decl(&mut self) -> Result<Struct, ParseError> {
        let start = self.expect(TokenKind::Struct)?;
        let name = self.ident()?;
        let generics = self.generic_parameters()?;
        let implements = if self.consume(TokenKind::Implements) {
            Some(self.ident()?)
        } else {
            None
        };
        self.expect(TokenKind::Newline)?;
        self.expect(TokenKind::Indent)?;
        let mut fields = Vec::new();
        let mut methods = Vec::new();
        self.skip_newlines();
        while !self.at(TokenKind::Dedent) && !self.at(TokenKind::Eof) {
            if self.at(TokenKind::Fn) {
                let mut method = self.function(Some(&name), false)?;
                method.name = format!("{name}__{}", method.name);
                methods.push(method);
                self.skip_newlines();
                continue;
            }
            let token = self.peek().clone();
            let name = self.ident()?;
            self.expect(TokenKind::Colon)?;
            let ty = self.ty()?;
            fields.push(StructField {
                span: self.span_from(token.span.start),
                name,
                ty,
                line: token.line,
                column: token.column,
            });
            self.end_line()?;
            self.skip_newlines();
        }
        self.expect(TokenKind::Dedent)?;
        Ok(Struct {
            span: self.span_from(start.span.start),
            name,
            generics,
            implements,
            fields,
            methods,
            line: start.line,
            column: start.column,
        })
    }

    fn trait_decl(&mut self) -> Result<Trait, ParseError> {
        let start = self.expect(TokenKind::Trait)?;
        let name = self.ident()?;
        self.expect(TokenKind::Newline)?;
        self.expect(TokenKind::Indent)?;
        let mut methods = Vec::new();
        self.skip_newlines();
        while !self.at(TokenKind::Dedent) && !self.at(TokenKind::Eof) {
            if !self.at(TokenKind::Fn) {
                return Err(self.error("expected trait method"));
            }
            methods.push(self.function(Some(&name), true)?);
            self.skip_newlines();
        }
        self.expect(TokenKind::Dedent)?;
        Ok(Trait {
            span: self.span_from(start.span.start),
            name,
            methods,
            line: start.line,
            column: start.column,
        })
    }

    fn impl_decl(&mut self) -> Result<Impl, ParseError> {
        let start = self.expect(TokenKind::Impl)?;
        let type_name = self.ident()?;
        let trait_name = if self.consume(TokenKind::For) {
            Some(self.ident()?)
        } else {
            None
        };
        self.expect(TokenKind::Newline)?;
        self.expect(TokenKind::Indent)?;
        let mut methods = Vec::new();
        self.skip_newlines();
        while !self.at(TokenKind::Dedent) && !self.at(TokenKind::Eof) {
            if !self.at(TokenKind::Fn) {
                return Err(self.error("expected implementation method"));
            }
            let mut method = self.function(Some(&type_name), false)?;
            method.name = format!("{type_name}__{}", method.name);
            methods.push(method);
            self.skip_newlines();
        }
        self.expect(TokenKind::Dedent)?;
        Ok(Impl {
            span: self.span_from(start.span.start),
            type_name,
            trait_name,
            methods,
            line: start.line,
            column: start.column,
        })
    }

    fn enum_decl(&mut self) -> Result<Enum, ParseError> {
        let start = self.expect(TokenKind::Enum)?;
        let name = self.ident()?;
        let generics = self.generic_parameters()?;
        self.expect(TokenKind::Newline)?;
        self.expect(TokenKind::Indent)?;
        let mut variants = Vec::new();
        self.skip_newlines();
        while !self.at(TokenKind::Dedent) && !self.at(TokenKind::Eof) {
            let token = self.peek().clone();
            let name = self.ident()?;
            let mut fields = Vec::new();
            if self.consume(TokenKind::LParen) {
                if !self.at(TokenKind::RParen) {
                    loop {
                        fields.push(self.ty()?);
                        if !self.consume(TokenKind::Comma) {
                            break;
                        }
                    }
                }
                self.expect(TokenKind::RParen)?;
            }
            variants.push(EnumVariant {
                span: self.span_from(token.span.start),
                name,
                fields,
                line: token.line,
                column: token.column,
            });
            self.end_line()?;
            self.skip_newlines();
        }
        self.expect(TokenKind::Dedent)?;
        Ok(Enum {
            span: self.span_from(start.span.start),
            name,
            generics,
            variants,
            line: start.line,
            column: start.column,
        })
    }

    fn function(
        &mut self,
        receiver: Option<&str>,
        declaration: bool,
    ) -> Result<Function, ParseError> {
        let start = self.expect(TokenKind::Fn)?;
        let name = self.ident()?;
        let generics = self.generic_parameters()?;
        if receiver.is_some() && !generics.is_empty() {
            return Err(self.error("generic method declarations are not supported"));
        }

        self.expect(TokenKind::LParen)?;
        let mut params = Vec::new();
        if !self.at(TokenKind::RParen) {
            loop {
                let token = self.peek().clone();
                let name = self.ident()?;
                let ty = if self.consume(TokenKind::Colon) {
                    self.ty()?
                } else if params.is_empty() {
                    Type::Named(
                        receiver
                            .ok_or_else(|| self.error("parameter type required"))?
                            .to_owned(),
                    )
                } else {
                    return Err(self.error("parameter type required"));
                };
                params.push(Parameter {
                    span: self.span_from(token.span.start),
                    name,
                    ty,
                    line: token.line,
                    column: token.column,
                });
                if !self.consume(TokenKind::Comma) {
                    break;
                }
            }
        }
        self.expect(TokenKind::RParen)?;
        let result = if self.consume(TokenKind::ThinArrow) {
            self.ty()?
        } else {
            Type::Void
        };
        let body = if declaration {
            self.end_line()?;
            Vec::new()
        } else if self.consume(TokenKind::FatArrow) {
            let expr = self.expr(0)?;
            self.end_line()?;
            vec![Statement {
                span: expr.span,
                kind: StatementKind::Expr(expr),
            }]
        } else {
            self.expect(TokenKind::Newline)?;
            self.block()?
        };
        Ok(Function {
            span: self.span_from(start.span.start),
            name,
            generics,
            params,
            result,
            body,
            line: start.line,
            column: start.column,
        })
    }

    fn named_block(&mut self, prefix: &str) -> Result<Function, ParseError> {
        let token = self.bump();
        if !self.at(TokenKind::String) {
            return Err(self.error("expected test or benchmark name"));
        }
        self.bump();
        self.expect(TokenKind::Newline)?;
        let body = self.block()?;
        Ok(Function {
            span: self.span_from(token.span.start),
            name: format!("{prefix}_{}_{}", token.line, token.column),
            generics: Vec::new(),
            params: Vec::new(),
            result: Type::Void,
            body,
            line: token.line,
            column: token.column,
        })
    }

    fn statement(&mut self) -> Result<Statement, ParseError> {
        let start = self.peek().span.start;
        let kind = self.statement_kind()?;
        Ok(Statement {
            span: self.span_from(start),
            kind,
        })
    }

    fn statement_kind(&mut self) -> Result<StatementKind, ParseError> {
        if self.consume(TokenKind::Let) {
            let mutable = self.consume(TokenKind::Mut);
            let token = self.peek().clone();
            let name = self.ident()?;
            let ty = if self.consume(TokenKind::Colon) {
                Some(self.ty()?)
            } else {
                None
            };
            self.expect(TokenKind::Eq)?;
            let value = self.expr(0)?;
            return Ok(StatementKind::Let {
                name,
                mutable,
                ty,
                value,
                line: token.line,
                column: token.column,
            });
        }
        if self.consume(TokenKind::Return) {
            return Ok(StatementKind::Return(self.expr(0)?));
        }
        if self.consume(TokenKind::While) {
            let condition = self.expr(0)?;
            self.expect(TokenKind::Newline)?;
            return Ok(StatementKind::While {
                condition,
                body: self.block()?,
            });
        }
        if self.consume(TokenKind::For) {
            let name = self.ident()?;
            self.expect(TokenKind::In)?;
            let start = self.expr(0)?;
            let inclusive = if self.consume(TokenKind::RangeInclusive) {
                true
            } else {
                self.expect(TokenKind::Range)?;
                false
            };
            let end = self.expr(0)?;
            self.expect(TokenKind::Newline)?;
            return Ok(StatementKind::For {
                name,
                start,
                end,
                inclusive,
                body: self.block()?,
            });
        }
        let target = self.expr(0)?;
        if !self.consume(TokenKind::Eq) {
            return Ok(StatementKind::Expr(target));
        }
        let value = self.expr(0)?;
        match target.kind {
            ExprKind::Name(name) => Ok(StatementKind::Assign { name, value }),
            ExprKind::Field { base, name } => Ok(StatementKind::AssignField {
                base: *base,
                name,
                value,
            }),
            ExprKind::Index { base, index } => Ok(StatementKind::AssignIndex {
                base: *base,
                index: *index,
                value,
            }),
            _ => Err(self.error("invalid assignment target")),
        }
    }

    fn expr(&mut self, min_precedence: u8) -> Result<Expr, ParseError> {
        let start = self.peek().span.start;
        let left = match self.kind() {
            TokenKind::Minus => {
                self.bump();
                if self.at(TokenKind::Int)
                    && self.peek().text(self.source).replace('_', "") == "9223372036854775808"
                {
                    self.bump();
                    ExprKind::Int(i64::MIN)
                } else {
                    ExprKind::Unary {
                        op: UnaryOp::Neg,
                        value: Box::new(self.expr(8)?),
                    }
                }
            }
            TokenKind::Not => {
                self.bump();
                ExprKind::Unary {
                    op: UnaryOp::Not,
                    value: Box::new(self.expr(8)?),
                }
            }
            TokenKind::If => self.if_expr()?,
            TokenKind::Match => self.match_expr()?,
            TokenKind::Fn => self.lambda()?,
            TokenKind::Int => {
                let token = self.bump();
                let text = self.text(token.clone()).replace('_', "");
                ExprKind::Int(
                    text.parse()
                        .map_err(|_| self.error_at_token(&token, "invalid integer"))?,
                )
            }
            TokenKind::Float => {
                let token = self.bump();
                let text = self.text(token.clone()).replace('_', "");
                let value: f64 = text
                    .parse()
                    .map_err(|_| self.error_at_token(&token, "invalid float"))?;
                if !value.is_finite() {
                    return Err(
                        self.error_at_token(&token, "float literal is outside the f64 range")
                    );
                }
                ExprKind::Float(value)
            }
            TokenKind::String => {
                let token = self.bump();
                let text = self.text(token.clone());
                let mut decoded = String::new();
                let mut chars = text[1..text.len() - 1].chars();
                while let Some(ch) = chars.next() {
                    decoded.push(match ch {
                        '\\' => match chars.next() {
                            Some('n') => '\n',
                            Some('r') => '\r',
                            Some('t') => '\t',
                            Some('"') => '"',
                            Some('\\') => '\\',
                            _ => return Err(self.error_at_token(&token, "unsupported string escape")),
                        },
                        '\0' => return Err(self.error_at_token(&token, "NUL bytes are not supported in string literals")),
                        ch => ch,
                    });
                }
                ExprKind::String(decoded)
            }
            TokenKind::LBracket => {
                self.bump();
                let mut values = Vec::new();
                if !self.at(TokenKind::RBracket) {
                    loop {
                        values.push(self.expr(0)?);
                        if !self.consume(TokenKind::Comma) {
                            break;
                        }
                    }
                }
                self.expect(TokenKind::RBracket)?;
                if values.len() == 1 && self.consume(TokenKind::Star) {
                    ExprKind::ArrayRepeat {
                        value: Box::new(values.pop().unwrap()),
                        length: Box::new(self.expr(7)?),
                    }
                } else {
                    ExprKind::Array(values)
                }
            }
            TokenKind::True => {
                self.bump();
                ExprKind::Bool(true)
            }
            TokenKind::False => {
                self.bump();
                ExprKind::Bool(false)
            }
            TokenKind::Ident | TokenKind::Some | TokenKind::None => ExprKind::Name(self.ident()?),
            TokenKind::LParen => {
                self.bump();
                let expr = self.expr(0)?;
                self.expect(TokenKind::RParen)?;
                expr.kind
            }
            _ => return Err(self.error("expected expression")),
        };

        let mut left = Expr {
            kind: left,
            span: self.span_from(start),
        };
        loop {
            if self.consume(TokenKind::LParen) {
                let mut args = Vec::new();
                if !self.at(TokenKind::RParen) {
                    loop {
                        args.push(self.expr(0)?);
                        if !self.consume(TokenKind::Comma) {
                            break;
                        }
                    }
                }
                self.expect(TokenKind::RParen)?;
                left = Expr {
                    kind: ExprKind::Call {
                        callee: Box::new(left),
                        args,
                    },
                    span: self.span_from(start),
                };
                continue;
            }
            if self.consume(TokenKind::Dot) {
                left = Expr {
                    kind: ExprKind::Field {
                        base: Box::new(left),
                        name: self.ident()?,
                    },
                    span: self.span_from(start),
                };
                continue;
            }
            if self.consume(TokenKind::Colon) {
                let name = self.ident()?;
                self.expect(TokenKind::LParen)?;
                let mut args = Vec::new();
                if !self.at(TokenKind::RParen) {
                    loop {
                        args.push(self.expr(0)?);
                        if !self.consume(TokenKind::Comma) {
                            break;
                        }
                    }
                }
                self.expect(TokenKind::RParen)?;
                left = Expr {
                    kind: ExprKind::Method {
                        base: Box::new(left),
                        name,
                        args,
                    },
                    span: self.span_from(start),
                };
                continue;
            }
            if self.consume(TokenKind::LBracket) {
                let index = self.expr(0)?;
                self.expect(TokenKind::RBracket)?;
                left = Expr {
                    kind: ExprKind::Index {
                        base: Box::new(left),
                        index: Box::new(index),
                    },
                    span: self.span_from(start),
                };
                continue;
            }
            if self.consume(TokenKind::LBrace) {
                let ExprKind::Name(name) = left.kind else {
                    return Err(self.error("expected struct name"));
                };
                let mut fields = Vec::new();
                if !self.at(TokenKind::RBrace) {
                    loop {
                        let field = self.ident()?;
                        self.expect(TokenKind::Colon)?;
                        let value = self.expr(0)?;
                        fields.push((field, value));
                        if !self.consume(TokenKind::Comma) {
                            break;
                        }
                    }
                }
                self.expect(TokenKind::RBrace)?;
                left = Expr {
                    kind: ExprKind::Struct { name, fields },
                    span: self.span_from(start),
                };
                continue;
            }
            let Some((op, precedence)) = binary(self.kind()) else {
                break;
            };
            if precedence < min_precedence {
                break;
            }
            self.bump();
            let right = self.expr(precedence + 1)?;
            left = Expr {
                kind: ExprKind::Binary {
                    left: Box::new(left),
                    op,
                    right: Box::new(right),
                },
                span: self.span_from(start),
            };
        }
        Ok(left)
    }

    fn if_expr(&mut self) -> Result<ExprKind, ParseError> {
        self.expect(TokenKind::If)?;
        let condition = self.expr(0)?;
        self.expect(TokenKind::Newline)?;
        let then_body = self.block()?;
        self.expect(TokenKind::Else)?;
        self.expect(TokenKind::Newline)?;
        let else_body = self.block()?;
        Ok(ExprKind::If {
            condition: Box::new(condition),
            then_body,
            else_body,
        })
    }

    fn match_expr(&mut self) -> Result<ExprKind, ParseError> {
        self.expect(TokenKind::Match)?;
        let value = self.expr(0)?;
        self.expect(TokenKind::Newline)?;
        self.expect(TokenKind::Indent)?;
        let mut arms = Vec::new();
        self.skip_newlines();
        while !self.at(TokenKind::Dedent) && !self.at(TokenKind::Eof) {
            let start = self.peek().span.start;
            let variant = self.path()?.join(".");
            let binding = if self.consume(TokenKind::LParen) {
                let value = self.ident()?;
                self.expect(TokenKind::RParen)?;
                Some(value)
            } else {
                None
            };
            self.expect(TokenKind::FatArrow)?;
            let body = self.expr(0)?;
            if self.tokens[self.index - 1].kind != TokenKind::Dedent {
                self.end_line()?;
            }
            arms.push(MatchArm {
                span: self.span_from(start),
                variant,
                binding,
                body,
            });
            self.skip_newlines();
        }
        self.expect(TokenKind::Dedent)?;
        Ok(ExprKind::Match {
            value: Box::new(value),
            arms,
        })
    }

    fn block(&mut self) -> Result<Vec<Statement>, ParseError> {
        self.expect(TokenKind::Indent)?;
        let mut body = Vec::new();
        self.skip_newlines();
        while !self.at(TokenKind::Dedent) && !self.at(TokenKind::Eof) {
            let statement = self.statement()?;
            let has_nested_block = self.tokens[self.index - 1].kind == TokenKind::Dedent;
            body.push(statement);
            if !has_nested_block {
                self.end_line()?;
            }
            self.skip_newlines();
        }
        self.expect(TokenKind::Dedent)?;
        Ok(body)
    }

    fn path(&mut self) -> Result<Vec<String>, ParseError> {
        let mut path = vec![self.ident()?];
        while self.consume(TokenKind::Dot) {
            path.push(self.ident()?);
        }
        Ok(path)
    }

    fn ty(&mut self) -> Result<Type, ParseError> {
        let start = self.peek().span.start;
        let ty = self.type_kind()?;
        self.type_uses.push(TypeUse {
            span: self.span_from(start),
            ty: ty.clone(),
        });
        Ok(ty)
    }

    fn type_kind(&mut self) -> Result<Type, ParseError> {
        if self.consume(TokenKind::Fn) {
            self.expect(TokenKind::LParen)?;
            let mut params = Vec::new();
            if !self.at(TokenKind::RParen) {
                loop {
                    params.push(self.ty()?);
                    if !self.consume(TokenKind::Comma) {
                        break;
                    }
                }
            }
            self.expect(TokenKind::RParen)?;
            self.expect(TokenKind::ThinArrow)?;
            return Ok(Type::Function {
                params,
                result: Box::new(self.ty()?),
            });
        }
        if self.consume(TokenKind::LBracket) {
            if self.consume(TokenKind::RBracket) {
                return Ok(Type::Array(Box::new(self.ty()?)));
            }
            let element = self.ty()?;
            self.expect(TokenKind::Semicolon)?;
            self.expect(TokenKind::Int)?;
            self.expect(TokenKind::RBracket)?;
            return Ok(Type::Array(Box::new(element)));
        }
        let name = self.ident()?;
        if self.consume(TokenKind::Lt) {
            let mut args = Vec::new();
            loop {
                args.push(self.ty()?);
                if !self.consume(TokenKind::Comma) {
                    break;
                }
            }
            self.expect(TokenKind::Gt)?;
            return Ok(Type::Applied { name, args });
        }

        Ok(match name.as_str() {
            "void" => Type::Void,
            "bool" => Type::Bool,
            "i64" => Type::Int,
            "f64" => Type::Float,
            "f32" => Type::Float32,
            "i8" | "i16" | "i32" | "u8" | "u16" | "u32" | "u64" | "usize" | "isize" => {
                numeric_type(&name).unwrap()
            }
            "string" => Type::String,
            _ => Type::Named(name),
        })
    }

    fn lambda(&mut self) -> Result<ExprKind, ParseError> {
        self.expect(TokenKind::Fn)?;
        self.expect(TokenKind::LParen)?;
        let mut params = Vec::new();
        if !self.at(TokenKind::RParen) {
            loop {
                let token = self.peek().clone();
                let name = self.ident()?;
                self.expect(TokenKind::Colon)?;
                let ty = self.ty()?;
                params.push(Parameter {
                    span: self.span_from(token.span.start),
                    name,
                    ty,
                    line: token.line,
                    column: token.column,
                });
                if !self.consume(TokenKind::Comma) {
                    break;
                }
            }
        }
        self.expect(TokenKind::RParen)?;
        self.expect(TokenKind::ThinArrow)?;
        let result = self.ty()?;
        self.expect(TokenKind::FatArrow)?;
        Ok(ExprKind::Lambda {
            params,
            result,
            body: Box::new(self.expr(0)?),
        })
    }

    fn ident(&mut self) -> Result<String, ParseError> {
        match self.kind() {
            TokenKind::Ident | TokenKind::Some | TokenKind::None => {
                let token = self.bump();
                Ok(self.text(token).to_owned())
            }
            _ => Err(self.error("expected identifier")),
        }
    }

    fn end_line(&mut self) -> Result<(), ParseError> {
        if self.at(TokenKind::Newline) {
            self.bump();
            Ok(())
        } else if self.at(TokenKind::Dedent) || self.at(TokenKind::Eof) {
            Ok(())
        } else {
            Err(self.error("expected end of line"))
        }
    }

    fn skip_newlines(&mut self) {
        while self.consume(TokenKind::Newline) {}
    }
    fn consume(&mut self, kind: TokenKind) -> bool {
        if self.at(kind) {
            self.bump();
            true
        } else {
            false
        }
    }
    fn expect(&mut self, kind: TokenKind) -> Result<Token, ParseError> {
        if self.at(kind) {
            Ok(self.bump())
        } else {
            Err(self.error(&format!("expected {kind:?}")))
        }
    }
    fn at(&self, kind: TokenKind) -> bool {
        self.kind() == kind
    }
    fn kind(&self) -> TokenKind {
        self.peek().kind
    }
    fn peek(&self) -> &Token {
        &self.tokens[self.index]
    }
    fn bump(&mut self) -> Token {
        let token = self.tokens[self.index].clone();
        self.index += 1;
        token
    }
    fn text(&self, token: Token) -> &str {
        token.text(self.source)
    }
    fn error(&self, message: &str) -> ParseError {
        self.error_at_token(self.peek(), message)
    }

    fn span_from(&self, start: usize) -> Span {
        // Layout tokens can sit at the beginning of the next declaration.
        let end = self.tokens[..self.index]
            .iter()
            .rev()
            .find(|token| {
                !matches!(
                    token.kind,
                    TokenKind::Indent | TokenKind::Dedent | TokenKind::Newline | TokenKind::Eof
                )
            })
            .map_or(start, |token| token.span.end);
        Span {
            source: self.id,
            start,
            end: end.max(start),
        }
    }

    fn error_at_token(&self, token: &Token, message: &str) -> ParseError {
        ParseError {
            span: Span {
                source: self.id,
                start: token.span.start,
                end: token.span.end,
            },
            line: token.line,
            column: token.column,
            message: message.to_owned(),
        }
    }
}

fn binary(kind: TokenKind) -> Option<(BinaryOp, u8)> {
    Some(match kind {
        TokenKind::Or => (BinaryOp::Or, 1),
        TokenKind::And => (BinaryOp::And, 2),
        TokenKind::EqEq => (BinaryOp::Eq, 3),
        TokenKind::NotEq => (BinaryOp::NotEq, 3),
        TokenKind::Lt => (BinaryOp::Lt, 4),
        TokenKind::LtEq => (BinaryOp::LtEq, 4),
        TokenKind::Gt => (BinaryOp::Gt, 4),
        TokenKind::GtEq => (BinaryOp::GtEq, 4),
        TokenKind::Plus => (BinaryOp::Add, 5),
        TokenKind::Minus => (BinaryOp::Sub, 5),
        TokenKind::Star => (BinaryOp::Mul, 6),
        TokenKind::Slash => (BinaryOp::Div, 6),
        TokenKind::Percent => (BinaryOp::Rem, 6),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retains_source_spans_on_ast_clones_and_type_occurrences() {
        let source = "import lib.math\nfn apply(callback: fn([]i64) -> i64) -> i64\n    let value: i64 = callback([1, 2])\n    value + 3\n";
        let module = parse_source(source, SourceId(7)).unwrap();
        assert_eq!(module.clone(), module);
        assert_eq!(module.source, SourceId(7));
        let slice = |span: Span| {
            assert_eq!(span.source, SourceId(7));
            &source[span.start..span.end]
        };
        assert_eq!(slice(module.imports[0].span), "import lib.math");
        let function = &module.functions[0];
        assert!(slice(function.span).starts_with("fn apply("));
        assert_eq!(slice(function.params[0].span), "callback: fn([]i64) -> i64");
        assert_eq!(
            slice(function.body[0].span),
            "let value: i64 = callback([1, 2])"
        );
        let StatementKind::Let { value, .. } = &function.body[0].kind else {
            panic!("expected binding")
        };
        assert_eq!(slice(value.span), "callback([1, 2])");
        let ExprKind::Call { callee, args } = &value.kind else {
            panic!("expected call")
        };
        assert_eq!(slice(callee.span), "callback");
        assert_eq!(slice(args[0].span), "[1, 2]");
        let ExprKind::Array(elements) = &args[0].kind else {
            panic!("expected array")
        };
        assert_eq!(slice(elements[1].span), "2");
        assert!(
            module
                .type_uses
                .iter()
                .any(|ty| slice(ty.span) == "fn([]i64) -> i64")
        );
        assert!(module.type_uses.iter().any(|ty| slice(ty.span) == "[]i64"));
        assert!(module.type_uses.iter().all(|ty| !slice(ty.span).is_empty()));
    }

    #[test]
    fn parses_a_function() {
        let module =
            parse("fn add(a: i64, b: i64) -> i64\n    let total = a + b\n    return total\n")
                .unwrap();
        assert_eq!(module.functions.len(), 1);
        assert_eq!(module.functions[0].params.len(), 2);
        assert_eq!(module.functions[0].body.len(), 2);
    }

    #[test]
    fn parses_if_expression() {
        let module = parse(
            "fn choose(flag: bool, left: i64, right: i64) -> i64\n    if flag\n        left\n    else\n        right\n",
        )
        .unwrap();
        assert!(matches!(
            &module.functions[0].body[0].kind,
            StatementKind::Expr(Expr {
                kind: ExprKind::If { .. },
                ..
            })
        ));
    }

    #[test]
    fn parses_struct_values() {
        let module = parse(
            "struct Pair\n    left: i64\n    right: i64\nfn sum(pair: Pair) -> i64 => pair.left + pair.right\n",
        )
        .unwrap();
        assert_eq!(module.structs[0].fields.len(), 2);
    }

    #[test]
    fn parses_field_assignment() {
        let module = parse(
            "struct Point\n    x: i64\nfn main() -> i64\n    let mut point = Point { x: 1 }\n    point.x = 2\n    point.x\n",
        )
        .unwrap();
        assert!(matches!(
            &module.functions[0].body[1].kind,
            StatementKind::AssignField { .. }
        ));
    }

    #[test]
    fn parses_loops_and_assignment() {
        let module = parse(
            "fn main() -> i64\n    let mut value = 0\n    for item in 1..=3\n        value = value + item\n    while value < 10\n        value = value + 1\n    return value\n",
        )
        .unwrap();
        assert_eq!(module.functions[0].body.len(), 4);
    }

    #[test]
    fn parses_an_enum() {
        let module = parse("enum Flag\n    Off\n    On\n").unwrap();
        assert_eq!(module.enums[0].variants.len(), 2);
    }

    #[test]
    fn parses_enum_payloads() {
        let module = parse("enum Result\n    Ok(i64)\n    Err(string)\n").unwrap();
        assert_eq!(module.enums[0].variants[0].fields, vec![Type::Int]);
        assert_eq!(module.enums[0].variants[1].fields, vec![Type::String]);
    }

    #[test]
    fn parses_match_arms() {
        let module = parse(
            "fn main() -> i64\n    let value = Some(1)\n    match value\n        Some(item) => item\n        None => 0\n",
        )
        .unwrap();
        assert!(matches!(
            &module.functions[0].body[1].kind,
            StatementKind::Expr(Expr {
                kind: ExprKind::Match { .. },
                ..
            })
        ));
    }

    #[test]
    fn parses_nested_blocks_in_match_arms() {
        let module = parse(
            "fn choose(value: Option<i64>) -> i64\n    match value\n        Some(item) => if item > 0\n            item\n        else\n            0\n        None => match value\n            Some(other) => other\n            None => 0\n    let after = 1\n    after\n",
        )
        .unwrap();
        assert_eq!(module.functions[0].body.len(), 3);
    }

    #[test]
    fn parses_test_and_bench_blocks() {
        let module =
            parse("test \"works\"\n    println(1)\nbench \"fast\"\n    println(2)\n").unwrap();
        assert_eq!(module.functions.len(), 2);
        assert!(module.functions[0].name.starts_with("__nerv_test"));
        assert!(module.functions[1].name.starts_with("__nerv_bench"));
    }

    #[test]
    fn parses_arrays() {
        let module = parse(
            "fn main() -> i64\n    let values: []i64 = [1, 2]\n    values[0] = 3\n    return values[0]\n",
        )
        .unwrap();
        assert_eq!(module.functions[0].body.len(), 3);
    }

    #[test]
    fn parses_fixed_array_syntax() {
        let module = parse(
            "fn main() -> i64\n    let values: [i64; 4] = [0] * 4\n    values[0] = 3\n    return values[0]\n",
        )
        .unwrap();
        assert!(matches!(
            &module.functions[0].body[0].kind,
            StatementKind::Let {
                ty: Some(Type::Array(_)),
                value: Expr {
                    kind: ExprKind::ArrayRepeat { .. },
                    ..
                },
                ..
            }
        ));
    }
}
