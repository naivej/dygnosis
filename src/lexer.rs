//! Byte-oriented lexer. Comments and strings are tokens or skips — never decls.

use crate::span::Span;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokenKind {
    Ident,
    Number,
    String,
    Latex,
    Eq,
    EqEq,
    Ne,
    Lt,
    Gt,
    Le,
    Ge,
    Plus,
    Minus,
    Star,
    Slash,
    Caret,
    Comma,
    Semi,
    LParen,
    RParen,
    LBrack,
    RBrack,
    Hash,
    MacroDir,
    MacroInterp,
    Eof,
}

#[derive(Clone, Debug)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
    /// Synthesized text after expansion; `None` means `src[span]`.
    pub lexeme: Option<String>,
}

impl Token {
    pub fn new(kind: TokenKind, span: Span) -> Self {
        Self {
            kind,
            span,
            lexeme: None,
        }
    }

    pub fn with_lexeme(kind: TokenKind, span: Span, lexeme: impl Into<String>) -> Self {
        Self {
            kind,
            span,
            lexeme: Some(lexeme.into()),
        }
    }

    pub fn text<'a>(&'a self, src: &'a str) -> &'a str {
        match &self.lexeme {
            Some(text) => text.as_str(),
            None => &src[self.span.start as usize..self.span.end as usize],
        }
    }
}

pub fn tokenize(src: &str) -> Vec<Token> {
    let mut lexer = Lexer { src, pos: 0 };
    let mut tokens = Vec::new();
    loop {
        let tok = lexer.next_token();
        let eof = tok.kind == TokenKind::Eof;
        tokens.push(tok);
        if eof {
            break;
        }
    }
    tokens
}

struct Lexer<'a> {
    src: &'a str,
    pos: usize,
}

impl Lexer<'_> {
    fn next_token(&mut self) -> Token {
        self.skip_trivia();
        let start = self.pos;
        if self.pos >= self.src.len() {
            return Token::new(TokenKind::Eof, Span::new(start, start));
        }
        let ch = self.peek();
        let kind = match ch {
            '=' => {
                self.bump();
                if self.peek() == '=' {
                    self.bump();
                    TokenKind::EqEq
                } else {
                    TokenKind::Eq
                }
            }
            '!' if self.peek_nth(1) == Some('=') => {
                self.bump();
                self.bump();
                TokenKind::Ne
            }
            '<' => {
                self.bump();
                if self.peek() == '=' {
                    self.bump();
                    TokenKind::Le
                } else {
                    TokenKind::Lt
                }
            }
            '>' => {
                self.bump();
                if self.peek() == '=' {
                    self.bump();
                    TokenKind::Ge
                } else {
                    TokenKind::Gt
                }
            }
            '+' => {
                self.bump();
                TokenKind::Plus
            }
            '-' => {
                self.bump();
                TokenKind::Minus
            }
            '*' => {
                self.bump();
                TokenKind::Star
            }
            '/' => {
                self.bump();
                TokenKind::Slash
            }
            '^' => {
                self.bump();
                TokenKind::Caret
            }
            ',' => {
                self.bump();
                TokenKind::Comma
            }
            ';' => {
                self.bump();
                TokenKind::Semi
            }
            '(' => {
                self.bump();
                TokenKind::LParen
            }
            ')' => {
                self.bump();
                TokenKind::RParen
            }
            '[' => {
                self.bump();
                TokenKind::LBrack
            }
            ']' => {
                self.bump();
                TokenKind::RBrack
            }
            '#' => {
                self.bump();
                TokenKind::Hash
            }
            '@' if self.starts("@#") => {
                self.scan_macro_dir();
                TokenKind::MacroDir
            }
            '@' if self.starts("@{") => {
                self.scan_macro_interp();
                TokenKind::MacroInterp
            }
            '\'' | '"' => {
                self.scan_string(ch);
                TokenKind::String
            }
            '$' => {
                self.scan_latex();
                TokenKind::Latex
            }
            '0'..='9' => {
                self.scan_number();
                TokenKind::Number
            }
            '.' if self.peek_nth(1).is_some_and(|c| c.is_ascii_digit()) => {
                self.scan_number();
                TokenKind::Number
            }
            'A'..='Z' | 'a'..='z' => {
                self.scan_ident();
                TokenKind::Ident
            }
            _ => {
                self.bump();
                return self.next_token();
            }
        };
        Token::new(kind, Span::new(start, self.pos))
    }

    fn skip_trivia(&mut self) {
        loop {
            self.skip_ws();
            if self.starts("//") {
                self.skip_line();
                continue;
            }
            if self.peek() == '%' {
                self.skip_line();
                continue;
            }
            if self.starts("/*") {
                self.pos += 2;
                if let Some(end) = self.src[self.pos..].find("*/") {
                    self.pos += end + 2;
                } else {
                    self.pos = self.src.len();
                }
                continue;
            }
            break;
        }
    }

    fn skip_ws(&mut self) {
        while matches!(self.peek(), ' ' | '\t' | '\n' | '\r') {
            self.bump();
        }
    }

    fn skip_line(&mut self) {
        while self.pos < self.src.len() && self.peek() != '\n' {
            self.bump();
        }
    }

    fn scan_macro_dir(&mut self) {
        // `@#` plus backslash-continued lines — one token.
        loop {
            let line_start = self.pos;
            self.skip_line();
            let line = &self.src[line_start..self.pos];
            if !line.trim_end().ends_with('\\') {
                break;
            }
            if self.peek() == '\n' {
                self.bump();
            }
        }
    }

    fn scan_macro_interp(&mut self) {
        self.pos += 2;
        if let Some(end) = self.src[self.pos..].find('}') {
            self.pos += end + 1;
        } else {
            self.pos = self.src.len();
        }
    }

    fn scan_string(&mut self, quote: char) {
        self.bump();
        while self.pos < self.src.len() {
            let c = self.peek();
            if c == quote || c == '\n' {
                if c == quote {
                    self.bump();
                }
                return;
            }
            self.bump();
        }
    }

    fn scan_latex(&mut self) {
        self.bump();
        while self.pos < self.src.len() {
            let c = self.peek();
            if c == '$' {
                self.bump();
                return;
            }
            if c == '\n' {
                return;
            }
            self.bump();
        }
    }

    fn scan_number(&mut self) {
        if self.peek() == '.' {
            self.bump();
        }
        while self.peek().is_ascii_digit() {
            self.bump();
        }
        if self.peek() == '.' && self.peek_nth(1).is_some_and(|c| c.is_ascii_digit()) {
            self.bump();
            while self.peek().is_ascii_digit() {
                self.bump();
            }
        }
        if matches!(self.peek(), 'e' | 'E') {
            let save = self.pos;
            self.bump();
            if matches!(self.peek(), '+' | '-') {
                self.bump();
            }
            if self.peek().is_ascii_digit() {
                while self.peek().is_ascii_digit() {
                    self.bump();
                }
            } else {
                self.pos = save;
            }
        }
    }

    fn scan_ident(&mut self) {
        self.bump();
        while matches!(self.peek(), 'A'..='Z' | 'a'..='z' | '0'..='9' | '_') {
            self.bump();
        }
    }

    fn peek(&self) -> char {
        self.src[self.pos..].chars().next().unwrap_or('\0')
    }

    fn peek_nth(&self, n: usize) -> Option<char> {
        self.src[self.pos..].chars().nth(n)
    }

    fn starts(&self, s: &str) -> bool {
        self.src[self.pos..].starts_with(s)
    }

    fn bump(&mut self) {
        if let Some(c) = self.src[self.pos..].chars().next() {
            self.pos += c.len_utf8();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(src: &str) -> Vec<TokenKind> {
        tokenize(src).into_iter().map(|t| t.kind).collect()
    }

    #[test]
    fn comparison_ops_are_tokens_not_eq() {
        let ks = kinds("a>=b==c");
        assert_eq!(
            ks,
            [
                TokenKind::Ident,
                TokenKind::Ge,
                TokenKind::Ident,
                TokenKind::EqEq,
                TokenKind::Ident,
                TokenKind::Eof,
            ]
        );
        assert!(!ks.contains(&TokenKind::Eq));
        assert_eq!(
            kinds("a!=b<=c>d<e=f"),
            [
                TokenKind::Ident,
                TokenKind::Ne,
                TokenKind::Ident,
                TokenKind::Le,
                TokenKind::Ident,
                TokenKind::Gt,
                TokenKind::Ident,
                TokenKind::Lt,
                TokenKind::Ident,
                TokenKind::Eq,
                TokenKind::Ident,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn ident_number_comment_string_unchanged() {
        let tokens = tokenize("foo 12.5 // c\n 's' \"t\" /* x */ bar");
        let ks: Vec<_> = tokens.iter().map(|t| t.kind).collect();
        assert_eq!(
            ks,
            [
                TokenKind::Ident,
                TokenKind::Number,
                TokenKind::String,
                TokenKind::String,
                TokenKind::Ident,
                TokenKind::Eof,
            ]
        );
        let src = "foo 12.5 // c\n 's' \"t\" /* x */ bar";
        assert_eq!(
            &src[tokens[0].span.start as usize..tokens[0].span.end as usize],
            "foo"
        );
        assert_eq!(
            &src[tokens[1].span.start as usize..tokens[1].span.end as usize],
            "12.5"
        );
        assert_eq!(
            &src[tokens[2].span.start as usize..tokens[2].span.end as usize],
            "'s'"
        );
        assert_eq!(
            &src[tokens[3].span.start as usize..tokens[3].span.end as usize],
            "\"t\""
        );
        assert_eq!(
            &src[tokens[4].span.start as usize..tokens[4].span.end as usize],
            "bar"
        );
    }
}
