//! Dynare 7.2's semi-structural commands and the independent trend block.

use super::Parser;
use crate::expr::{ExprId, ExprKind};
use crate::intern::Name;
use crate::lexer::{Token, TokenKind};
use crate::model::{
    DeterministicTrendRow, DeterministicTrendsBlock, NamedModelOperator, NamedModelOperatorKind,
    PacTargetComponent, PacTargetComponentRow, PacTargetInfoBlock, PacTargetInfoRow, ParseIssue,
    ParseIssueKind, SemiStructuralCommand, SemiStructuralKind, SemiStructuralOption,
    SemiStructuralValue, WrittenExpression,
};
use crate::span::Span;

pub(super) fn named_operator_kind(name: &str) -> Option<NamedModelOperatorKind> {
    if name.eq_ignore_ascii_case("var_expectation") {
        Some(NamedModelOperatorKind::VarExpectation)
    } else if name.eq_ignore_ascii_case("pac_expectation") {
        Some(NamedModelOperatorKind::PacExpectation)
    } else if name.eq_ignore_ascii_case("pac_target_nonstationary") {
        Some(NamedModelOperatorKind::PacTargetNonstationary)
    } else {
        None
    }
}

impl Parser<'_> {
    pub(super) fn at_semi_structural_command(&self) -> Option<SemiStructuralKind> {
        if self.at_ident_ci("var_model") {
            Some(SemiStructuralKind::VarModel)
        } else if self.at_ident_ci("trend_component_model") {
            Some(SemiStructuralKind::TrendComponentModel)
        } else if self.at_ident_ci("var_expectation_model") {
            Some(SemiStructuralKind::VarExpectationModel)
        } else if self.at_ident_ci("pac_model") {
            Some(SemiStructuralKind::PacModel)
        } else {
            None
        }
    }

    pub(super) fn parse_semi_structural_command(&mut self, kind: SemiStructuralKind) {
        let start = self.bump().span.start;
        let mut options = Vec::new();
        if !self.at(TokenKind::LParen) {
            self.pac_syntax("'('");
            self.skip_until_semi();
            return;
        }
        self.bump();
        if self.at(TokenKind::RParen) {
            self.pac_syntax(self.pac_option_expectation(kind));
        }
        while !self.at(TokenKind::RParen) && !self.at(TokenKind::Semi) && !self.at(TokenKind::Eof) {
            let before = self.i;
            if let Some(option) = self.parse_semi_structural_option(kind) {
                if !allows_repeated_option(kind, &option.name)
                    && options.iter().any(|prior: &SemiStructuralOption| {
                        prior.name.eq_ignore_ascii_case(&option.name)
                    })
                {
                    self.model
                        .option_twice
                        .push((official_option_key(kind, &option.name), option.span));
                }
                options.push(option);
            }
            if self.at(TokenKind::Comma) {
                self.bump();
                if self.at(TokenKind::RParen) {
                    self.pac_syntax(self.pac_option_expectation(kind));
                }
            } else if !self.at(TokenKind::RParen)
                && !self.at(TokenKind::Semi)
                && !self.at(TokenKind::Eof)
            {
                self.pac_syntax("COMMA or ')' ");
                self.recover_pac_option();
                self.eat(TokenKind::Comma);
            }
            if self.i == before {
                self.bump();
            }
        }
        if !self.at(TokenKind::RParen) {
            self.pac_syntax("')'");
        } else {
            self.bump();
        }
        if !self.at(TokenKind::Semi) {
            self.pac_syntax("';'");
        }
        let end = if self.at(TokenKind::Semi) {
            self.bump().span.end
        } else {
            self.tokens[self.i.saturating_sub(1)].span.end
        };
        self.model
            .semi_structural_commands
            .push(SemiStructuralCommand {
                kind,
                span: Span { start, end },
                options,
            });
    }

    fn pac_option_expectation(&self, kind: SemiStructuralKind) -> &'static str {
        match kind {
            SemiStructuralKind::VarModel => "MODEL_NAME or EQTAGS or STRUCTURAL",
            SemiStructuralKind::TrendComponentModel => "MODEL_NAME or TARGETS or EQTAGS",
            SemiStructuralKind::VarExpectationModel => {
                "VARIABLE or EXPRESSION or AUXILIARY_MODEL_NAME or HORIZON"
            }
            SemiStructuralKind::PacModel => {
                "MODEL_NAME or AUXILIARY_MODEL_NAME or DISCOUNT or GROWTH"
            }
        }
    }

    fn parse_semi_structural_option(
        &mut self,
        kind: SemiStructuralKind,
    ) -> Option<SemiStructuralOption> {
        if !self.at(TokenKind::Ident) {
            self.pac_syntax(self.pac_option_expectation(kind));
            self.recover_pac_option();
            return None;
        }
        let token = self.bump();
        let word = token.text(self.src).to_ascii_lowercase();
        let value_type = match (kind, word.as_str()) {
            (SemiStructuralKind::VarModel, "structural") => PacOptionType::Flag,
            (SemiStructuralKind::VarModel, "model_name")
            | (SemiStructuralKind::TrendComponentModel, "model_name")
            | (SemiStructuralKind::VarExpectationModel, "variable")
            | (SemiStructuralKind::VarExpectationModel, "auxiliary_model_name")
            | (SemiStructuralKind::VarExpectationModel, "model_name")
            | (SemiStructuralKind::PacModel, "model_name")
            | (SemiStructuralKind::PacModel, "auxiliary_model_name")
            | (SemiStructuralKind::PacModel, "discount")
            | (SemiStructuralKind::PacModel, "auxname") => PacOptionType::Symbol,
            (SemiStructuralKind::VarModel, "eqtags")
            | (SemiStructuralKind::TrendComponentModel, "eqtags")
            | (SemiStructuralKind::TrendComponentModel, "targets") => PacOptionType::Tags,
            (SemiStructuralKind::VarExpectationModel, "expression")
            | (SemiStructuralKind::VarExpectationModel, "discount")
            | (SemiStructuralKind::PacModel, "growth") => PacOptionType::Expression,
            (SemiStructuralKind::VarExpectationModel, "horizon") => PacOptionType::Horizon,
            (SemiStructuralKind::VarExpectationModel, "time_shift") => PacOptionType::SignedInteger,
            (SemiStructuralKind::PacModel, "kind") => PacOptionType::Kind,
            _ => {
                self.pac_syntax_at(self.i - 1, self.pac_option_expectation(kind));
                self.recover_pac_option();
                return None;
            }
        };
        if value_type == PacOptionType::Flag {
            return Some(SemiStructuralOption {
                name: token.text(self.src).to_string(),
                span: token.span,
                value: SemiStructuralValue::Flag,
            });
        }
        if !self.at(TokenKind::Eq) {
            self.pac_syntax("EQUAL");
            return None;
        }
        self.bump();
        let value = match value_type {
            PacOptionType::Flag => unreachable!(),
            PacOptionType::Symbol => self
                .read_pac_symbol()
                .map(|(name, span)| SemiStructuralValue::Symbol { name, span }),
            PacOptionType::Tags => self.read_pac_tag_list().map(SemiStructuralValue::Tags),
            PacOptionType::Expression => self
                .read_pac_expression()
                .map(SemiStructuralValue::Expression),
            PacOptionType::Horizon => self.read_pac_horizon(),
            PacOptionType::SignedInteger => self.read_pac_integer(true),
            PacOptionType::Kind => self
                .read_pac_kind()
                .map(|(text, span)| SemiStructuralValue::Kind { text, span }),
        }?;
        Some(SemiStructuralOption {
            name: token.text(self.src).to_string(),
            span: token.span,
            value,
        })
    }

    fn read_pac_symbol(&mut self) -> Option<(Name, Span)> {
        if !self.at(TokenKind::Ident) || is_pac_reserved(self.tokens[self.i].text(self.src)) {
            self.pac_syntax("IDENTIFIER");
            return None;
        }
        let token = self.bump();
        Some((self.intern.intern(token.text(self.src)), token.span))
    }

    fn read_pac_kind(&mut self) -> Option<(String, Span)> {
        if !self.at(TokenKind::Ident) {
            self.pac_syntax("LL or DL or DD");
            return None;
        }
        let token = self.tokens[self.i].clone();
        let word = token.text(self.src);
        if !matches!(word.to_ascii_lowercase().as_str(), "ll" | "dl" | "dd") {
            self.pac_syntax("LL or DL or DD");
            return None;
        }
        self.bump();
        Some((word.to_string(), token.span))
    }

    fn read_pac_tag_list(&mut self) -> Option<Vec<(String, Span)>> {
        if !self.at(TokenKind::LBrack) {
            self.pac_syntax("'['");
            return None;
        }
        self.bump();
        self.eat(TokenKind::Comma);
        let mut tags = Vec::new();
        loop {
            if !self.at(TokenKind::String) {
                if tags.is_empty() || !self.at(TokenKind::RBrack) {
                    self.pac_syntax(if tags.is_empty() {
                        "COMMA or QUOTED_STRING"
                    } else {
                        "QUOTED_STRING"
                    });
                }
                break;
            }
            let token = self.bump();
            tags.push((super::unquote_string(token.text(self.src)), token.span));
            if self.at(TokenKind::Comma) {
                self.bump();
                if self.at(TokenKind::RBrack) {
                    break;
                }
            } else if !self.at(TokenKind::String) {
                break;
            }
        }
        if !self.at(TokenKind::RBrack) {
            self.pac_syntax("']'");
            return None;
        }
        self.bump();
        Some(tags)
    }

    fn read_pac_horizon(&mut self) -> Option<SemiStructuralValue> {
        let token = self.tokens[self.i].clone();
        if token.kind != TokenKind::Number || !super::is_integer_lexeme(token.text(self.src)) {
            self.pac_syntax("INT_NUMBER");
            return None;
        }
        self.bump();
        let first = token.text(self.src).to_string();
        if self.at(TokenKind::Number) || self.at_ident_ci("inf") {
            let last_token = self.tokens[self.i].clone();
            let between = &self.src[token.span.end as usize..last_token.span.start as usize];
            if between.trim() == ":" {
                if last_token.kind == TokenKind::Number
                    && !super::is_integer_lexeme(last_token.text(self.src))
                {
                    self.pac_syntax("INT_NUMBER or INF_CONSTANT");
                    return None;
                }
                self.bump();
                return Some(SemiStructuralValue::Horizon {
                    first,
                    last: last_token.text(self.src).to_string(),
                    span: Span {
                        start: token.span.start,
                        end: last_token.span.end,
                    },
                });
            }
        }
        Some(SemiStructuralValue::Integer {
            text: first,
            span: token.span,
        })
    }

    fn read_pac_integer(&mut self, signed: bool) -> Option<SemiStructuralValue> {
        let start = self.current_start();
        if signed && (self.at(TokenKind::Plus) || self.at(TokenKind::Minus)) {
            self.bump();
        }
        if !self.at(TokenKind::Number)
            || !super::is_integer_lexeme(self.tokens[self.i].text(self.src))
        {
            self.pac_syntax("INT_NUMBER");
            return None;
        }
        let end = self.bump().span.end;
        let span = Span { start, end };
        Some(SemiStructuralValue::Integer {
            text: self.src[start as usize..end as usize].to_string(),
            span,
        })
    }

    fn read_pac_expression(&mut self) -> Option<WrittenExpression> {
        let start = self.current_start();
        let start_i = self.i;
        let was_in_model = self.in_model;
        self.in_model = true;
        let expr = self.parse_expr();
        self.in_model = was_in_model;
        let Some(id) = expr else {
            self.pac_syntax("");
            return None;
        };
        if let Some(string_i) =
            (start_i..self.i).find(|&i| self.tokens[i].kind == TokenKind::String)
        {
            self.pac_syntax_at(string_i, "");
            return None;
        }
        if self.pac_expr_has_error(id) {
            self.pac_syntax("");
            return None;
        }
        let end = self.tokens[self.i.saturating_sub(1)].span.end;
        let span = Span { start, end };
        Some(WrittenExpression {
            text: self
                .src
                .get(start as usize..end as usize)
                .unwrap_or("")
                .to_string(),
            span,
            expr: Some(id),
        })
    }

    fn pac_expr_has_error(&self, id: ExprId) -> bool {
        match &self.model.exprs.get(id).kind {
            ExprKind::Error => true,
            ExprKind::Unary { arg, .. }
            | ExprKind::SteadyState { arg }
            | ExprKind::Expectation { arg, .. } => self.pac_expr_has_error(*arg),
            ExprKind::Binary { lhs, rhs, .. } => {
                self.pac_expr_has_error(*lhs) || self.pac_expr_has_error(*rhs)
            }
            ExprKind::Call { args, .. } => args.iter().any(|arg| self.pac_expr_has_error(*arg)),
            ExprKind::Ident { .. } | ExprKind::Number | ExprKind::String => false,
        }
    }

    fn recover_pac_option(&mut self) {
        while !self.at(TokenKind::Comma)
            && !self.at(TokenKind::RParen)
            && !self.at(TokenKind::Semi)
            && !self.at(TokenKind::Eof)
        {
            if self.at(TokenKind::LParen) {
                self.skip_balanced(TokenKind::LParen, TokenKind::RParen);
            } else if self.at(TokenKind::LBrack) {
                self.skip_balanced(TokenKind::LBrack, TokenKind::RBrack);
            } else {
                self.bump();
            }
        }
    }

    pub(super) fn parse_named_model_operator(
        &mut self,
        keyword: Token,
        callee: Name,
        kind: NamedModelOperatorKind,
    ) -> ExprId {
        self.bump(); // `(`
        let name =
            if self.at(TokenKind::Ident) && !is_pac_reserved(self.tokens[self.i].text(self.src)) {
                self.read_pac_symbol()
            } else {
                self.pac_syntax("");
                None
            };
        if name.is_some() && !self.at(TokenKind::RParen) {
            self.pac_syntax("')'");
        }
        if !self.at(TokenKind::RParen) {
            self.recover_pac_option();
        }
        let end = if self.at(TokenKind::RParen) {
            self.bump().span.end
        } else {
            self.current_start()
        };
        let span = Span {
            start: keyword.span.start,
            end,
        };
        if let Some((name, name_span)) = name {
            self.model.named_model_operators.push(NamedModelOperator {
                kind,
                name,
                operator_span: keyword.span,
                name_span,
                span,
            });
        }
        // The name identifies a model, not a variable reference. Keep the tree
        // leaf call so ordinary symbol walks do not treat it as an undeclared var.
        self.alloc(
            ExprKind::Call {
                callee,
                args: Vec::new(),
            },
            span,
        )
    }

    pub(super) fn parse_pac_target_info_block(&mut self) {
        let start = self.bump().span.start;
        if !self.at(TokenKind::LParen) {
            self.pac_syntax("'('");
            self.skip_until_semi();
            return;
        }
        self.bump();
        let name = self.read_pac_symbol();
        if !self.at(TokenKind::RParen) {
            self.pac_syntax("')'");
        } else {
            self.bump();
        }
        if !self.at(TokenKind::Semi) {
            self.pac_syntax("';'");
        }
        self.eat(TokenKind::Semi);
        let opener = Span {
            start,
            end: self.tokens[self.i.saturating_sub(1)].span.end,
        };
        let body_i = self.i;
        let body_end_i = self.consume_until_end();
        self.record_missing_end_if_unclosed("pac_target_info", opener, body_i, body_end_i);
        let end = self.block_end_after_consume();
        let saved = self.i;
        self.i = body_i;
        let mut rows = Vec::new();
        if body_i == body_end_i {
            self.pac_syntax("COMPONENT or TARGET or AUXNAME_TARGET_NONSTATIONARY");
        }
        while self.i < body_end_i {
            let before = self.i;
            if self.at_ident_ci("target") {
                self.bump();
                if let Some(expr) = self.read_pac_expression() {
                    rows.push(PacTargetInfoRow::Target(expr));
                }
                self.pac_finish_row(body_end_i);
            } else if self.at_ident_ci("auxname_target_nonstationary") {
                self.bump();
                if let Some((name, span)) = self.read_pac_symbol() {
                    rows.push(PacTargetInfoRow::AuxnameTargetNonstationary { name, span });
                }
                self.pac_finish_row(body_end_i);
            } else if self.at_ident_ci("component") {
                let component_start = self.bump().span.start;
                let component = self.read_pac_expression();
                self.pac_finish_row(body_end_i);
                let mut component_rows = Vec::new();
                while self.i < body_end_i {
                    let row = if self.at_ident_ci("growth") {
                        self.bump();
                        self.read_pac_expression()
                            .map(PacTargetComponentRow::Growth)
                    } else if self.at_ident_ci("auxname") {
                        self.bump();
                        self.read_pac_symbol()
                            .map(|(name, span)| PacTargetComponentRow::Auxname { name, span })
                    } else if self.at_ident_ci("kind") {
                        self.bump();
                        self.read_pac_kind()
                            .map(|(text, span)| PacTargetComponentRow::Kind { text, span })
                    } else {
                        break;
                    };
                    if let Some(row) = row {
                        component_rows.push(row);
                    }
                    self.pac_finish_row(body_end_i);
                }
                if component_rows.is_empty() {
                    self.pac_syntax("GROWTH or AUXNAME or KIND");
                }
                if let Some(component) = component {
                    let component_end = self.tokens[self.i.saturating_sub(1)].span.end;
                    rows.push(PacTargetInfoRow::Component(PacTargetComponent {
                        component,
                        rows: component_rows,
                        span: Span {
                            start: component_start,
                            end: component_end,
                        },
                    }));
                }
            } else {
                self.pac_syntax("TARGET or AUXNAME_TARGET_NONSTATIONARY or COMPONENT");
                self.pac_finish_row(body_end_i);
            }
            if self.i <= before {
                self.bump();
            }
        }
        self.i = saved;
        if let Some((name, name_span)) = name {
            self.model.pac_target_info.push(PacTargetInfoBlock {
                name,
                name_span,
                span: Span { start, end },
                rows,
            });
        }
    }

    pub(super) fn parse_deterministic_trends_block(&mut self) {
        let start = self.bump().span.start;
        if !self.at(TokenKind::Semi) {
            self.pac_syntax("';'");
            if self.at(TokenKind::LParen) {
                self.skip_balanced(TokenKind::LParen, TokenKind::RParen);
            }
        }
        let opener = Span {
            start,
            end: if self.at(TokenKind::Semi) {
                self.bump().span.end
            } else {
                self.current_start()
            },
        };
        let body_i = self.i;
        let body_end_i = self.consume_until_end();
        self.record_missing_end_if_unclosed("deterministic_trends", opener, body_i, body_end_i);
        let end = self.block_end_after_consume();
        let saved = self.i;
        self.i = body_i;
        let mut rows = Vec::new();
        let mut seen = std::collections::HashSet::new();
        if body_i == body_end_i {
            self.pac_syntax("");
        }
        while self.i < body_end_i {
            let before = self.i;
            let name = self.read_pac_symbol();
            if !self.at(TokenKind::LParen) {
                self.pac_syntax("'('");
                self.pac_finish_row(body_end_i);
                continue;
            }
            self.bump();
            let expression = self.read_pac_expression();
            if !self.at(TokenKind::RParen) {
                self.pac_syntax("')'");
            } else {
                self.bump();
            }
            let end = self.pac_finish_row(body_end_i);
            if let (Some((name, name_span)), Some(expression)) = (name, expression) {
                if !seen.insert(name) {
                    self.model.deterministic_trends_dups.push((name, name_span));
                }
                rows.push(DeterministicTrendRow {
                    name,
                    name_span,
                    expression,
                    span: Span {
                        start: name_span.start,
                        end,
                    },
                });
            }
            if self.i <= before {
                self.bump();
            }
        }
        self.i = saved;
        self.model
            .deterministic_trends
            .push(DeterministicTrendsBlock {
                span: Span {
                    start: opener.start,
                    end,
                },
                rows,
            });
    }

    fn pac_finish_row(&mut self, end_i: usize) -> u32 {
        if self.at(TokenKind::Semi) {
            return self.bump().span.end;
        }
        self.pac_syntax("';'");
        while self.i < end_i && !self.at(TokenKind::Semi) {
            self.bump();
        }
        if self.at(TokenKind::Semi) && self.i < end_i {
            self.bump().span.end
        } else {
            self.tokens[self.i.saturating_sub(1)].span.end
        }
    }

    fn pac_syntax(&mut self, expected: &str) {
        self.pac_syntax_at(self.i, expected);
    }

    fn pac_syntax_at(&mut self, at: usize, expected: &str) {
        if self.has_pac_refusal() {
            return;
        }
        let token = &self.tokens[at];
        let unexpected = pac_token_name(token, self.src);
        let message = if expected.is_empty() {
            format!("syntax error, unexpected {unexpected}")
        } else {
            format!(
                "syntax error, unexpected {unexpected}, expecting {}",
                expected.trim()
            )
        };
        self.model.parse_issues.push(ParseIssue {
            kind: ParseIssueKind::BisonSyntax(message),
            span: token.span,
        });
    }

    fn has_pac_refusal(&self) -> bool {
        self.model
            .parse_issues
            .iter()
            .any(|issue| matches!(issue.kind, ParseIssueKind::BisonSyntax(_)))
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PacOptionType {
    Flag,
    Symbol,
    Tags,
    Expression,
    Horizon,
    SignedInteger,
    Kind,
}

fn pac_token_name(token: &Token, src: &str) -> String {
    match token.kind {
        TokenKind::Ident => {
            let word = token.text(src);
            const WORDS: &[&str] = &[
                "model_name",
                "eqtags",
                "targets",
                "structural",
                "variable",
                "expression",
                "auxiliary_model_name",
                "horizon",
                "discount",
                "time_shift",
                "growth",
                "auxname",
                "kind",
                "target",
                "component",
                "auxname_target_nonstationary",
                "ll",
                "dl",
                "dd",
                "end",
                "inf",
            ];
            if WORDS.iter().any(|known| word.eq_ignore_ascii_case(known)) {
                if word.eq_ignore_ascii_case("inf") {
                    "INF_CONSTANT".to_string()
                } else {
                    word.to_ascii_uppercase()
                }
            } else {
                "IDENTIFIER".to_string()
            }
        }
        TokenKind::Number => {
            if super::is_integer_lexeme(token.text(src)) {
                "INT_NUMBER".to_string()
            } else {
                "FLOAT_NUMBER".to_string()
            }
        }
        TokenKind::String => "QUOTED_STRING".to_string(),
        TokenKind::Eof => "end of file".to_string(),
        TokenKind::Comma => "COMMA".to_string(),
        TokenKind::Eq => "EQUAL".to_string(),
        TokenKind::Plus => "PLUS".to_string(),
        TokenKind::Minus => "MINUS".to_string(),
        TokenKind::LParen => "'('".to_string(),
        TokenKind::RParen => "')'".to_string(),
        TokenKind::LBrack => "'['".to_string(),
        TokenKind::RBrack => "']'".to_string(),
        TokenKind::Semi => "';'".to_string(),
        _ => "IDENTIFIER".to_string(),
    }
}

fn is_pac_reserved(word: &str) -> bool {
    // Words with a DYNARE_STATEMENT or DYNARE_BLOCK lexer rule cannot fill a
    // `symbol` slot in this grammar. Context-free lexer tokens in our parser
    // still need that distinction here.
    const RESERVED: &[&str] = &[
        "target",
        "component",
        "growth",
        "auxname",
        "kind",
        "ll",
        "dl",
        "dd",
        "eqtags",
        "targets",
        "model_name",
        "auxiliary_model_name",
        "variable",
        "expression",
        "discount",
        "horizon",
        "time_shift",
        "structural",
        "inf",
        "end",
    ];
    RESERVED
        .iter()
        .any(|known| word.eq_ignore_ascii_case(known))
}

fn allows_repeated_option(kind: SemiStructuralKind, name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    match kind {
        SemiStructuralKind::VarExpectationModel => {
            matches!(name.as_str(), "expression" | "discount")
        }
        SemiStructuralKind::PacModel => matches!(name.as_str(), "growth" | "auxname" | "kind"),
        _ => false,
    }
}

fn official_option_key(kind: SemiStructuralKind, name: &str) -> String {
    let name = name.to_ascii_lowercase();
    match kind {
        SemiStructuralKind::VarModel => format!("var.{name}"),
        SemiStructuralKind::TrendComponentModel => {
            if name == "model_name" {
                "trend_component.name".to_string()
            } else {
                format!("trend_component.{name}")
            }
        }
        SemiStructuralKind::VarExpectationModel => name.to_string(),
        SemiStructuralKind::PacModel => {
            if name == "auxiliary_model_name" {
                "pac.aux_model_name".to_string()
            } else {
                format!("pac.{name}")
            }
        }
    }
}
