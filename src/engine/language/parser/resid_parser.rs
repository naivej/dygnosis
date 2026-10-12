//! The two resid productions in pinned DynareBison.yy:1080–1086.

use super::{ParseIssue, ParseIssueKind, Parser, TokenKind};

impl Parser<'_> {
    /// Stop the common lexer scan at the first token this grammar could refuse.
    /// A later dropped character must not pre-empt an earlier syntax refusal.
    pub(super) fn resid_syntax_limit(&self) -> usize {
        let kind = |ahead: usize| self.tokens.get(self.i + ahead).map(|token| token.kind);
        if kind(1) != Some(TokenKind::LParen) {
            return self.i + 1;
        }
        if kind(2) != Some(TokenKind::Ident)
            || !self
                .lexeme(&self.tokens[self.i + 2])
                .eq_ignore_ascii_case("non_zero")
        {
            return self.i + 2;
        }
        if kind(3) != Some(TokenKind::RParen) {
            return self.i + 3;
        }
        self.i + 4
    }

    /// Keep refused resid units out of the generic option scanner and statement
    /// records. Only a completed production creates a command.
    pub(super) fn parse_resid(&mut self) -> bool {
        let from = self.i;
        self.bump();
        if self.at(TokenKind::Semi) {
            self.bump();
            return true;
        }
        if !self.at(TokenKind::LParen) {
            let complete = self.at(TokenKind::Eof)
                || (self.at_follower_keyword() && self.at_statement_boundary());
            self.resid_refuse("';' or '('", complete.then_some(from..self.i));
            self.recover_resid();
            return false;
        }
        self.bump();
        if !self.at_ident_ci("non_zero") {
            self.resid_refuse("NON_ZERO", None);
            self.recover_resid();
            return false;
        }
        self.bump();
        if !self.at(TokenKind::RParen) {
            self.resid_refuse("')'", None);
            self.recover_resid();
            return false;
        }
        self.bump();
        if !self.at(TokenKind::Semi) {
            self.resid_refuse("';'", Some(from..self.i));
            // The complete production before `;` is known. Retain the follower
            // for editor recovery instead of scanning it as resid options.
            return false;
        }
        self.bump();
        true
    }

    fn resid_refuse(&mut self, expected: &str, active_tokens: Option<std::ops::Range<usize>>) {
        let token = &self.tokens[self.i];
        let unexpected = match token.kind {
            TokenKind::Ident => super::statement_keywords::keyword_token(self.lexeme(token))
                .unwrap_or_else(|| "IDENTIFIER".to_string()),
            TokenKind::Eof => "end of file".to_string(),
            TokenKind::String => "QUOTED_STRING".to_string(),
            TokenKind::Star => "TIMES".to_string(),
            TokenKind::Slash => "DIVIDE".to_string(),
            TokenKind::Caret => "POWER".to_string(),
            TokenKind::Lt => "LESS".to_string(),
            TokenKind::Gt => "GREATER".to_string(),
            TokenKind::Le => "LESS_EQUAL".to_string(),
            TokenKind::Ge => "GREATER_EQUAL".to_string(),
            TokenKind::EqEq => "EQUAL_EQUAL".to_string(),
            TokenKind::Ne => "EXCLAMATION_EQUAL".to_string(),
            TokenKind::LBrack => "'['".to_string(),
            TokenKind::RBrack => "']'".to_string(),
            TokenKind::Dot => "'.'".to_string(),
            _ => self.bison_token_name(self.i),
        };
        let message = format!("syntax error, unexpected {unexpected}, expecting {expected}");
        let kind = match active_tokens {
            Some(active_tokens) => ParseIssueKind::BisonMissingSemi {
                message,
                active_tokens,
            },
            None => ParseIssueKind::BisonSyntax(message),
        };
        self.record_issue(ParseIssue {
            kind,
            span: token.span,
        });
    }

    fn recover_resid(&mut self) {
        while !self.at(TokenKind::Semi) && !self.at(TokenKind::Eof) {
            if self.at_follower_keyword() && self.at_statement_boundary() {
                return;
            }
            self.bump();
        }
        self.eat(TokenKind::Semi);
    }
}
