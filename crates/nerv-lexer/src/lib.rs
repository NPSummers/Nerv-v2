use std::{collections::VecDeque, ops::Range};

use logos::Logos;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    Int,
    Float,
    String,
    Char,
    True,
    False,
    Let,
    Mut,
    Fn,
    Struct,
    Enum,
    Trait,
    Impl,
    Import,
    Export,
    Module,
    Use,
    If,
    Else,
    For,
    In,
    While,
    Match,
    Test,
    Bench,
    Unsafe,
    Return,
    And,
    Or,
    Not,
    Some,
    None,
    Implements,
    Ident,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    EqEq,
    NotEq,
    Lt,
    Gt,
    LtEq,
    GtEq,
    Eq,
    FatArrow,
    ThinArrow,
    PipeArrow,
    LParen,
    RParen,
    LBracket,
    RBracket,
    LBrace,
    RBrace,
    Comma,
    Dot,
    Range,
    RangeInclusive,
    Colon,
    ColonColon,
    Semicolon,
    Ampersand,
    Pipe,
    Question,
    Newline,
    Indent,
    Dedent,
    Eof,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Range<usize>,
    pub line: usize,
    pub column: usize,
}

impl Token {
    pub fn text<'a>(&self, source: &'a str) -> &'a str {
        &source[self.span.clone()]
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LexError {
    pub span: Range<usize>,
    pub line: usize,
    pub column: usize,
}

impl std::fmt::Display for LexError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("invalid token")
    }
}

impl std::error::Error for LexError {}

#[derive(Logos, Debug, Clone, Copy, PartialEq, Eq)]
enum RawKind {
    #[regex(r"[ \t]+")]
    Space,
    #[regex(r"\r\n|\n|\r")]
    Newline,
    #[regex(r"--[^\r\n]*", allow_greedy = true)]
    Comment,
    #[regex(r#"\"([^\"\\\r\n]|\\.)*\""#)]
    String,
    #[regex(r"'([^'\\\r\n]|\\.)'")]
    Char,
    #[regex(r"[0-9][0-9_]*\.[0-9_]+([eE][+-]?[0-9_]+)?")]
    #[regex(r"[0-9][0-9_]*[eE][+-]?[0-9_]+")]
    Float,
    #[regex(r"[0-9][0-9_]*")]
    Int,
    #[regex(r"[A-Za-z_][A-Za-z0-9_]*")]
    Ident,
    #[token("..=")]
    RangeInclusive,
    #[token("=>")]
    FatArrow,
    #[token("->")]
    ThinArrow,
    #[token("|>")]
    PipeArrow,
    #[token("==")]
    EqEq,
    #[token("!=")]
    NotEq,
    #[token("<=")]
    LtEq,
    #[token(">=")]
    GtEq,
    #[token("::")]
    ColonColon,
    #[token("..")]
    Range,
    #[token("+")]
    Plus,
    #[token("-")]
    Minus,
    #[token("*")]
    Star,
    #[token("/")]
    Slash,
    #[token("%")]
    Percent,
    #[token("<")]
    Lt,
    #[token(">")]
    Gt,
    #[token("=")]
    Eq,
    #[token("(")]
    LParen,
    #[token(")")]
    RParen,
    #[token("[")]
    LBracket,
    #[token("]")]
    RBracket,
    #[token("{")]
    LBrace,
    #[token("}")]
    RBrace,
    #[token(",")]
    Comma,
    #[token(".")]
    Dot,
    #[token(":")]
    Colon,
    #[token(";")]
    Semicolon,
    #[token("&")]
    Ampersand,
    #[token("|")]
    Pipe,
    #[token("?")]
    Question,
}

pub struct Lexer<'source> {
    source: &'source str,
    raw: logos::Lexer<'source, RawKind>,
    pending: VecDeque<Token>,
    indents: Vec<usize>,
    line_start: bool,
    line_has_token: bool,
    indentation: usize,
    delimiters: usize,
    finished: bool,
}

impl<'source> Lexer<'source> {
    pub fn new(source: &'source str) -> Self {
        Self {
            source,
            raw: RawKind::lexer(source),
            pending: VecDeque::new(),
            indents: vec![0],
            line_start: true,
            line_has_token: false,
            indentation: 0,
            delimiters: 0,
            finished: false,
        }
    }

    fn location(&self, offset: usize) -> (usize, usize) {
        let prefix = &self.source[..offset];
        let line = prefix.bytes().filter(|byte| *byte == b'\n').count() + 1;
        let column = prefix
            .rsplit('\n')
            .next()
            .unwrap_or_default()
            .chars()
            .count()
            + 1;
        (line, column)
    }

    fn token(&self, kind: TokenKind, span: Range<usize>) -> Token {
        let (line, column) = self.location(span.start);
        Token {
            kind,
            span,
            line,
            column,
        }
    }

    fn layout(&mut self, span: Range<usize>) {
        let current = *self
            .indents
            .last()
            .expect("indentation stack is never empty");
        let marker = span.start..span.start;
        if self.indentation > current {
            self.indents.push(self.indentation);
            self.pending
                .push_back(self.token(TokenKind::Indent, marker));
        } else if self.indentation < current {
            while self.indents.len() > 1 && self.indentation < *self.indents.last().unwrap() {
                self.indents.pop();
                self.pending
                    .push_back(self.token(TokenKind::Dedent, marker.clone()));
            }
        }
    }

    fn finish(&mut self) -> Option<Result<Token, LexError>> {
        if self.indents.len() > 1 {
            self.indents.pop();
            return Some(Ok(
                self.token(TokenKind::Dedent, self.source.len()..self.source.len())
            ));
        }
        if !self.finished {
            self.finished = true;
            return Some(Ok(
                self.token(TokenKind::Eof, self.source.len()..self.source.len())
            ));
        }
        None
    }
}

impl Iterator for Lexer<'_> {
    type Item = Result<Token, LexError>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(token) = self.pending.pop_front() {
                return Some(Ok(token));
            }

            let Some(raw) = self.raw.next() else {
                return self.finish();
            };
            let span = self.raw.span();
            let raw = match raw {
                Ok(raw) => raw,
                Err(_) => {
                    let (line, column) = self.location(span.start);
                    return Some(Err(LexError { span, line, column }));
                }
            };

            match raw {
                RawKind::Space => {
                    if self.line_start && self.delimiters == 0 {
                        self.indentation += self
                            .raw
                            .slice()
                            .chars()
                            .map(|ch| if ch == '\t' { 4 } else { 1 })
                            .sum::<usize>();
                    }
                    continue;
                }
                RawKind::Comment => continue,
                RawKind::Newline => {
                    let emit = self.line_has_token && self.delimiters == 0;
                    self.line_start = true;
                    self.line_has_token = false;
                    self.indentation = 0;
                    if emit {
                        return Some(Ok(self.token(TokenKind::Newline, span)));
                    }
                    continue;
                }
                _ => {}
            }

            let kind = match raw {
                RawKind::String => TokenKind::String,
                RawKind::Char => TokenKind::Char,
                RawKind::Float => TokenKind::Float,
                RawKind::Int => TokenKind::Int,
                RawKind::Ident => keyword(self.raw.slice()),
                RawKind::RangeInclusive => TokenKind::RangeInclusive,
                RawKind::FatArrow => TokenKind::FatArrow,
                RawKind::ThinArrow => TokenKind::ThinArrow,
                RawKind::PipeArrow => TokenKind::PipeArrow,
                RawKind::EqEq => TokenKind::EqEq,
                RawKind::NotEq => TokenKind::NotEq,
                RawKind::LtEq => TokenKind::LtEq,
                RawKind::GtEq => TokenKind::GtEq,
                RawKind::ColonColon => TokenKind::ColonColon,
                RawKind::Range => TokenKind::Range,
                RawKind::Plus => TokenKind::Plus,
                RawKind::Minus => TokenKind::Minus,
                RawKind::Star => TokenKind::Star,
                RawKind::Slash => TokenKind::Slash,
                RawKind::Percent => TokenKind::Percent,
                RawKind::Lt => TokenKind::Lt,
                RawKind::Gt => TokenKind::Gt,
                RawKind::Eq => TokenKind::Eq,
                RawKind::LParen => TokenKind::LParen,
                RawKind::RParen => TokenKind::RParen,
                RawKind::LBracket => TokenKind::LBracket,
                RawKind::RBracket => TokenKind::RBracket,
                RawKind::LBrace => TokenKind::LBrace,
                RawKind::RBrace => TokenKind::RBrace,
                RawKind::Comma => TokenKind::Comma,
                RawKind::Dot => TokenKind::Dot,
                RawKind::Colon => TokenKind::Colon,
                RawKind::Semicolon => TokenKind::Semicolon,
                RawKind::Ampersand => TokenKind::Ampersand,
                RawKind::Pipe => TokenKind::Pipe,
                RawKind::Question => TokenKind::Question,
                RawKind::Space | RawKind::Newline | RawKind::Comment => unreachable!(),
            };

            if self.line_start && self.delimiters == 0 {
                self.layout(span.clone());
            }
            self.line_start = false;
            self.line_has_token = true;
            match kind {
                TokenKind::LParen | TokenKind::LBracket | TokenKind::LBrace => self.delimiters += 1,
                TokenKind::RParen | TokenKind::RBracket | TokenKind::RBrace => {
                    self.delimiters = self.delimiters.saturating_sub(1)
                }
                _ => {}
            }
            self.pending.push_back(self.token(kind, span));
        }
    }
}

fn keyword(value: &str) -> TokenKind {
    match value {
        "let" => TokenKind::Let,
        "mut" => TokenKind::Mut,
        "fn" => TokenKind::Fn,
        "struct" => TokenKind::Struct,
        "enum" => TokenKind::Enum,
        "trait" => TokenKind::Trait,
        "impl" => TokenKind::Impl,
        "import" => TokenKind::Import,
        "export" => TokenKind::Export,
        "module" => TokenKind::Module,
        "use" => TokenKind::Use,
        "if" => TokenKind::If,
        "else" => TokenKind::Else,
        "for" => TokenKind::For,
        "in" => TokenKind::In,
        "while" => TokenKind::While,
        "match" => TokenKind::Match,
        "test" => TokenKind::Test,
        "bench" => TokenKind::Bench,
        "unsafe" => TokenKind::Unsafe,
        "return" => TokenKind::Return,
        "and" => TokenKind::And,
        "or" => TokenKind::Or,
        "not" => TokenKind::Not,
        "true" => TokenKind::True,
        "false" => TokenKind::False,
        "Some" => TokenKind::Some,
        "None" => TokenKind::None,
        "implements" => TokenKind::Implements,
        _ => TokenKind::Ident,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(source: &str) -> Vec<TokenKind> {
        Lexer::new(source)
            .map(|token| token.unwrap().kind)
            .collect()
    }

    #[test]
    fn lexes_keywords_and_operators() {
        assert_eq!(
            kinds("fn main() -> void\n    let value = 1..=3\n"),
            vec![
                TokenKind::Fn,
                TokenKind::Ident,
                TokenKind::LParen,
                TokenKind::RParen,
                TokenKind::ThinArrow,
                TokenKind::Ident,
                TokenKind::Newline,
                TokenKind::Indent,
                TokenKind::Let,
                TokenKind::Ident,
                TokenKind::Eq,
                TokenKind::Int,
                TokenKind::RangeInclusive,
                TokenKind::Int,
                TokenKind::Newline,
                TokenKind::Dedent,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn ignores_comments_and_blank_lines() {
        assert_eq!(
            kinds("-- note\n\nlet x = Some(1)\n"),
            vec![
                TokenKind::Let,
                TokenKind::Ident,
                TokenKind::Eq,
                TokenKind::Some,
                TokenKind::LParen,
                TokenKind::Int,
                TokenKind::RParen,
                TokenKind::Newline,
                TokenKind::Eof,
            ]
        );
    }
}
