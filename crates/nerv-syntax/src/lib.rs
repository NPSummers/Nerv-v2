use std::fmt;

use nerv_lexer::{LexError, Lexer, Token, TokenKind};

#[derive(Debug, Clone, PartialEq)]
pub struct Module {
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
    pub path: Vec<String>,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Function {
    pub name: String,
    pub params: Vec<Parameter>,
    pub result: Type,
    pub body: Vec<Statement>,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Struct {
    pub name: String,
    pub implements: Option<String>,
    pub fields: Vec<StructField>,
    pub methods: Vec<Function>,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructField {
    pub name: String,
    pub ty: Type,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Enum {
    pub name: String,
    pub variants: Vec<EnumVariant>,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Trait {
    pub name: String,
    pub methods: Vec<Function>,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Impl {
    pub type_name: String,
    pub trait_name: Option<String>,
    pub methods: Vec<Function>,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnumVariant {
    pub name: String,
    pub fields: Vec<Type>,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parameter {
    pub name: String,
    pub ty: Type,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Type {
    Void,
    Bool,
    Int,
    Float,
    String,
    Array(Box<Type>),
    Function {
        params: Vec<Type>,
        result: Box<Type>,
    },
    Named(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Statement {
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
pub enum Expr {
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
    let tokens = Lexer::new(source).collect::<Result<Vec<_>, _>>()?;
    Parser::new(source, tokens)
        .parse_module()
        .map_err(Into::into)
}

#[derive(Debug)]
pub enum FrontendError {
    Lex(LexError),
    Parse(ParseError),
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
    source: &'source str,
    tokens: Vec<Token>,
    index: usize,
}

impl<'source> Parser<'source> {
    fn new(source: &'source str, tokens: Vec<Token>) -> Self {
        Self {
            source,
            tokens,
            index: 0,
        }
    }

    fn parse_module(mut self) -> Result<Module, ParseError> {
        let mut module = Module {
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
        Ok(module)
    }

    fn struct_decl(&mut self) -> Result<Struct, ParseError> {
        let start = self.expect(TokenKind::Struct)?;
        let name = self.ident()?;
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
            name,
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
            name,
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
            vec![Statement::Expr(expr)]
        } else {
            self.expect(TokenKind::Newline)?;
            self.block()?
        };
        Ok(Function {
            name,
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
            name: format!("{prefix}_{}_{}", token.line, token.column),
            params: Vec::new(),
            result: Type::Void,
            body,
            line: token.line,
            column: token.column,
        })
    }

    fn statement(&mut self) -> Result<Statement, ParseError> {
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
            return Ok(Statement::Let {
                name,
                mutable,
                ty,
                value,
                line: token.line,
                column: token.column,
            });
        }
        if self.consume(TokenKind::Return) {
            return Ok(Statement::Return(self.expr(0)?));
        }
        if self.consume(TokenKind::While) {
            let condition = self.expr(0)?;
            self.expect(TokenKind::Newline)?;
            return Ok(Statement::While {
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
            return Ok(Statement::For {
                name,
                start,
                end,
                inclusive,
                body: self.block()?,
            });
        }
        if self.at(TokenKind::Ident)
            && self
                .tokens
                .get(self.index + 1)
                .is_some_and(|token| token.kind == TokenKind::Dot)
            && self
                .tokens
                .get(self.index + 2)
                .is_some_and(|token| token.kind == TokenKind::Ident)
            && self
                .tokens
                .get(self.index + 3)
                .is_some_and(|token| token.kind == TokenKind::Eq)
        {
            let base = Expr::Name(self.ident()?);
            self.expect(TokenKind::Dot)?;
            let name = self.ident()?;
            self.expect(TokenKind::Eq)?;
            return Ok(Statement::AssignField {
                base,
                name,
                value: self.expr(0)?,
            });
        }
        if self.at(TokenKind::Ident)
            && self
                .tokens
                .get(self.index + 1)
                .is_some_and(|token| token.kind == TokenKind::Eq)
        {
            let name = self.ident()?;
            self.expect(TokenKind::Eq)?;
            return Ok(Statement::Assign {
                name,
                value: self.expr(0)?,
            });
        }
        if self.at(TokenKind::Ident)
            && self
                .tokens
                .get(self.index + 1)
                .is_some_and(|token| token.kind == TokenKind::LBracket)
        {
            let base = Expr::Name(self.ident()?);
            self.expect(TokenKind::LBracket)?;
            let index = self.expr(0)?;
            self.expect(TokenKind::RBracket)?;
            self.expect(TokenKind::Eq)?;
            return Ok(Statement::AssignIndex {
                base,
                index,
                value: self.expr(0)?,
            });
        }
        Ok(Statement::Expr(self.expr(0)?))
    }

    fn expr(&mut self, min_precedence: u8) -> Result<Expr, ParseError> {
        let mut left = match self.kind() {
            TokenKind::Minus => {
                self.bump();
                Expr::Unary {
                    op: UnaryOp::Neg,
                    value: Box::new(self.expr(8)?),
                }
            }
            TokenKind::Not => {
                self.bump();
                Expr::Unary {
                    op: UnaryOp::Not,
                    value: Box::new(self.expr(8)?),
                }
            }
            TokenKind::If => self.if_expr()?,
            TokenKind::Match => self.match_expr()?,
            TokenKind::Fn => self.lambda()?,
            TokenKind::Int => {
                let token = self.bump();
                let text = self.text(token).replace('_', "");
                Expr::Int(text.parse().map_err(|_| self.error("invalid integer"))?)
            }
            TokenKind::Float => {
                let token = self.bump();
                let text = self.text(token).replace('_', "");
                Expr::Float(text.parse().map_err(|_| self.error("invalid float"))?)
            }
            TokenKind::String => {
                let token = self.bump();
                let text = self.text(token);
                Expr::String(text[1..text.len() - 1].to_owned())
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
                    Expr::ArrayRepeat {
                        value: Box::new(values.pop().unwrap()),
                        length: Box::new(self.expr(7)?),
                    }
                } else {
                    Expr::Array(values)
                }
            }
            TokenKind::True => {
                self.bump();
                Expr::Bool(true)
            }
            TokenKind::False => {
                self.bump();
                Expr::Bool(false)
            }
            TokenKind::Ident | TokenKind::Some | TokenKind::None => Expr::Name(self.ident()?),
            TokenKind::LParen => {
                self.bump();
                let expr = self.expr(0)?;
                self.expect(TokenKind::RParen)?;
                expr
            }
            _ => return Err(self.error("expected expression")),
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
                left = Expr::Call {
                    callee: Box::new(left),
                    args,
                };
                continue;
            }
            if self.consume(TokenKind::Dot) {
                left = Expr::Field {
                    base: Box::new(left),
                    name: self.ident()?,
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
                left = Expr::Method {
                    base: Box::new(left),
                    name,
                    args,
                };
                continue;
            }
            if self.consume(TokenKind::LBracket) {
                let index = self.expr(0)?;
                self.expect(TokenKind::RBracket)?;
                left = Expr::Index {
                    base: Box::new(left),
                    index: Box::new(index),
                };
                continue;
            }
            if self.consume(TokenKind::LBrace) {
                let Expr::Name(name) = left else {
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
                left = Expr::Struct { name, fields };
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
            left = Expr::Binary {
                left: Box::new(left),
                op,
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn if_expr(&mut self) -> Result<Expr, ParseError> {
        self.expect(TokenKind::If)?;
        let condition = self.expr(0)?;
        self.expect(TokenKind::Newline)?;
        let then_body = self.block()?;
        self.expect(TokenKind::Else)?;
        self.expect(TokenKind::Newline)?;
        let else_body = self.block()?;
        Ok(Expr::If {
            condition: Box::new(condition),
            then_body,
            else_body,
        })
    }

    fn match_expr(&mut self) -> Result<Expr, ParseError> {
        self.expect(TokenKind::Match)?;
        let value = self.expr(0)?;
        self.expect(TokenKind::Newline)?;
        self.expect(TokenKind::Indent)?;
        let mut arms = Vec::new();
        self.skip_newlines();
        while !self.at(TokenKind::Dedent) && !self.at(TokenKind::Eof) {
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
            self.end_line()?;
            arms.push(MatchArm {
                variant,
                binding,
                body,
            });
            self.skip_newlines();
        }
        self.expect(TokenKind::Dedent)?;
        Ok(Expr::Match {
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
            let has_nested_block = matches!(
                statement,
                Statement::While { .. } | Statement::For { .. } | Statement::Expr(Expr::If { .. })
            );
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
        Ok(match name.as_str() {
            "void" => Type::Void,
            "bool" => Type::Bool,
            "i8" | "i16" | "i32" | "i64" | "u8" | "u16" | "u32" | "u64" | "usize" | "isize" => {
                Type::Int
            }
            "f32" | "f64" => Type::Float,
            "string" => Type::String,
            _ => Type::Named(name),
        })
    }

    fn lambda(&mut self) -> Result<Expr, ParseError> {
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
        Ok(Expr::Lambda {
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
        let token = self.peek();
        ParseError {
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
            module.functions[0].body[0],
            Statement::Expr(Expr::If { .. })
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
            module.functions[0].body[1],
            Statement::AssignField { .. }
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
            module.functions[0].body[1],
            Statement::Expr(Expr::Match { .. })
        ));
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
            module.functions[0].body[0],
            Statement::Let {
                ty: Some(Type::Array(_)),
                value: Expr::ArrayRepeat { .. },
                ..
            }
        ));
    }
}
