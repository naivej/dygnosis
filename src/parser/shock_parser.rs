//! Written Dynare 7.2 shock, path, and date forms. Semantic refusals are
//! checked by the diagnostic passes; this module keeps the file's structure.

use super::*;
use crate::model::{
    DatabaseDeclaration, DateExpr, DateOption, EndvalEntry, EndvalInstruction, PathBlock,
    PathReference, PathStanza, PathTarget, PeriodPoint, PeriodRange, ScheduledShock,
    SetTimeStatement, ShockBlock, ShockBlockKind, ShockOperation, ShockOptions, StochSimulRequest,
    SubsampleHead, SubsampleInstruction, SubsampleRange, WrittenValue,
};

impl Parser<'_> {
    pub(super) fn shock_block_kind(&mut self, opener_i: usize, body_i: usize) -> ShockBlockKind {
        let command = self.tokens[opener_i].text(self.src).to_string();
        if command.eq_ignore_ascii_case("mshocks") {
            return ShockBlockKind::Multiplicative;
        }
        if command.eq_ignore_ascii_case("heteroskedastic_shocks") {
            return ShockBlockKind::Heteroskedastic;
        }
        let options = self.shock_options_at(opener_i, body_i);
        if options.heterogeneity.is_some() {
            ShockBlockKind::Heterogeneous
        } else if options.learnt_in.is_some() {
            ShockBlockKind::LearntIn
        } else if options
            .written
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case("surprise"))
        {
            ShockBlockKind::Surprise
        } else {
            ShockBlockKind::Regular
        }
    }

    fn shock_options_at(&mut self, opener_i: usize, body_i: usize) -> ShockOptions {
        let mut options = ShockOptions::default();
        let mut i = opener_i + 1;
        if self.tokens.get(i).map(|t| t.kind) != Some(TokenKind::LParen) {
            return options;
        }
        i += 1;
        while i < body_i && self.tokens[i].kind != TokenKind::RParen {
            if self.tokens[i].kind != TokenKind::Ident {
                i += 1;
                continue;
            }
            let tok = self.tokens[i].clone();
            let word = tok.text(self.src).to_ascii_lowercase();
            options.written.push((word.clone(), tok.span));
            i += 1;
            match word.as_str() {
                "overwrite" => options.overwrite = true,
                "relative_to_initval" => options.relative_to_initval = true,
                "learnt_in" if self.tokens.get(i).map(|t| t.kind) == Some(TokenKind::Eq) => {
                    i += 1;
                    if let Some((point, next)) = self.period_point_at(i, false) {
                        options.learnt_in = Some(point);
                        options.learnt_in_span = Some(Span {
                            start: self.tokens[i].span.start,
                            end: self.tokens[next - 1].span.end,
                        });
                        i = next;
                    }
                }
                "heterogeneity" if self.tokens.get(i).map(|t| t.kind) == Some(TokenKind::Eq) => {
                    i += 1;
                    if let Some(tok) = self.tokens.get(i).filter(|t| t.kind == TokenKind::Ident) {
                        options.heterogeneity =
                            Some((self.intern.intern(tok.text(self.src)), tok.span));
                        i += 1;
                    }
                }
                _ => {}
            }
        }
        options
    }

    pub(super) fn shock_var_is_scheduled(&self, at: usize, end_i: usize) -> bool {
        let mut i = at + 1;
        while i < end_i && self.tokens[i].kind != TokenKind::Semi {
            i += 1;
        }
        i + 1 < end_i
            && self.tokens[i + 1].kind == TokenKind::Ident
            && self.tokens[i + 1]
                .text(self.src)
                .eq_ignore_ascii_case("periods")
    }

    pub(super) fn skip_scheduled_shock(&mut self, end_i: usize) {
        let mut semis = 0;
        while self.i < end_i && semis < 3 {
            if self.bump().kind == TokenKind::Semi {
                semis += 1;
            }
        }
    }

    pub(super) fn collect_shock_block(
        &mut self,
        opener_i: usize,
        body_i: usize,
        body_end_i: usize,
        end: u32,
        kind: ShockBlockKind,
    ) {
        let options = self.shock_options_at(opener_i, body_i);
        let start = self.tokens[opener_i].span.start;
        let mut scheduled = Vec::new();
        let saved = self.i;
        self.i = body_i;
        while self.i < body_end_i {
            if self.at_ident_ci("var") && self.shock_var_is_scheduled(self.i, body_end_i) {
                if let Some(row) = self.read_scheduled_row(body_end_i, kind) {
                    scheduled.push(row);
                }
            } else {
                self.bump();
            }
        }
        self.i = saved;
        let body_start = self.tokens.get(body_i).map(|t| t.span.start).unwrap_or(end);
        let body_end = self
            .tokens
            .get(body_end_i)
            .map(|t| t.span.start)
            .unwrap_or(end);
        let stochastic = if matches!(kind, ShockBlockKind::Regular) {
            self.model
                .shock_stmts
                .iter()
                .filter(|stmt| stmt.span.start >= body_start && stmt.span.start < body_end)
                .cloned()
                .collect()
        } else {
            Vec::new()
        };
        self.model.shock_blocks.push(ShockBlock {
            kind,
            options,
            stochastic,
            scheduled,
            span: Span { start, end },
        });
    }

    fn read_scheduled_row(
        &mut self,
        end_i: usize,
        block_kind: ShockBlockKind,
    ) -> Option<ScheduledShock> {
        let start = self.bump().span.start; // var
        let name_tok = self.tokens.get(self.i)?.clone();
        if name_tok.kind != TokenKind::Ident {
            self.skip_scheduled_shock(end_i);
            return None;
        }
        self.bump();
        let name = self.intern.intern(name_tok.text(self.src));
        if !self.at(TokenKind::Semi) {
            self.skip_scheduled_shock(end_i);
            return None;
        }
        self.bump();
        if !self.at_ident_ci("periods") {
            return None;
        }
        self.bump();
        let periods = self.read_periods_until_semi(false);
        let operation = if self.at_ident_ci("values") {
            ShockOperation::Values
        } else if self.at_ident_ci("add") {
            ShockOperation::Add
        } else if self.at_ident_ci("multiply") {
            ShockOperation::Multiply
        } else if self.at_ident_ci("scales") && block_kind == ShockBlockKind::Heteroskedastic {
            ShockOperation::Scales
        } else {
            return None;
        };
        self.bump();
        let values_start = self.i;
        while self.i < end_i && !self.at(TokenKind::Semi) {
            self.bump();
        }
        let values_end = self.i;
        if values_start < values_end && self.tokens[values_start].kind == TokenKind::Ident {
            self.model.shape_refuses.push(ShapeRefuse::official(
                self.tokens[values_start].span,
                "values",
                "syntax error, unexpected IDENTIFIER",
            ));
        }
        let values = self.written_values(values_start, values_end, false, false);
        let end = if self.at(TokenKind::Semi) {
            self.bump().span.end
        } else {
            self.current_start()
        };
        Some(ScheduledShock {
            name,
            name_span: name_tok.span,
            periods,
            values,
            operation,
            span: Span { start, end },
        })
    }

    /// A DATE is a number glued to its unit suffix. The byte lexer stores the
    /// two pieces separately, unlike Dynare's Flex lexer.
    pub(super) fn date_at(&self, at: usize) -> Option<(DateExpr, usize)> {
        let mut i = at;
        let start = self.tokens.get(i)?.span.start;
        if self.tokens[i].kind == TokenKind::Ident
            && crate::model::dynare_date(self.tokens[i].text(self.src))
        {
            i += 1;
            while self.tokens.get(i).map(|t| t.kind) == Some(TokenKind::Plus)
                && self.tokens.get(i + 1).is_some_and(|t| {
                    t.kind == TokenKind::Number && is_integer_lexeme(t.text(self.src))
                })
            {
                i += 2;
            }
            let end = self.tokens[i - 1].span.end;
            let span = Span { start, end };
            return Some((
                DateExpr {
                    text: self.src[start as usize..end as usize].to_string(),
                    span,
                },
                i,
            ));
        }
        if self.tokens[i].kind == TokenKind::Minus {
            if self
                .tokens
                .get(i + 1)
                .is_none_or(|next| next.span.start != self.tokens[i].span.end)
            {
                return None;
            }
            i += 1;
        }
        let number = self.tokens.get(i)?;
        let suffix = self.tokens.get(i + 1)?;
        if number.kind != TokenKind::Number
            || suffix.kind != TokenKind::Ident
            || number.span.end != suffix.span.start
        {
            return None;
        }
        let base_end = suffix.span.end;
        let base = &self.src[start as usize..base_end as usize];
        if !crate::model::dynare_date(base) {
            return None;
        }
        i += 2;
        while self.tokens.get(i).map(|t| t.kind) == Some(TokenKind::Plus)
            && self
                .tokens
                .get(i + 1)
                .is_some_and(|t| t.kind == TokenKind::Number && is_integer_lexeme(t.text(self.src)))
        {
            i += 2;
        }
        let end = self.tokens[i - 1].span.end;
        let span = Span { start, end };
        Some((
            DateExpr {
                text: self.src[start as usize..end as usize].to_string(),
                span,
            },
            i,
        ))
    }

    fn period_point_at(&self, at: usize, allow_end: bool) -> Option<(PeriodPoint, usize)> {
        if let Some((date, next)) = self.date_at(at) {
            return Some((PeriodPoint::Date(date), next));
        }
        let tok = self.tokens.get(at)?;
        if allow_end
            && tok.kind == TokenKind::Ident
            && tok.text(self.src).eq_ignore_ascii_case("end")
        {
            return Some((PeriodPoint::End, at + 1));
        }
        if tok.kind == TokenKind::Number && is_integer_lexeme(tok.text(self.src)) {
            return tok
                .text(self.src)
                .parse()
                .ok()
                .map(|n| (PeriodPoint::Integer(n), at + 1));
        }
        None
    }

    fn read_periods_until_semi(&mut self, allow_end: bool) -> Vec<PeriodRange> {
        let mut out = Vec::new();
        while !self.at(TokenKind::Semi) && !self.at(TokenKind::Eof) {
            if self.at(TokenKind::Comma) {
                self.bump();
                continue;
            }
            let at = self.i;
            let Some((first, next)) = self.period_point_at(at, allow_end) else {
                self.bump();
                continue;
            };
            let start = self.tokens[at].span.start;
            let mut last = None;
            self.i = next;
            if self.i < self.tokens.len() && self.gap_is(self.i - 1, self.i, ":") {
                if let Some((point, after)) = self.period_point_at(self.i, allow_end) {
                    last = Some(point);
                    self.i = after;
                } else if !allow_end && self.word_at(self.i, "end") {
                    self.model.shape_refuses.push(ShapeRefuse::official(
                        self.tokens[self.i].span,
                        "periods",
                        "syntax error, unexpected END, expecting INT_NUMBER",
                    ));
                }
            }
            let end = self.tokens[self.i - 1].span.end;
            out.push(PeriodRange {
                first,
                last,
                span: Span { start, end },
            });
            if allow_end
                && self.i < self.tokens.len()
                && !matches!(self.tokens[self.i].kind, TokenKind::Comma | TokenKind::Semi)
                && self.period_point_at(self.i, true).is_some()
            {
                let message = if self.date_at(self.i).is_some() {
                    "syntax error, unexpected DATE, expecting COMMA or ';'"
                } else {
                    "syntax error, unexpected INT_NUMBER, expecting COMMA or ';'"
                };
                self.model.shape_refuses.push(ShapeRefuse::official(
                    self.tokens[self.i].span,
                    "shock_paths",
                    message,
                ));
            }
        }
        self.eat(TokenKind::Semi);
        out
    }

    fn written_value(&mut self, from: usize, to: usize, path: bool) -> Option<WrittenValue> {
        if from >= to {
            return None;
        }
        let span = Span {
            start: self.tokens[from].span.start,
            end: self.tokens[to - 1].span.end,
        };
        let text = self.src[span.start as usize..span.end as usize]
            .trim()
            .to_string();
        let path_refs = if path {
            self.path_refs(from, to)
        } else {
            Vec::new()
        };
        let expr = if path {
            None
        } else {
            let saved = self.i;
            self.i = from;
            let expr = self.parse_expr();
            self.i = saved;
            expr
        };
        Some(WrittenValue {
            text,
            span,
            expr,
            path_refs,
        })
    }

    fn written_values(
        &mut self,
        from: usize,
        to: usize,
        comma_required: bool,
        path: bool,
    ) -> Vec<WrittenValue> {
        let mut out = Vec::new();
        let mut i = from;
        while i < to {
            if self.tokens[i].kind == TokenKind::Comma {
                i += 1;
                continue;
            }
            let start = i;
            if comma_required {
                let mut depth = 0_i32;
                while i < to {
                    match self.tokens[i].kind {
                        TokenKind::LParen => depth += 1,
                        TokenKind::RParen => depth -= 1,
                        TokenKind::Comma if depth == 0 => break,
                        _ => {}
                    }
                    i += 1;
                }
            } else if self.tokens[i].kind == TokenKind::LParen {
                i = skip_balanced_tokens(&self.tokens, i, TokenKind::LParen, TokenKind::RParen)
                    .min(to);
            } else {
                if matches!(self.tokens[i].kind, TokenKind::Plus | TokenKind::Minus) {
                    i += 1;
                }
                i = (i + 1).min(to);
            }
            if let Some(value) = self.written_value(start, i, path) {
                out.push(value);
            }
        }
        out
    }

    fn path_refs(&mut self, from: usize, to: usize) -> Vec<PathReference> {
        let mut out = Vec::new();
        let mut i = from;
        while i < to {
            let tok = &self.tokens[i];
            if tok.kind != TokenKind::Ident {
                i += 1;
                continue;
            }
            let mut namespace = None;
            let mut learnt_in = None;
            let start = tok.span.start;
            let mut name_i = i;
            if tok.text(self.src).eq_ignore_ascii_case("learnt_in")
                && self.tokens.get(i + 1).map(|t| t.kind) == Some(TokenKind::LParen)
            {
                let after =
                    skip_balanced_tokens(&self.tokens, i + 1, TokenKind::LParen, TokenKind::RParen);
                if self.tokens.get(after).map(|t| t.kind) == Some(TokenKind::Dot)
                    && self.tokens.get(after + 1).map(|t| t.kind) == Some(TokenKind::Ident)
                {
                    learnt_in = self.period_point_at(i + 2, false).map(|(p, _)| p);
                    namespace = Some("learnt_in".to_string());
                    name_i = after + 1;
                }
            } else if self.tokens.get(i + 1).map(|t| t.kind) == Some(TokenKind::Dot)
                && self.tokens.get(i + 2).map(|t| t.kind) == Some(TokenKind::Ident)
            {
                namespace = Some(tok.text(self.src).to_string());
                name_i = i + 2;
            }
            let name_tok = self.tokens[name_i].clone();
            let after_name = name_i + 1;
            let mut lag = None;
            let mut next = after_name;
            if self.tokens.get(after_name).map(|t| t.kind) == Some(TokenKind::LParen) {
                let after = skip_balanced_tokens(
                    &self.tokens,
                    after_name,
                    TokenKind::LParen,
                    TokenKind::RParen,
                )
                .min(to);
                if after > after_name + 1 {
                    let lag_start = self.tokens[after_name + 1].span.start as usize;
                    let lag_end = self.tokens[after - 2].span.end as usize;
                    lag = Some(self.src[lag_start..lag_end].trim().to_string());
                }
                next = after;
            }
            let call = namespace.is_none() && next > after_name;
            let name = self.intern.intern(name_tok.text(self.src));
            out.push(PathReference {
                namespace,
                name,
                span: Span {
                    start,
                    end: name_tok.span.end,
                },
                lag,
                learnt_in,
                call,
            });
            // A bare function call may contain references we still need to
            // inspect (`exp(p)`); a namespace call's contents are its lag.
            i = if call { after_name } else { next };
        }
        out
    }

    /// `end` can be a period in an exogenous path stanza. Dynare's lexer
    /// distinguishes that use from the block closer; our token lexer needs this
    /// small context check while searching for the closer.
    fn consume_until_path_end(&mut self) -> usize {
        let mut in_periods = false;
        loop {
            if self.at(TokenKind::Eof) || self.at_block_opener() {
                return self.i;
            }
            if self.at_ident_ci("periods") {
                in_periods = true;
            }
            if self.at_ident_ci("end") && self.peek_kind(1) == Some(TokenKind::Semi) && !in_periods
            {
                let at = self.i;
                self.bump();
                self.bump();
                return at;
            }
            if self.at(TokenKind::Semi) {
                in_periods = false;
            }
            self.bump();
        }
    }

    pub(super) fn parse_path_block(&mut self, companion: bool) {
        let opener_i = self.i;
        let keyword = if companion {
            "perfect_foresight_controlled_paths"
        } else {
            "shock_paths"
        };
        let start_tok = self.bump();
        if companion {
            self.model
                .perfect_foresight_controlled_paths_span
                .get_or_insert(start_tok.span);
        } else {
            self.model.shock_paths_span.get_or_insert(start_tok.span);
        }
        if self.at(TokenKind::LParen) {
            self.skip_balanced(TokenKind::LParen, TokenKind::RParen);
        }
        let opener_end = if self.at(TokenKind::Semi) {
            self.bump().span.end
        } else {
            self.current_start()
        };
        let body_i = self.i;
        if !companion {
            for i in opener_i + 1..body_i {
                if self.tokens[i].kind == TokenKind::Comma {
                    self.model.shape_refuses.push(ShapeRefuse::official(
                        self.tokens[i].span,
                        keyword,
                        "syntax error, unexpected COMMA, expecting OVERWRITE or LEARNT_IN or ')'",
                    ));
                    break;
                }
            }
        }
        let body_end_i = self.consume_until_path_end();
        self.record_missing_end_if_unclosed(
            keyword,
            Span {
                start: start_tok.span.start,
                end: opener_end,
            },
            body_i,
            body_end_i,
        );
        let end = self.block_end_after_consume();
        let options = self.shock_options_at(opener_i, body_i);
        let saved = self.i;
        self.i = body_i;
        let mut stanzas = Vec::new();
        while self.i < body_end_i {
            if self.at_ident_ci("var") && !companion {
                if let Some(row) = self.read_path_stanza(body_end_i, false, false) {
                    stanzas.push(row);
                }
            } else if self.at_ident_ci("exogenize") {
                if let Some(row) = self.read_path_stanza(body_end_i, true, companion) {
                    stanzas.push(row);
                }
            } else {
                self.bump();
            }
        }
        self.i = saved;
        if stanzas.is_empty() && body_i == body_end_i {
            let message = if companion {
                "syntax error, unexpected END, expecting EXOGENIZE"
            } else {
                "syntax error, unexpected END, expecting VAR or EXOGENIZE"
            };
            self.model.shape_refuses.push(ShapeRefuse::official(
                self.tokens[body_end_i].span,
                keyword,
                message,
            ));
        }
        let block = PathBlock {
            options,
            stanzas,
            span: Span {
                start: start_tok.span.start,
                end,
            },
        };
        if companion {
            self.model.controlled_paths.push(block);
        } else {
            self.model.shock_paths.push(block);
        }
    }

    fn read_path_stanza(
        &mut self,
        end_i: usize,
        controlled: bool,
        companion: bool,
    ) -> Option<PathStanza> {
        let start = self.bump().span.start;
        let target_tok = self.tokens.get(self.i)?.clone();
        if target_tok.kind != TokenKind::Ident {
            return None;
        }
        self.bump();
        let target_name = self.intern.intern(target_tok.text(self.src));
        if !self.at(TokenKind::Semi) {
            return None;
        }
        self.bump();
        if !self.at_ident_ci("periods") {
            return None;
        }
        self.bump();
        let periods = self.read_periods_until_semi(!controlled);
        if !self.at_ident_ci("values") {
            return None;
        }
        self.bump();
        let value_start = self.i;
        while self.i < end_i && !self.at(TokenKind::Semi) {
            self.bump();
        }
        let value_end = self.i;
        let values = self.written_values(value_start, value_end, !companion, !companion);
        if !companion {
            let mut depth = 0_i32;
            for i in value_start..value_end {
                let kind = self.tokens[i].kind;
                if depth == 0 && i > value_start {
                    let previous = self.tokens[i - 1].kind;
                    let ends_expression = matches!(
                        previous,
                        TokenKind::Ident | TokenKind::Number | TokenKind::RParen
                    );
                    let starts_expression = matches!(
                        kind,
                        TokenKind::Ident | TokenKind::Number | TokenKind::LParen
                    );
                    let function_call = previous == TokenKind::Ident && kind == TokenKind::LParen;
                    let message = if ends_expression && starts_expression && !function_call {
                        Some(if kind == TokenKind::Number {
                            "syntax error, unexpected INT_NUMBER, expecting COMMA or ';'"
                        } else if kind == TokenKind::LParen {
                            "syntax error, unexpected '(', expecting COMMA or ';'"
                        } else {
                            "syntax error, unexpected IDENTIFIER, expecting COMMA or ';'"
                        })
                    } else {
                        None
                    };
                    if let Some(message) = message {
                        self.model.shape_refuses.push(ShapeRefuse::official(
                            self.tokens[i].span,
                            "shock_paths",
                            message,
                        ));
                        break;
                    }
                }
                match kind {
                    TokenKind::LParen => depth += 1,
                    TokenKind::RParen => depth -= 1,
                    _ => {}
                }
            }
        }
        self.eat(TokenKind::Semi);
        let target = if controlled {
            if !self.at_ident_ci("endogenize") {
                return None;
            }
            self.bump();
            let endo_tok = self.tokens.get(self.i)?.clone();
            if endo_tok.kind != TokenKind::Ident {
                return None;
            }
            self.bump();
            let endogenize = self.intern.intern(endo_tok.text(self.src));
            self.eat(TokenKind::Semi);
            PathTarget::Controlled {
                exogenize: target_name,
                exogenize_span: target_tok.span,
                endogenize,
                endogenize_span: endo_tok.span,
            }
        } else {
            PathTarget::Exogenous {
                name: target_name,
                span: target_tok.span,
            }
        };
        let end = self.tokens[self.i.saturating_sub(1)].span.end;
        Some(PathStanza {
            target,
            periods,
            values,
            span: Span { start, end },
        })
    }

    pub(super) fn collect_endval_instruction(
        &mut self,
        opener_i: usize,
        body_i: usize,
        body_end_i: usize,
        end: u32,
    ) {
        let options = self.shock_options_at(opener_i, body_i);
        let saved = self.i;
        self.i = body_i;
        let mut entries = Vec::new();
        while self.i < body_end_i {
            let start = self.i;
            if !self.at(TokenKind::Ident) {
                self.bump();
                continue;
            }
            let name_tok = self.bump();
            let name = self.intern.intern(name_tok.text(self.src));
            let operation = if self.at(TokenKind::Eq) {
                self.bump();
                ShockOperation::Values
            } else if self.at(TokenKind::Plus) && self.peek_kind(1) == Some(TokenKind::Eq) {
                self.bump();
                self.bump();
                ShockOperation::Add
            } else if self.at(TokenKind::Star) && self.peek_kind(1) == Some(TokenKind::Eq) {
                self.bump();
                self.bump();
                ShockOperation::Multiply
            } else {
                while self.i < body_end_i && !self.at(TokenKind::Semi) {
                    self.bump();
                }
                self.eat(TokenKind::Semi);
                continue;
            };
            let value_start = self.i;
            while self.i < body_end_i && !self.at(TokenKind::Semi) {
                self.bump();
            }
            let value_end = self.i;
            let value = self.written_value(value_start, value_end, false);
            let row_end = if self.at(TokenKind::Semi) {
                self.bump().span.end
            } else {
                self.current_start()
            };
            if let Some(value) = value {
                entries.push(EndvalEntry {
                    name,
                    name_span: name_tok.span,
                    value,
                    operation,
                    span: Span {
                        start: self.tokens[start].span.start,
                        end: row_end,
                    },
                });
            }
        }
        self.i = saved;
        self.model.endval_instructions.push(EndvalInstruction {
            learnt_in: options.learnt_in,
            learnt_in_span: options.learnt_in_span,
            entries,
            span: Span {
                start: self.tokens[opener_i].span.start,
                end,
            },
        });
    }

    pub(super) fn parse_database_statement(&mut self) {
        let start = self.bump().span.start;
        let mut names = Vec::new();
        while !self.at(TokenKind::Eof) && !self.at(TokenKind::Semi) {
            if self.at(TokenKind::Ident) {
                let tok = self.bump();
                let name = self.intern.intern(tok.text(self.src));
                names.push((name, tok.span));
            } else {
                self.bump();
            }
        }
        let end = if self.at(TokenKind::Semi) {
            self.bump().span.end
        } else {
            self.current_start()
        };
        self.model.databases.push(DatabaseDeclaration {
            names,
            span: Span { start, end },
        });
    }

    pub(super) fn parse_set_time_statement(&mut self) {
        let start = self.bump().span.start;
        if self.at(TokenKind::LParen) {
            let first = self.i + 1;
            let message = match self.tokens.get(first).map(|t| t.kind) {
                Some(TokenKind::Plus) => Some("syntax error, unexpected PLUS, expecting DATE"),
                Some(TokenKind::Minus) if self.date_at(first).is_none() => {
                    Some("syntax error, unexpected MINUS, expecting DATE")
                }
                _ => None,
            };
            if let Some(message) = message {
                self.model.shape_refuses.push(ShapeRefuse::official(
                    self.tokens[first].span,
                    "set_time",
                    message,
                ));
            }
        }
        let value = if self.at(TokenKind::LParen) {
            self.date_at(self.i + 1).map(|(date, _)| date)
        } else {
            None
        };
        while !self.at(TokenKind::Eof) && !self.at(TokenKind::Semi) {
            self.bump();
        }
        let end = if self.at(TokenKind::Semi) {
            self.bump().span.end
        } else {
            self.current_start()
        };
        if let Some(value) = value {
            self.model.set_time.push(SetTimeStatement {
                value,
                span: Span { start, end },
            });
        }
    }

    /// Capture DATE-valued command options without claiming numeric solver work.
    pub(super) fn collect_date_options(&mut self, command: &str, open_i: usize, close_i: usize) {
        let mut i = open_i + 1;
        while i + 2 < close_i {
            let tok = &self.tokens[i];
            if tok.kind != TokenKind::Ident || self.tokens[i + 1].kind != TokenKind::Eq {
                i += 1;
                continue;
            }
            let name = tok.text(self.src).to_ascii_lowercase();
            let date_option = matches!(
                name.as_str(),
                "first_obs"
                    | "last_obs"
                    | "first_simulation_period"
                    | "last_simulation_period"
                    | "plot_init_date"
                    | "plot_end_date"
            );
            if date_option {
                if let Some((value, next)) = self.date_at(i + 2) {
                    self.model.date_options.push(DateOption {
                        command: command.to_ascii_lowercase(),
                        name,
                        span: Span {
                            start: tok.span.start,
                            end: value.span.end,
                        },
                        value,
                    });
                    i = next;
                    continue;
                }
            }
            i += 1;
        }
    }

    pub(super) fn collect_stoch_simul_request(
        &mut self,
        span: Span,
        option_range: Option<(usize, usize)>,
    ) {
        let mut request = StochSimulRequest {
            span,
            irf: None,
            irf_shocks: None,
        };
        if let Some((open, close)) = option_range {
            let mut i = open + 1;
            while i < close.saturating_sub(1) {
                if self.tokens[i].kind != TokenKind::Ident {
                    i += 1;
                    continue;
                }
                let option = self.tokens[i].text(self.src).to_ascii_lowercase();
                if option == "irf" && self.tokens.get(i + 1).map(|t| t.kind) == Some(TokenKind::Eq)
                {
                    if let Some(tok) = self
                        .tokens
                        .get(i + 2)
                        .filter(|t| t.kind == TokenKind::Number)
                    {
                        if let Ok(number) = tok.text(self.src).parse::<i32>() {
                            request.irf = Some((number, tok.span));
                        }
                    }
                } else if option == "irf_shocks"
                    && self.tokens.get(i + 1).map(|t| t.kind) == Some(TokenKind::Eq)
                    && self.tokens.get(i + 2).map(|t| t.kind) == Some(TokenKind::LParen)
                {
                    let after = skip_balanced_tokens(
                        &self.tokens,
                        i + 2,
                        TokenKind::LParen,
                        TokenKind::RParen,
                    )
                    .min(close);
                    let mut names = Vec::new();
                    for j in i + 3..after.saturating_sub(1) {
                        if self.tokens[j].kind == TokenKind::Ident {
                            let tok = &self.tokens[j];
                            names.push((self.intern.intern(tok.text(self.src)), tok.span));
                        }
                    }
                    request.irf_shocks = Some(names);
                    i = after;
                    continue;
                }
                i += 1;
            }
        }
        self.model.stoch_simul_requests.push(request);
    }

    fn word_at(&self, i: usize, word: &str) -> bool {
        self.tokens.get(i).is_some_and(|t| {
            t.kind == TokenKind::Ident && t.text(self.src).eq_ignore_ascii_case(word)
        })
    }

    fn subsample_head_at(&mut self, i: usize) -> Option<(SubsampleHead, usize)> {
        if self.word_at(i, "std")
            && self.tokens.get(i + 1).map(|t| t.kind) == Some(TokenKind::LParen)
        {
            let name = self.tokens.get(i + 2)?.clone();
            if name.kind != TokenKind::Ident
                || self.tokens.get(i + 3).map(|t| t.kind) != Some(TokenKind::RParen)
                || self.tokens.get(i + 4).map(|t| t.kind) != Some(TokenKind::Dot)
                || !self.word_at(i + 5, "subsamples")
            {
                return None;
            }
            return Some((
                SubsampleHead::Std(self.intern.intern(name.text(self.src)), name.span),
                i + 6,
            ));
        }
        if self.word_at(i, "corr")
            && self.tokens.get(i + 1).map(|t| t.kind) == Some(TokenKind::LParen)
        {
            let a = self.tokens.get(i + 2)?.clone();
            let b = self.tokens.get(i + 4)?.clone();
            if a.kind != TokenKind::Ident
                || b.kind != TokenKind::Ident
                || self.tokens.get(i + 3).map(|t| t.kind) != Some(TokenKind::Comma)
                || self.tokens.get(i + 5).map(|t| t.kind) != Some(TokenKind::RParen)
                || self.tokens.get(i + 6).map(|t| t.kind) != Some(TokenKind::Dot)
                || !self.word_at(i + 7, "subsamples")
            {
                return None;
            }
            return Some((
                SubsampleHead::Corr(
                    self.intern.intern(a.text(self.src)),
                    a.span,
                    self.intern.intern(b.text(self.src)),
                    b.span,
                ),
                i + 8,
            ));
        }
        let name = self.tokens.get(i)?.clone();
        if name.kind != TokenKind::Ident
            || self.tokens.get(i + 1).map(|t| t.kind) != Some(TokenKind::Dot)
            || !self.word_at(i + 2, "subsamples")
        {
            return None;
        }
        Some((
            SubsampleHead::Symbol(self.intern.intern(name.text(self.src)), name.span),
            i + 3,
        ))
    }

    pub(super) fn collect_subsample_statement(&mut self, from: usize, to: usize) {
        let Some((head, mut i)) = self.subsample_head_at(from) else {
            return;
        };
        let span = Span {
            start: self.tokens[from].span.start,
            end: self.tokens[to.saturating_sub(1)].span.end,
        };
        if self.tokens.get(i).map(|t| t.kind) == Some(TokenKind::Eq) {
            i += 1;
            if let Some((source, _)) = self.subsample_head_at(i) {
                self.model.subsamples.push(SubsampleInstruction::Copy {
                    target: head,
                    source,
                    span,
                });
            }
            return;
        }
        if self.tokens.get(i).map(|t| t.kind) != Some(TokenKind::LParen) {
            return;
        }
        if self.tokens.get(i + 1).map(|t| t.kind) == Some(TokenKind::RParen) {
            self.model.shape_refuses.push(ShapeRefuse::official(
                self.tokens[i + 1].span,
                "subsamples",
                "syntax error, unexpected ')'",
            ));
        }
        i += 1;
        let mut ranges = Vec::new();
        while i < to && self.tokens[i].kind != TokenKind::RParen {
            if self.tokens[i].kind == TokenKind::Comma {
                i += 1;
                continue;
            }
            let name_tok = self.tokens[i].clone();
            if name_tok.kind != TokenKind::Ident
                || self.tokens.get(i + 1).map(|t| t.kind) != Some(TokenKind::Eq)
            {
                i += 1;
                continue;
            }
            let Some((first, after_first)) = self.date_at(i + 2) else {
                i += 1;
                continue;
            };
            if !self.gap_is(after_first - 1, after_first, ":") {
                if self.tokens.get(after_first).map(|t| t.kind) == Some(TokenKind::Comma) {
                    self.model.shape_refuses.push(ShapeRefuse::official(
                        self.tokens[after_first].span,
                        "subsamples",
                        "syntax error, unexpected COMMA, expecting PLUS or ':'",
                    ));
                }
                i += 1;
                continue;
            }
            let Some((last, after_last)) = self.date_at(after_first) else {
                i += 1;
                continue;
            };
            let name = self.intern.intern(name_tok.text(self.src));
            ranges.push(SubsampleRange {
                name,
                name_span: name_tok.span,
                span: Span {
                    start: name_tok.span.start,
                    end: last.span.end,
                },
                first,
                last,
            });
            i = after_last;
        }
        self.model
            .subsamples
            .push(SubsampleInstruction::Declare { head, ranges, span });
    }

    pub(super) fn record_shock_shape_refuses(
        &mut self,
        opener_i: usize,
        body_i: usize,
        body_end_i: usize,
        kind: ShockBlockKind,
    ) {
        let command = self.tokens[opener_i].text(self.src).to_string();
        let opts = self.shock_options_at(opener_i, body_i);
        if body_i == body_end_i {
            let message = match kind {
                ShockBlockKind::Regular if opts.overwrite => None,
                ShockBlockKind::Heteroskedastic if opts.overwrite => None,
                ShockBlockKind::Regular | ShockBlockKind::Heterogeneous => {
                    Some("syntax error, unexpected END, expecting CORR or SKEW or VAR")
                }
                ShockBlockKind::Multiplicative
                | ShockBlockKind::Heteroskedastic
                | ShockBlockKind::Surprise
                | ShockBlockKind::LearntIn => Some("syntax error, unexpected END, expecting VAR"),
            };
            if let Some(message) = message {
                self.model.shape_refuses.push(ShapeRefuse::official(
                    self.tokens[body_end_i].span,
                    &command,
                    message,
                ));
            }
        }
        if matches!(
            kind,
            ShockBlockKind::Surprise | ShockBlockKind::LearntIn | ShockBlockKind::Multiplicative
        ) {
            for i in body_i..body_end_i.saturating_sub(2) {
                if self.word_at(i, "var")
                    && self.tokens[i + 1].kind == TokenKind::Ident
                    && self.tokens[i + 2].kind == TokenKind::Eq
                {
                    self.model.shape_refuses.push(ShapeRefuse::official(
                        self.tokens[i + 2].span,
                        &command,
                        "syntax error, unexpected EQUAL, expecting ';'",
                    ));
                }
                if self.word_at(i, "var")
                    && self.tokens[i + 1].kind == TokenKind::Ident
                    && self.tokens[i + 2].kind == TokenKind::Semi
                    && self.word_at(i + 3, "stderr")
                {
                    self.model.shape_refuses.push(ShapeRefuse::official(
                        self.tokens[i + 3].span,
                        &command,
                        "syntax error, unexpected STDERR, expecting PERIODS",
                    ));
                }
            }
        }
        if command.eq_ignore_ascii_case("mshocks") {
            for i in opener_i + 1..body_i {
                if self.tokens[i].kind == TokenKind::Comma {
                    self.model.shape_refuses.push(ShapeRefuse::official(
                        self.tokens[i].span, &command,
                        "syntax error, unexpected COMMA, expecting OVERWRITE or LEARNT_IN or RELATIVE_TO_INITVAL or ')'",
                    ));
                    break;
                }
            }
        }
    }
}
