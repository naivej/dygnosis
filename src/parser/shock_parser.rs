//! Written Dynare 7.2 shock, path, and date forms. Semantic refusals are
//! checked by the diagnostic passes; this module keeps the file's structure.

use super::*;
use crate::model::{
    DatabaseDeclaration, DateExpr, DateOption, EndvalEntry, EndvalInstruction, IrfShocksOption,
    PathBlock, PathReference, PathStanza, PathTarget, PeriodPoint, PeriodRange, ScheduledShock,
    SetTimeStatement, ShockBlock, ShockBlockKind, ShockOperation, ShockOptions, ShockStmt,
    StochSimulRequest, SubsampleHead, SubsampleInstruction, SubsampleRange, WrittenValue,
};

pub(super) struct PathExpressionContext {
    learnt_in: Option<PeriodPoint>,
    controlled: bool,
    databases: HashSet<String>,
    pub(super) failed: bool,
}

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

    pub(super) fn scheduled_shock_has_value_row(&self, at: usize, end_i: usize) -> bool {
        if !self.word_at(at, "var") || !self.shock_var_is_scheduled(at, end_i) {
            return false;
        }
        let mut semis = 0;
        let mut i = at;
        while i < end_i && semis < 2 {
            if self.tokens[i].kind == TokenKind::Semi {
                semis += 1;
            }
            i += 1;
        }
        semis == 2
            && ["values", "add", "multiply", "scales"]
                .iter()
                .any(|word| self.word_at(i, word))
    }

    /// A regular `var symbol;` must continue with `periods` or `stderr`.
    /// Include the closer/EOF token at `end_i`: neither completes a row.
    pub(super) fn incomplete_regular_shock_var(&self, at: usize, end_i: usize) -> bool {
        at + 2 < end_i
            && self.word_at(at, "var")
            && self.tokens[at + 1].kind == TokenKind::Ident
            && self.tokens[at + 2].kind == TokenKind::Semi
            && !self.word_at(at + 3, "periods")
            && !self.word_at(at + 3, "stderr")
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
        let stochastic = if matches!(kind, ShockBlockKind::Regular) {
            // Macro copies share written spans. The flat view's latest start
            // records this block's parser execution, independently of spans.
            let first = self
                .model
                .shock_stmt_block_starts
                .last()
                .copied()
                .unwrap_or(self.model.shock_stmts.len());
            self.model.shock_stmts[first..].to_vec()
        } else if kind == ShockBlockKind::Heterogeneous {
            self.read_stochastic_rows(body_i, body_end_i)
        } else {
            Vec::new()
        };
        if !stochastic.is_empty() || !scheduled.is_empty() || options.overwrite {
            // Every accepted opener option is retained in this block's typed
            // shock instructions. Refused parents never acquire these claims.
            self.retain_fact_receipt(
                "shock_instruction",
                opener_i,
                vec![opener_i..body_i, body_end_i..saved],
            );
        }
        self.model.shock_blocks.push(ShockBlock {
            kind,
            options,
            stochastic,
            scheduled,
            span: Span { start, end },
        });
    }

    /// The stochastic rows of a `shocks(heterogeneity=d)` body: kinds, names,
    /// folded values, and spans as for Regular blocks, kept on the block record
    /// so nothing merges with the flat `shock_stmts` view.
    fn read_stochastic_rows(&mut self, start_i: usize, end_i: usize) -> Vec<ShockStmt> {
        let saved = self.i;
        self.i = start_i;
        let mut rows = Vec::new();
        while self.i < end_i {
            if self.at(TokenKind::Semi) {
                self.bump();
                continue;
            }
            if self.incomplete_regular_shock_var(self.i, end_i) {
                self.i += 3;
                continue;
            }
            let row = if self.at_ident_ci("var") && !self.shock_var_is_scheduled(self.i, end_i) {
                self.parse_shock_var_stmt(end_i)
            } else if self.at_ident_ci("corr") {
                self.parse_shock_corr_stmt(end_i)
            } else if self.at_ident_ci("skew") {
                self.parse_shock_skew_stmt(end_i)
            } else {
                None
            };
            if let Some(row) = row {
                rows.push(row);
                continue;
            }
            while self.i < end_i && !self.at(TokenKind::Semi) {
                self.bump();
            }
            self.eat(TokenKind::Semi);
        }
        self.i = saved;
        rows
    }

    fn read_scheduled_row(
        &mut self,
        end_i: usize,
        block_kind: ShockBlockKind,
    ) -> Option<ScheduledShock> {
        let row_i = self.i;
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
            self.model.shape_refuses.push(
                ShapeRefuse::official(
                    self.tokens[values_start].span,
                    "values",
                    "syntax error, unexpected IDENTIFIER",
                )
                .with_parse_order(self.token_origins[values_start].start),
            );
        }
        let values = self.written_values(values_start, values_end);
        let end = if self.at(TokenKind::Semi) {
            self.bump().span.end
        } else {
            self.current_start()
        };
        self.retain_fact_receipt(
            "shock_instruction",
            row_i,
            std::iter::once(row_i..self.i).collect(),
        );
        Some(ScheduledShock {
            symbol_type_context: self.model.symbol_context(),
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
                    constructor_text: self.tokens[at..i]
                        .iter()
                        .map(|token| token.text(self.src))
                        .collect(),
                },
                i,
            ));
        }
        if self.tokens[i].kind == TokenKind::Minus {
            if self.tokens.get(i + 1).is_none_or(|next| {
                !self.tokens[i]
                    .expanded_adjacent_next
                    .unwrap_or(next.span.start == self.tokens[i].span.end)
            }) {
                return None;
            }
            i += 1;
        }
        let number = self.tokens.get(i)?;
        let suffix = self.tokens.get(i + 1)?;
        if number.kind != TokenKind::Number
            || suffix.kind != TokenKind::Ident
            || !number
                .expanded_adjacent_next
                .unwrap_or(number.span.end == suffix.span.start)
        {
            return None;
        }
        let base: String = self.tokens[at..i + 2]
            .iter()
            .map(|token| token.text(self.src))
            .collect();
        if !crate::model::dynare_date(&base) {
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
                constructor_text: self.tokens[at..i]
                    .iter()
                    .map(|token| token.text(self.src))
                    .collect(),
            },
            i,
        ))
    }

    /// `date_at` intentionally returns the longest legal DATE prefix. A token
    /// immediately after that prefix can still make the full value illegal.
    pub(super) fn date_suffix_refusal_at(
        &self,
        next: usize,
        subject: &str,
        minus_message: &'static str,
    ) -> Option<ShapeRefuse> {
        let token = self.tokens.get(next)?;
        if token.kind == TokenKind::Minus {
            return Some(
                ShapeRefuse::official(token.span, subject, minus_message)
                    .with_parse_order(self.token_origins[next].start),
            );
        }
        if token.kind == TokenKind::Plus {
            let value = self.tokens.get(next + 1)?;
            if value.kind == TokenKind::Number && !is_integer_lexeme(value.text(self.src)) {
                return Some(
                    ShapeRefuse::official(
                        value.span,
                        subject,
                        "syntax error, unexpected FLOAT_NUMBER, expecting INT_NUMBER",
                    )
                    .with_parse_order(self.token_origins[next + 1].start),
                );
            }
        }
        None
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
        if self.path_context.is_some() {
            return self.read_path_periods(allow_end);
        }
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
                    self.model.shape_refuses.push(
                        ShapeRefuse::official(
                            self.tokens[self.i].span,
                            "periods",
                            "syntax error, unexpected END, expecting INT_NUMBER",
                        )
                        .with_parse_order(self.token_origins[self.i].start),
                    );
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
                self.model.shape_refuses.push(
                    ShapeRefuse::official(self.tokens[self.i].span, "shock_paths", message)
                        .with_parse_order(self.token_origins[self.i].start),
                );
            }
        }
        self.eat(TokenKind::Semi);
        out
    }

    /// The exogenous path list requires commas. Controlled path lists use
    /// period_list, which also permits adjacent ranges separated by whitespace.
    fn read_path_periods(&mut self, allow_end: bool) -> Vec<PeriodRange> {
        let mut out = Vec::new();
        let first_expected = if allow_end {
            "END or DATE or INT_NUMBER"
        } else {
            "DATE or INT_NUMBER"
        };
        loop {
            let at = self.i;
            if let Some(&span) = self.path_period_colons(at.saturating_sub(1), at).first() {
                self.refuse_path_period_colon(span, first_expected);
                break;
            }
            let Some((first, next)) = self.period_point_at(at, allow_end) else {
                self.refuse_path_period_syntax(first_expected);
                break;
            };
            self.i = next;
            let mut last = None;
            let colons = self.path_period_colons(self.i - 1, self.i);
            if let Some(&span) = colons.first() {
                let expected = match first {
                    PeriodPoint::Integer(_) if allow_end => "END or INT_NUMBER",
                    PeriodPoint::Integer(_) => "INT_NUMBER",
                    PeriodPoint::Date(_) if allow_end => "END or DATE",
                    PeriodPoint::Date(_) => "DATE",
                    PeriodPoint::End => {
                        self.refuse_path_period_colon(span, "COMMA or ';'");
                        break;
                    }
                };
                if let Some(&span) = colons.get(1) {
                    self.refuse_path_period_colon(span, expected);
                    break;
                }
                let point = self.period_point_at(self.i, allow_end);
                let same_kind = point.as_ref().is_some_and(|(point, _)| {
                    matches!(
                        (&first, point),
                        (PeriodPoint::Integer(_), PeriodPoint::Integer(_))
                            | (PeriodPoint::Date(_), PeriodPoint::Date(_))
                            | (_, PeriodPoint::End)
                    )
                });
                if !same_kind {
                    self.refuse_path_period_syntax(expected);
                    break;
                }
                let (point, after) = point.unwrap();
                last = Some(point);
                self.i = after;
                if let (PeriodPoint::Integer(first), Some(PeriodPoint::Integer(last))) =
                    (&first, &last)
                    && first > last
                {
                    let span = Span {
                        start: self.tokens[at].span.start,
                        end: self.tokens[self.i - 1].span.end,
                    };
                    self.path_error(span, "E395", "Can't have first period index greater than second index in range specification");
                    break;
                }
                if let Some(&span) = self.path_period_colons(self.i - 1, self.i).first() {
                    self.refuse_path_period_colon(
                        span,
                        if allow_end {
                            "COMMA or ';'"
                        } else {
                            "COMMA or DATE or INT_NUMBER or ';'"
                        },
                    );
                    break;
                }
            }
            out.push(PeriodRange {
                first,
                last,
                span: Span {
                    start: self.tokens[at].span.start,
                    end: self.tokens[self.i - 1].span.end,
                },
            });
            if self.at(TokenKind::Semi) {
                break;
            }
            if self.at(TokenKind::Comma) {
                self.bump();
                continue;
            }
            if !allow_end && self.period_point_at(self.i, false).is_some() {
                continue;
            }
            self.refuse_path_period_syntax(if allow_end {
                "COMMA or ';'"
            } else {
                "COMMA or DATE or INT_NUMBER or ';'"
            });
            break;
        }
        if self.path_context.as_ref().unwrap().failed {
            while !self.at(TokenKind::Semi) && !self.at(TokenKind::Eof) && !self.at_ident_ci("end")
            {
                self.bump();
            }
        }
        self.eat(TokenKind::Semi);
        out
    }

    fn refuse_path_period_syntax(&mut self, expected: &str) {
        if self.path_context.as_ref().unwrap().failed {
            return;
        }
        let (span, unexpected) = if self.date_at(self.i).is_some() {
            let end_i = if self.tokens[self.i].kind == TokenKind::Ident {
                self.i
            } else if self.tokens[self.i].kind == TokenKind::Minus {
                self.i + 2
            } else {
                self.i + 1
            };
            (
                Span {
                    start: self.tokens[self.i].span.start,
                    end: self.tokens[end_i].span.end,
                },
                "DATE".to_string(),
            )
        } else {
            self.ss_unexpected_token(self.i)
        };
        self.push_bison(
            span,
            format!("syntax error, unexpected {unexpected}, expecting {expected}"),
        );
        self.path_context.as_mut().unwrap().failed = true;
    }

    fn refuse_path_period_colon(&mut self, span: Span, expected: &str) {
        if self.path_context.as_ref().unwrap().failed {
            return;
        }
        self.push_bison(
            span,
            format!("syntax error, unexpected ':', expecting {expected}"),
        );
        self.path_context.as_mut().unwrap().failed = true;
    }

    /// Colons are omitted by the byte lexer. Read the emitted gap so spaces,
    /// comments, and macro-written range punctuation retain the pinned grammar.
    fn path_period_colons(&self, left: usize, right: usize) -> Vec<Span> {
        let (Some(left), Some(right)) = (self.tokens.get(left), self.tokens.get(right)) else {
            return Vec::new();
        };
        let gap = match (&left.emitted, &right.emitted) {
            (Some(a), Some(b)) if std::sync::Arc::ptr_eq(&a.source, &b.source) => a
                .source
                .text
                .get(a.span.end as usize..b.span.start as usize),
            _ => self
                .src
                .get(left.span.end as usize..right.span.start as usize),
        };
        let Some(gap) = gap else {
            return Vec::new();
        };
        let mut out = Vec::new();
        let mut start = crate::native_line::initial_content_start(gap, 0);
        while gap.as_bytes().get(start) == Some(&b':') && out.len() < 2 {
            let span = if let Some(emitted) = &left.emitted {
                let offset = emitted.span.end as usize + start;
                Span {
                    start: emitted.source.written_start(offset),
                    end: emitted.source.written_end(offset + 1),
                }
            } else {
                Span {
                    start: left.span.end + start as u32,
                    end: left.span.end + start as u32 + 1,
                }
            };
            out.push(span);
            start = crate::native_line::initial_content_start(gap, start + 1);
        }
        out
    }

    fn written_value(&mut self, from: usize, to: usize) -> Option<WrittenValue> {
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
        let path_refs = Vec::new();
        let saved = self.i;
        self.i = from;
        let expr = self.parse_expr();
        self.i = saved;
        Some(WrittenValue {
            text,
            span,
            expr,
            completed: expr.is_some(),
            path_refs,
        })
    }

    fn written_values(&mut self, from: usize, to: usize) -> Vec<WrittenValue> {
        let mut out = Vec::new();
        let mut i = from;
        while i < to {
            if self.tokens[i].kind == TokenKind::Comma {
                i += 1;
                continue;
            }
            let start = i;
            if self.tokens[i].kind == TokenKind::LParen {
                i = skip_balanced_tokens(&self.tokens, i, TokenKind::LParen, TokenKind::RParen)
                    .min(to);
            } else {
                if matches!(self.tokens[i].kind, TokenKind::Plus | TokenKind::Minus) {
                    i += 1;
                }
                i = (i + 1).min(to);
            }
            if let Some(value) = self.written_value(start, i) {
                out.push(value);
            }
        }
        out
    }

    /// References come from the written tree, before constructor simplification.
    fn path_refs_from_expr(&self, root: ExprId) -> Vec<PathReference> {
        let mut out = Vec::new();
        let mut pending = vec![root];
        while let Some(id) = pending.pop() {
            let expr = self.model.exprs.get(id);
            match &expr.kind {
                ExprKind::PathNamespace { reference, lag } => {
                    out.push((**reference).clone());
                    pending.extend(lag);
                }
                ExprKind::Ident {
                    name, ident_span, ..
                } => out.push(PathReference {
                    symbol_type_context: self.model.symbol_context(),
                    namespace: None,
                    name: *name,
                    span: *ident_span,
                    ident_span: *ident_span,
                    lag_span: None,
                    constructed_lag: None,
                    lag: None,
                    lag_call: false,
                    learnt_in: None,
                    call: false,
                }),
                ExprKind::Call { args, .. } => {
                    if let Some(reference) = self.path_call_refs.get(&id) {
                        out.push(reference.clone());
                    }
                    pending.extend(args.iter().rev());
                }
                ExprKind::Unary { arg, .. }
                | ExprKind::SteadyState { arg }
                | ExprKind::Expectation { arg, .. } => pending.push(*arg),
                ExprKind::Binary { lhs, rhs, .. } => {
                    pending.push(*rhs);
                    pending.push(*lhs);
                }
                _ => {}
            }
        }
        out
    }

    fn path_error(&mut self, span: Span, code: &'static str, message: impl Into<String>) {
        if self
            .path_context
            .as_ref()
            .is_some_and(|context| context.failed)
        {
            return;
        }
        self.model
            .path_parse_errors
            .push((span, code, message.into()));
        if let Some(context) = &mut self.path_context {
            context.failed = true;
        }
    }

    fn path_type_action(&mut self, name: Name, span: Span, required: &str) -> bool {
        let kind = self.model.final_symbol_kind(name);
        let text = self.intern.get(name).to_string();
        let refusal = match (kind, required) {
            (None, _) => Some(("E058", format!("Unknown symbol: {text}."))),
            (Some("var"), "var" | "init") | (Some("varexo"), "varexo" | "init") => None,
            (Some("varexo_det"), "varexo" | "init") => {
                Some(("E317", format!("{text} is an exogenous deterministic.")))
            }
            (_, "var") => Some(("E317", format!("{text} is not endogenous."))),
            (_, "init") => Some((
                "E059",
                format!("{text} is neither endogenous or exogenous."),
            )),
            _ => Some(("E387", format!("{text} is not exogenous."))),
        };
        if let Some((code, message)) = refusal {
            self.path_error(span, code, message);
            false
        } else {
            true
        }
    }

    pub(super) fn parse_path_ident_expr(&mut self) -> ExprId {
        let token_i = self.i;
        let token = self.bump();
        let mut written = token.text(self.src).to_string();
        let mut name_token = token.clone();
        let mut learnt_in = None;
        if let Some(keyword) = self.ss_block_word_token(token_i)
            && !Self::ss_symbol_token(keyword)
            && !is_dynare_expression_builtin(&written)
            && !matches!(keyword, "LEARNT_IN" | "NAN_CONSTANT" | "INF_CONSTANT")
        {
            self.push_bison(token.span, format!("syntax error, unexpected {keyword}"));
            return self.alloc_error(token.span);
        }
        if written.eq_ignore_ascii_case("learnt_in") && self.at(TokenKind::LParen) {
            self.bump();
            if let Some((point, next)) = self.period_point_at(self.i, false) {
                learnt_in = Some(point);
                self.i = next;
            } else {
                self.refuse_model_argument_syntax();
                return self.alloc_error(token.span);
            }
            if !self.at(TokenKind::RParen) {
                self.refuse_model_argument_syntax();
                return self.alloc_error(token.span);
            }
            self.bump();
            if !self.at(TokenKind::Dot) {
                self.refuse_model_argument_syntax();
                return self.alloc_error(token.span);
            }
            self.bump();
            if !self.at(TokenKind::Ident) {
                self.refuse_model_argument_syntax();
                return self.alloc_error(token.span);
            }
            name_token = self.bump();
            written = "learnt_in".into();
        } else {
            while self.at(TokenKind::Dot) {
                self.bump();
                if !self.at(TokenKind::Ident) {
                    self.refuse_model_argument_syntax();
                    return self.alloc_error(token.span);
                }
                name_token = self.bump();
                written.push('.');
                written.push_str(name_token.text(self.src));
            }
        }
        let namespace = if learnt_in.is_some() {
            Some("learnt_in".to_string())
        } else {
            written
                .rsplit_once('.')
                .map(|(prefix, _)| prefix.to_string())
        };
        let reference_span = Span {
            start: token.span.start,
            end: name_token.span.end,
        };
        let callee = self.intern.intern(&written);
        let is_namespace_call = namespace.as_deref().is_some_and(|prefix| {
            matches!(prefix, "self" | "prev" | "learnt_in")
                || self
                    .path_context
                    .as_ref()
                    .unwrap()
                    .databases
                    .contains(prefix)
        });
        if self.at(TokenKind::LParen) && !is_namespace_call {
            return self.parse_call(
                callee,
                Token::with_lexeme(TokenKind::Ident, reference_span, written),
            );
        }
        if namespace.is_none() {
            if written.eq_ignore_ascii_case("nan") || written.eq_ignore_ascii_case("inf") {
                return self.alloc(ExprKind::Number, token.span);
            }
            if is_dynare_expression_builtin(&written) {
                let (span, unexpected) = self.ss_unexpected_token(self.i);
                self.push_bison(
                    span,
                    format!("syntax error, unexpected {unexpected}, expecting '('"),
                );
                return self.alloc_error(token.span);
            }
            if let Some(keyword) = self.ss_block_word_token(token_i)
                && !Self::ss_symbol_token(keyword)
            {
                self.push_bison(token.span, format!("syntax error, unexpected {keyword}"));
                return self.alloc_error(token.span);
            }
            let name = self.intern.intern(&written);
            let kind = self.model.final_symbol_kind(name);
            if kind.is_some() && kind != Some("parameters") {
                self.path_error(token.span, "E407", "In the shock_paths block, parameters are the only symbols allowed without a namespace-qualifier");
            }
            let id = self.alloc(
                ExprKind::Ident {
                    name,
                    timing: 0,
                    ident_span: token.span,
                    timing_span: None,
                },
                token.span,
            );
            if kind != Some("parameters") {
                // The pinned unknown bare read crashes without an ERROR.
                // Preserve the written name, but make its construction unavailable.
                self.refused_constructors.insert(id);
                self.constructed.insert(id, self.constructors.opaque(id));
                self.path_context.as_mut().unwrap().failed = true;
            }
            return id;
        }
        let namespace = namespace.unwrap();
        let name = self.intern.intern(name_token.text(self.src));
        let mut lag = None;
        let mut lag_span = None;
        let mut lag_text = None;
        let lag_call = self.at(TokenKind::LParen);
        let mut end = reference_span.end;
        if lag_call {
            let open = self.bump();
            end = open.span.end;
            let mut arguments = Vec::new();
            loop {
                if let Some(arg) = self.parse_expr() {
                    arguments.push(arg);
                } else {
                    self.refuse_model_argument_syntax();
                    break;
                }
                if self.path_context.as_ref().unwrap().failed {
                    break;
                }
                if namespace == "learnt_in" && self.at(TokenKind::Comma) {
                    self.refuse_model_argument_syntax();
                    break;
                }
                if !self.at(TokenKind::Comma) {
                    break;
                }
                self.bump();
            }
            if self.path_context.as_ref().unwrap().failed {
                while !self.at(TokenKind::Eof)
                    && !self.at(TokenKind::Semi)
                    && !self.at(TokenKind::RParen)
                {
                    if self.at(TokenKind::LParen) {
                        self.skip_balanced(TokenKind::LParen, TokenKind::RParen);
                    } else {
                        self.bump();
                    }
                }
            }
            if !self.at(TokenKind::RParen) {
                self.refuse_model_argument_syntax();
            } else {
                end = self.bump().span.end;
            }
            lag_span = Some(Span {
                start: open.span.start,
                end,
            });
            let inside_end = end.saturating_sub(1).max(open.span.end);
            lag_text = self
                .src
                .get(open.span.end as usize..inside_end as usize)
                .map(|text| text.trim().to_string());
            lag = arguments.first().copied();
            if arguments
                .iter()
                .any(|id| self.refused_constructors.contains(id))
            {
                self.path_context.as_mut().unwrap().failed = true;
            } else if arguments.len() > 1 {
                self.path_error(reference_span, "E410", format!("The parenthesis after {written} should only include a lag, since it references a variable inside a namespace"));
            }
        }
        let mut reference = PathReference {
            symbol_type_context: self.model.symbol_context(),
            namespace: Some(namespace.clone()),
            name,
            span: reference_span,
            ident_span: name_token.span,
            lag_span,
            constructed_lag: lag
                .map(|id| self.constructors.match_integer(self.constructed[&id]))
                .unwrap_or(Some(0)),
            lag: lag_text,
            lag_call,
            learnt_in,
            call: false,
        };
        if self
            .model
            .parse_issues
            .last()
            .is_some_and(|issue| reference_span.start <= issue.span.start && issue.span.end <= end)
        {
            self.path_context.as_mut().unwrap().failed = true;
        }
        if !self.path_context.as_ref().unwrap().failed {
            self.path_namespace_action(&reference, lag);
        }
        let failed = self.path_context.as_ref().unwrap().failed;
        if failed {
            reference.constructed_lag = None;
        }
        let id = self.alloc(
            ExprKind::PathNamespace {
                reference: Box::new(reference),
                lag,
            },
            Span {
                start: token.span.start,
                end,
            },
        );
        if failed {
            self.refused_constructors.insert(id);
            self.constructed.insert(id, self.constructors.opaque(id));
        }
        id
    }

    fn path_namespace_action(&mut self, reference: &PathReference, lag: Option<ExprId>) {
        let namespace = reference.namespace.as_deref().unwrap();
        let text = self.intern.get(reference.name).to_string();
        let syntax = if namespace == "learnt_in" {
            let period = match reference.learnt_in.as_ref().unwrap() {
                PeriodPoint::Integer(n) => n.to_string(),
                PeriodPoint::Date(date) => {
                    crate::constructor::path_learning_date(&date.constructor_text)
                }
                PeriodPoint::End => unreachable!(),
            };
            format!("learnt_in({period}).{text}")
        } else {
            format!("{namespace}.{text}")
        };
        if matches!(namespace, "init" | "initval") {
            self.path_type_action(reference.name, reference.span, "init");
            return;
        }
        if matches!(namespace, "self" | "prev" | "learnt_in")
            && !self.path_type_action(reference.name, reference.span, "varexo")
        {
            return;
        }
        if namespace == "prev"
            && matches!(
                self.path_context.as_ref().unwrap().learnt_in,
                None | Some(PeriodPoint::Integer(1))
            )
        {
            self.path_error(reference.span, "E411", format!("The syntax {syntax} is not accepted in a 'shock_paths' block without the 'learnt_in' option or in a 'shock_paths(learnt_in=1)' block"));
            return;
        }
        if namespace == "learnt_in"
            && let Some(PeriodPoint::Integer(n)) = reference.learnt_in.as_ref()
        {
            if *n < 1 {
                self.path_error(
                    reference.span,
                    "E412",
                    format!("The syntax {syntax} is not accepted"),
                );
                return;
            }
            let block_n = match self.path_context.as_ref().unwrap().learnt_in.as_ref() {
                Some(PeriodPoint::Integer(n)) => Some(*n),
                None => Some(1),
                _ => None,
            };
            if let Some(block_n) = block_n.filter(|block_n| block_n <= n) {
                self.path_error(reference.span, "E413", format!("The syntax {syntax} is not accepted in a 'shock_paths' block without the 'learnt_in' option or in a 'shock_paths(learnt_in={block_n})' block"));
                return;
            }
        }
        if self.path_context.as_ref().unwrap().controlled {
            self.path_error(reference.span, "E416", format!("The syntax {syntax} is not accepted in an 'endogenize' stanza of a 'shock_paths' block"));
            return;
        }
        if !matches!(namespace, "self" | "prev" | "learnt_in") {
            if self.model.final_symbol_kind(reference.name).is_none() {
                self.record_symbol_declaration(
                    reference.name,
                    reference.ident_span,
                    crate::model::SymbolKind::DatabaseVariable,
                );
            }
            if !self
                .path_context
                .as_ref()
                .unwrap()
                .databases
                .contains(namespace)
            {
                self.path_error(reference.span, "E415", format!("Unknown database: {namespace}. You may want to declare it via the 'database' command."));
                return;
            }
        }
        let integer = lag.map(|id| self.constructors.match_integer(self.constructed[&id]));
        if integer == Some(None) {
            self.path_error(reference.span, "E409", format!("Symbol {syntax} is being treated as if it were a function (i.e., passed an argument that is not an integer)."));
            return;
        }
        if namespace == "self" && integer.flatten().is_some_and(|n| n > 0) {
            self.path_error(
                reference.span,
                "E408",
                format!("The syntax {syntax} cannot be used with a lead"),
            );
        }
    }

    fn read_path_values(&mut self, from: usize, to: usize) -> Vec<WrittenValue> {
        let saved = self.i;
        self.i = from;
        let mut values = Vec::new();
        loop {
            let first = self.i;
            let issues_before = self.model.parse_issues.len();
            let expr = if !self.path_context.as_ref().unwrap().failed {
                self.parse_expr()
            } else {
                None
            };
            let refused = expr.is_some_and(|id| self.refused_constructors.contains(&id));
            if refused || self.model.parse_issues.len() != issues_before {
                self.path_context.as_mut().unwrap().failed = true;
            }
            if !self.path_context.as_ref().unwrap().failed {
                if expr.is_none() {
                    self.refuse_model_argument_syntax();
                } else if !self.at(TokenKind::Comma) && !self.at(TokenKind::Semi) {
                    let (span, unexpected) = self.ss_unexpected_token(self.i);
                    self.push_bison(
                        span,
                        format!("syntax error, unexpected {unexpected}, expecting COMMA or ';'"),
                    );
                }
                if self.model.parse_issues.len() != issues_before {
                    self.path_context.as_mut().unwrap().failed = true;
                }
            }
            let completed = expr.is_some() && !self.path_context.as_ref().unwrap().failed;
            if !completed {
                // Keep the recovered text, but do not execute later read actions.
                let mut depth = 0_i32;
                while self.i < to {
                    match self.tokens[self.i].kind {
                        TokenKind::LParen => depth += 1,
                        TokenKind::RParen => depth -= 1,
                        TokenKind::Comma if depth <= 0 => break,
                        _ => {}
                    }
                    self.i += 1;
                }
            }
            if first < self.i {
                let span = Span {
                    start: self.tokens[first].span.start,
                    end: self.tokens[self.i - 1].span.end,
                };
                let path_refs = expr
                    .map(|id| self.path_refs_from_expr(id))
                    .unwrap_or_default();
                values.push(WrittenValue {
                    text: self.src[span.start as usize..span.end as usize]
                        .trim()
                        .into(),
                    span,
                    expr,
                    path_refs,
                    completed,
                });
            }
            if !self.at(TokenKind::Comma) {
                break;
            }
            self.bump();
            if self.i == to && !self.path_context.as_ref().unwrap().failed {
                self.refuse_model_argument_syntax();
                self.path_context.as_mut().unwrap().failed = true;
                break;
            }
        }
        self.i = saved;
        values
    }

    fn path_stanza_action(&mut self, stanza: &PathStanza) -> bool {
        match &stanza.target {
            PathTarget::Exogenous { name, span } => {
                if !self.path_type_action(*name, *span, "varexo") {
                    return false;
                }
                if stanza.periods.len() != stanza.values.len() {
                    self.path_error(stanza.span, "E404", format!("shock_paths: variable {}: number of periods is different from number of shock values", self.intern.get(*name)));
                    return false;
                }
                self.record_path_value_facts(stanza);
                for (period, value) in stanza.periods.iter().zip(&stanza.values) {
                    let max_lag = self.model.path_value_facts[&value.expr.unwrap()].max_lag;
                    if let PeriodPoint::Integer(first) = period.first
                        && i64::from(first) <= max_lag
                    {
                        self.path_error(
                            value.span,
                            "E405",
                            format!(
                                "shock_paths: a lag of {max_lag} is not allowed at period {first}"
                            ),
                        );
                        return false;
                    }
                }
            }
            PathTarget::Controlled {
                exogenize,
                exogenize_span,
                endogenize,
                endogenize_span,
            } => {
                if !self.path_type_action(*exogenize, *exogenize_span, "var")
                    || !self.path_type_action(*endogenize, *endogenize_span, "varexo")
                {
                    return false;
                }
                if stanza.periods.len() != stanza.values.len() {
                    self.path_error(
                        stanza.span,
                        "E406",
                        "The number of periods is different from the number of values",
                    );
                    return false;
                }
                self.record_path_value_facts(stanza);
            }
        }
        true
    }

    fn record_path_value_facts(&mut self, stanza: &PathStanza) {
        let context = self.model.symbol_context();
        for value in &stanza.values {
            let id = value.expr.unwrap();
            let facts = self
                .constructors
                .path_value_facts(self.constructed[&id], |name| {
                    matches!(
                        self.model.symbol_kind_in_context(name, context),
                        Some("var" | "varexo" | "varexo_det" | "epilogue" | "database_variable")
                    )
                });
            self.model.path_value_facts.insert(id, facts);
        }
    }

    /// `end` can be a period in an exogenous path stanza. Dynare's lexer
    /// distinguishes that use from the block closer; our token lexer needs this
    /// small context check while searching for the closer.
    fn consume_until_path_end(&mut self, allow_period_end: bool) -> usize {
        let mut in_periods = false;
        let mut row_head = true;
        loop {
            if row_head
                && self.discard_refused_block_row(
                    allow_period_end
                        && (self.at_ident_ci("periods") || self.at_ident_ci("endogenize")),
                )
            {
                row_head = false;
                continue;
            }
            if self.at(TokenKind::Eof) || self.at_block_opener() {
                return self.i;
            }
            if self.at_ident_ci("periods") {
                in_periods = true;
            }
            if self.at_ident_ci("end") && !in_periods {
                let at = self.i;
                self.bump();
                self.advance_initial_source_cursor();
                self.finish_block_separator();
                return at;
            }
            if self.at(TokenKind::Semi) {
                in_periods = false;
            }
            row_head = self.bump().kind == TokenKind::Semi;
        }
    }

    pub(super) fn parse_path_block(&mut self, companion: bool) {
        let parse_issues_before = self.model.parse_issues.len();
        let shape_refuses_before = self.model.shape_refuses.len();
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
            let option_start = self.i;
            self.skip_balanced(TokenKind::LParen, TokenKind::RParen);
            if !companion {
                self.record_option_twice(option_start, self.i);
            }
        }
        let opener_end = if self.at(TokenKind::Semi) {
            self.bump().span.end
        } else {
            self.current_start()
        };
        let body_i = self.i;
        self.record_shock_opener_refuse(opener_i, body_i, None);
        if !companion {
            for i in opener_i + 1..body_i {
                if self.tokens[i].kind == TokenKind::Comma {
                    self.model.shape_refuses.push(ShapeRefuse::official(
                        self.tokens[i].span,
                        keyword,
                        "syntax error, unexpected COMMA, expecting OVERWRITE or LEARNT_IN or ')'",
                    ).with_parse_order(self.token_origins[i].start));
                    break;
                }
            }
        }
        let body_end_i = self.consume_until_path_end(!companion);
        let closed = self.word_at(body_end_i, "end")
            && self
                .tokens
                .get(body_end_i + 1)
                .is_some_and(|token| token.kind == TokenKind::Semi);
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
        let previous_path_context = self.path_context.take();
        if !companion {
            self.path_context =
                Some(PathExpressionContext {
                    learnt_in: options.learnt_in.clone(),
                    controlled: false,
                    databases: self
                        .model
                        .databases
                        .iter()
                        .flat_map(|decl| decl.names.iter())
                        .map(|(name, _)| self.intern.get(*name).to_string())
                        .collect(),
                    failed: self.model.option_twice.iter().any(|(_, span)| {
                        start_tok.span.start <= span.start && span.end <= opener_end
                    }) || self.model.shape_refuses.iter().any(|issue| {
                        start_tok.span.start <= issue.span.start && issue.span.end <= opener_end
                    }),
                });
            if let Some(PeriodPoint::Integer(n)) = options.learnt_in.as_ref()
                && *n < 1
            {
                self.path_error(
                    options.learnt_in_span.unwrap_or(start_tok.span),
                    "E421",
                    format!("Value '{n}' is not allowed for 'learnt_in' option"),
                );
            }
        }
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
                let expected = if stanzas.is_empty() {
                    "VAR or EXOGENIZE"
                } else {
                    "END"
                };
                self.refuse_path_stanza_syntax(companion, Some(expected));
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
            self.model.shape_refuses.push(
                ShapeRefuse::official(self.tokens[body_end_i].span, keyword, message)
                    .with_parse_order(self.token_origins[body_end_i].start),
            );
        }
        let completed = companion
            || closed
                && self.model.parse_issues.len() == parse_issues_before
                && self.model.shape_refuses.len() == shape_refuses_before
                && !stanzas.is_empty()
                && !self.path_context.as_ref().unwrap().failed
                && stanzas.iter().all(|stanza| stanza.callback_completed);
        self.path_context = previous_path_context;
        let block = PathBlock {
            options,
            stanzas,
            completed,
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

    fn refuse_path_stanza_syntax(&mut self, companion: bool, expected: Option<&str>) {
        if companion || self.path_context.as_ref().unwrap().failed {
            return;
        }
        let (span, unexpected) = self.ss_unexpected_token(self.i);
        let mut message = format!("syntax error, unexpected {unexpected}");
        if let Some(expected) = expected {
            message.push_str(&format!(", expecting {expected}"));
        }
        self.push_bison(span, message);
        self.path_context.as_mut().unwrap().failed = true;
    }

    fn read_path_stanza(
        &mut self,
        end_i: usize,
        controlled: bool,
        companion: bool,
    ) -> Option<PathStanza> {
        let row_i = self.i;
        let start = self.bump().span.start;
        let target_tok = self.tokens.get(self.i)?.clone();
        if target_tok.kind != TokenKind::Ident {
            self.refuse_path_stanza_syntax(companion, None);
            return None;
        }
        self.bump();
        let target_name = self.intern.intern(target_tok.text(self.src));
        if !self.at(TokenKind::Semi) {
            self.refuse_path_stanza_syntax(companion, Some("';'"));
            return None;
        }
        self.bump();
        if !self.at_ident_ci("periods") {
            self.refuse_path_stanza_syntax(companion, Some("PERIODS"));
            return None;
        }
        self.bump();
        let issues_before = self.model.shape_refuses.len();
        let periods = self.read_periods_until_semi(!controlled);
        if !companion && self.model.shape_refuses.len() != issues_before {
            self.path_context.as_mut().unwrap().failed = true;
        }
        if !self.at_ident_ci("values") {
            self.refuse_path_stanza_syntax(companion, Some("VALUES"));
            return None;
        }
        self.bump();
        let value_start = self.i;
        while self.i < end_i && !self.at(TokenKind::Semi) {
            self.bump();
        }
        let value_end = self.i;
        let values = if companion {
            self.written_values(value_start, value_end)
        } else {
            self.path_context.as_mut().unwrap().controlled = controlled;
            self.read_path_values(value_start, value_end)
        };
        self.eat(TokenKind::Semi);
        let target = if controlled {
            if !self.at_ident_ci("endogenize") {
                self.refuse_path_stanza_syntax(companion, Some("ENDOGENIZE"));
                return None;
            }
            self.bump();
            let endo_tok = self.tokens.get(self.i)?.clone();
            if endo_tok.kind != TokenKind::Ident {
                self.refuse_path_stanza_syntax(companion, None);
                return None;
            }
            self.bump();
            let endogenize = self.intern.intern(endo_tok.text(self.src));
            if !self.at(TokenKind::Semi) && !companion {
                self.refuse_path_stanza_syntax(companion, Some("';'"));
                return None;
            }
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
        let mut stanza = PathStanza {
            symbol_type_context: self.model.symbol_context(),
            target,
            periods,
            values,
            callback_completed: companion,
            span: Span { start, end },
        };
        if !companion
            && !self.path_context.as_ref().unwrap().failed
            && stanza.values.iter().all(|value| value.completed)
        {
            stanza.callback_completed = self.path_stanza_action(&stanza);
        }
        if stanza.callback_completed {
            self.retain_fact_receipt(
                "shock_instruction",
                row_i,
                std::iter::once(row_i..self.i).collect(),
            );
        }
        Some(stanza)
    }

    pub(super) fn collect_endval_instruction(
        &mut self,
        opener_i: usize,
        body_i: usize,
        body_end_i: usize,
        end: u32,
    ) {
        self.record_shock_opener_refuse(opener_i, body_i, None);
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
            let value = self.written_value(value_start, value_end);
            let row_end = if self.at(TokenKind::Semi) {
                self.bump().span.end
            } else {
                self.current_start()
            };
            if let Some(value) = value {
                self.retain_fact_receipt(
                    "shock_instruction",
                    start,
                    std::iter::once(start..self.i).collect(),
                );
                entries.push(EndvalEntry {
                    symbol_type_context: self.model.symbol_context(),
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
        let parse_order = self.i;
        let mut owned = std::iter::once(self.i..self.i + 1).collect::<Vec<_>>();
        let start = self.bump().span.start;
        let mut names = Vec::new();
        while !self.at(TokenKind::Eof) && !self.at(TokenKind::Semi) {
            if self.at(TokenKind::Ident) {
                owned.push(self.i..self.i + 1);
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
        self.retain_fact_receipt("database", parse_order, owned);
    }

    pub(super) fn parse_set_time_statement(&mut self) {
        let parse_order = self.i;
        let mut value_tokens = None;
        let start = self.bump().span.start;
        if self.at(TokenKind::LParen) {
            let first = self.i + 1;
            let message = match self.tokens.get(first).map(|t| t.kind) {
                Some(TokenKind::Plus) => Some("syntax error, unexpected PLUS, expecting DATE"),
                Some(TokenKind::Minus) if self.date_at(first).is_none() => {
                    Some("syntax error, unexpected MINUS, expecting DATE")
                }
                Some(TokenKind::Number) if self.date_at(first).is_none() => {
                    Some("syntax error, unexpected INT_NUMBER, expecting DATE")
                }
                Some(TokenKind::RParen) => Some("syntax error, unexpected ')', expecting DATE"),
                Some(TokenKind::Ident) if self.date_at(first).is_none() => {
                    Some("syntax error, unexpected IDENTIFIER, expecting DATE")
                }
                _ => None,
            };
            if let Some(message) = message {
                self.model.shape_refuses.push(
                    ShapeRefuse::official(self.tokens[first].span, "set_time", message)
                        .with_parse_order(self.token_origins[first].start),
                );
            }
        }
        let value = if self.at(TokenKind::LParen) {
            self.date_at(self.i + 1).map(|(date, next)| {
                value_tokens = Some(self.i + 1..next);
                if let Some(refuse) = self.date_suffix_refusal_at(
                    next,
                    "set_time",
                    "syntax error, unexpected MINUS, expecting PLUS or ')'",
                ) {
                    self.model.shape_refuses.push(refuse);
                }
                date
            })
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
            let mut owned = std::iter::once(parse_order..parse_order + 1).collect::<Vec<_>>();
            owned.extend(value_tokens);
            self.retain_fact_receipt("set_time", parse_order, owned);
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
            if date_option && let Some((value, next)) = self.date_at(i + 2) {
                self.model.date_options.push(DateOption {
                    command: command.to_ascii_lowercase(),
                    name,
                    span: Span {
                        start: tok.span.start,
                        end: value.span.end,
                    },
                    value,
                });
                self.retain_fact_receipt("date_option", i, std::iter::once(i..next).collect());
                i = next;
                continue;
            }
            i += 1;
        }
    }

    pub(super) fn collect_stoch_simul_request(
        &mut self,
        parse_order: usize,
        span: Span,
        option_range: Option<(usize, usize)>,
    ) {
        let mut request = StochSimulRequest {
            parse_order,
            option_tokens: option_range.map(|(open, close)| open..close),
            irf_tokens: None,
            irf_shocks_tokens: None,
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
                        && let Ok(number) = tok.text(self.src).parse::<i32>()
                    {
                        request.irf = Some((number, tok.span));
                        request.irf_tokens = Some(i..i + 3);
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
                    request.irf_shocks_tokens = Some(i..after);
                    i = after;
                    continue;
                }
                i += 1;
            }
        }
        self.model.stoch_simul_requests.push(request);
    }

    pub(super) fn collect_irf_shocks_option(&mut self, command: &str, open: usize, close: usize) {
        let mut i = open + 1;
        while i + 2 < close {
            if self.word_at(i, "irf")
                && self.tokens.get(i + 1).map(|t| t.kind) == Some(TokenKind::Eq)
                && let Some(value) = self.tokens.get(i + 2)
            {
                let message = if value.kind == TokenKind::Minus {
                    Some("syntax error, unexpected MINUS, expecting INT_NUMBER")
                } else if value.kind == TokenKind::Number
                    && !is_integer_lexeme(value.text(self.src))
                {
                    Some("syntax error, unexpected FLOAT_NUMBER, expecting INT_NUMBER")
                } else {
                    None
                };
                if let Some(message) = message {
                    self.model.shape_refuses.push(
                        ShapeRefuse::official(value.span, command, message)
                            .with_parse_order(self.token_origins[i + 2].start),
                    );
                }
            }
            if !self.word_at(i, "irf_shocks")
                || self.tokens.get(i + 1).map(|t| t.kind) != Some(TokenKind::Eq)
                || self.tokens.get(i + 2).map(|t| t.kind) != Some(TokenKind::LParen)
            {
                i += 1;
                continue;
            }
            let option_span = self.tokens[i].span;
            let after =
                skip_balanced_tokens(&self.tokens, i + 2, TokenKind::LParen, TokenKind::RParen)
                    .min(close);
            if self.tokens.get(i + 3).map(|t| t.kind) == Some(TokenKind::RParen) {
                self.model.shape_refuses.push(
                    ShapeRefuse::official(
                        self.tokens[i + 3].span,
                        command,
                        "syntax error, unexpected ')'",
                    )
                    .with_parse_order(self.token_origins[i + 3].start),
                );
            }
            if self.tokens.get(after.saturating_sub(2)).map(|t| t.kind) == Some(TokenKind::Comma) {
                self.model.shape_refuses.push(
                    ShapeRefuse::official(
                        self.tokens[after - 1].span,
                        command,
                        "syntax error, unexpected ')'",
                    )
                    .with_parse_order(self.token_origins[after - 1].start),
                );
            }
            let mut names = Vec::new();
            for j in i + 3..after.saturating_sub(1) {
                if self.tokens[j].kind == TokenKind::Ident {
                    let tok = &self.tokens[j];
                    names.push((self.intern.intern(tok.text(self.src)), tok.span));
                } else if self.tokens[j].kind == TokenKind::Number {
                    let message = if is_integer_lexeme(self.tokens[j].text(self.src)) {
                        "syntax error, unexpected INT_NUMBER"
                    } else {
                        "syntax error, unexpected FLOAT_NUMBER"
                    };
                    self.model.shape_refuses.push(
                        ShapeRefuse::official(self.tokens[j].span, command, message)
                            .with_parse_order(self.token_origins[j].start),
                    );
                }
            }
            self.model.irf_shocks_options.push(IrfShocksOption {
                parse_order: i,
                active_tokens: i..after,
                option_tokens: open..close,
                symbol_type_context: self.model.symbol_context(),
                command: command.to_ascii_lowercase(),
                span: option_span,
                names,
            });
            i = after;
        }
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
        let mut owned = std::iter::once(from..i).collect::<Vec<_>>();
        let span = Span {
            start: self.tokens[from].span.start,
            end: self.tokens[to.saturating_sub(1)].span.end,
        };
        if self.tokens.get(i).map(|t| t.kind) == Some(TokenKind::Eq) {
            i += 1;
            if let Some((source, after_source)) = self.subsample_head_at(i) {
                self.model.subsamples.push(SubsampleInstruction::Copy {
                    symbol_type_context: self.model.symbol_context(),
                    target: head,
                    source,
                    span,
                });
                owned.push(i - 1..after_source);
                self.retain_fact_receipt("subsamples", from, owned);
            }
            return;
        }
        if self.tokens.get(i).map(|t| t.kind) != Some(TokenKind::LParen) {
            return;
        }
        if self.tokens.get(i + 1).map(|t| t.kind) == Some(TokenKind::RParen) {
            self.model.shape_refuses.push(
                ShapeRefuse::official(
                    self.tokens[i + 1].span,
                    "subsamples",
                    "syntax error, unexpected ')'",
                )
                .with_parse_order(self.token_origins[i + 1].start),
            );
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
                if let Some(tok) = self.tokens.get(i + 2)
                    && tok.kind == TokenKind::Number
                {
                    self.model.shape_refuses.push(
                        ShapeRefuse::official(
                            tok.span,
                            "subsamples",
                            "syntax error, unexpected INT_NUMBER, expecting DATE",
                        )
                        .with_parse_order(self.token_origins[i + 2].start),
                    );
                }
                i += 1;
                continue;
            };
            if let Some(refuse) = self.date_suffix_refusal_at(
                after_first,
                "subsamples",
                "syntax error, unexpected MINUS, expecting PLUS or ':'",
            ) {
                self.model.shape_refuses.push(refuse);
            }
            if !self.gap_is(after_first - 1, after_first, ":") {
                if self.tokens.get(after_first).map(|t| t.kind) == Some(TokenKind::Comma) {
                    self.model.shape_refuses.push(
                        ShapeRefuse::official(
                            self.tokens[after_first].span,
                            "subsamples",
                            "syntax error, unexpected COMMA, expecting PLUS or ':'",
                        )
                        .with_parse_order(self.token_origins[after_first].start),
                    );
                }
                i += 1;
                continue;
            }
            let Some((last, after_last)) = self.date_at(after_first) else {
                if let Some(tok) = self.tokens.get(after_first)
                    && tok.kind == TokenKind::Number
                {
                    self.model.shape_refuses.push(
                        ShapeRefuse::official(
                            tok.span,
                            "subsamples",
                            "syntax error, unexpected INT_NUMBER, expecting DATE",
                        )
                        .with_parse_order(self.token_origins[after_first].start),
                    );
                }
                i += 1;
                continue;
            };
            if let Some(refuse) = self.date_suffix_refusal_at(
                after_last,
                "subsamples",
                "syntax error, unexpected MINUS, expecting COMMA or ')'",
            ) {
                self.model.shape_refuses.push(refuse);
            }
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
            owned.push(i..after_last);
            i = after_last;
        }
        self.model.subsamples.push(SubsampleInstruction::Declare {
            head,
            ranges,
            span,
            symbol_type_context: self.model.symbol_context(),
        });
        self.retain_fact_receipt("subsamples", from, owned);
    }

    fn shock_opener_refuse(&mut self, at: usize, command: &str, expected: &str) -> bool {
        let token = self.tokens[at].clone();
        let word = token.text(self.src).to_ascii_lowercase();
        let unexpected = match (token.kind, word.as_str()) {
            (TokenKind::Comma, _) => "COMMA",
            (TokenKind::RParen, _) => "')'",
            (TokenKind::Number, _) if is_integer_lexeme(token.text(self.src)) => "INT_NUMBER",
            (TokenKind::Number, _) => "FLOAT_NUMBER",
            (TokenKind::Eq, _) => "EQUAL",
            (_, "overwrite") => "OVERWRITE",
            (_, "surprise") => "SURPRISE",
            (_, "learnt_in") => "LEARNT_IN",
            (_, "heterogeneity") => "HETEROGENEITY",
            (_, "relative_to_initval") => "RELATIVE_TO_INITVAL",
            (_, "all_values_required") => "ALL_VALUES_REQUIRED",
            _ => "IDENTIFIER",
        };
        self.model.shape_refuses.push(
            ShapeRefuse::official(
                token.span,
                command,
                format!("syntax error, unexpected {unexpected}, expecting {expected}"),
            )
            .with_parse_order(self.token_origins[at].start),
        );
        true
    }

    /// The heterogeneous `shocks` opener takes only `heterogeneity=d` with an
    /// optional single `overwrite`, in either order. The pinned parser stops on
    /// the first token after that shape with `expecting ')'`; a differently
    /// shaped list is left to its later owner (02).
    fn record_hetero_shock_opener_refuse(&mut self, opener_i: usize, body_i: usize) -> bool {
        if self.tokens.get(opener_i + 1).map(|t| t.kind) != Some(TokenKind::LParen) {
            return false;
        }
        let Some(close_i) =
            (opener_i + 2..body_i).find(|&i| self.tokens[i].kind == TokenKind::RParen)
        else {
            return false;
        };
        let word_ci = |parser: &Self, i: usize| -> Option<String> {
            let tok = parser.tokens.get(i)?;
            (tok.kind == TokenKind::Ident).then(|| tok.text(parser.src).to_ascii_lowercase())
        };
        let kind_at = |parser: &Self, i: usize, kind: TokenKind| {
            parser.tokens.get(i).map(|t| t.kind) == Some(kind)
        };
        let shape_end = if word_ci(self, opener_i + 2).as_deref() == Some("overwrite")
            && kind_at(self, opener_i + 3, TokenKind::Comma)
            && word_ci(self, opener_i + 4).as_deref() == Some("heterogeneity")
            && kind_at(self, opener_i + 5, TokenKind::Eq)
            && kind_at(self, opener_i + 6, TokenKind::Ident)
        {
            Some(opener_i + 7)
        } else if word_ci(self, opener_i + 2).as_deref() == Some("heterogeneity")
            && kind_at(self, opener_i + 3, TokenKind::Eq)
            && kind_at(self, opener_i + 4, TokenKind::Ident)
        {
            let mut end = opener_i + 5;
            if kind_at(self, end, TokenKind::Comma)
                && word_ci(self, end + 1).as_deref() == Some("overwrite")
            {
                end += 2;
            }
            Some(end)
        } else {
            None
        };
        let Some(shape_end) = shape_end else {
            return false;
        };
        if shape_end >= close_i {
            return false;
        }
        let unexpected = self.bison_token_name(shape_end);
        self.model.shape_refuses.push(
            ShapeRefuse::official(
                self.tokens[shape_end].span,
                "shocks",
                format!("syntax error, unexpected {unexpected}, expecting ')'"),
            )
            .with_parse_order(self.token_origins[shape_end].start),
        );
        true
    }

    /// The six 0.7 openers have different option grammars. Keep the
    /// heterogeneous `shocks` variant with its later owner.
    fn record_shock_opener_refuse(
        &mut self,
        opener_i: usize,
        body_i: usize,
        kind: Option<ShockBlockKind>,
    ) -> bool {
        let command = self.tokens[opener_i].text(self.src).to_ascii_lowercase();
        if kind == Some(ShockBlockKind::Heterogeneous) {
            return self.record_hetero_shock_opener_refuse(opener_i, body_i);
        }
        if self.tokens.get(opener_i + 1).map(|t| t.kind) != Some(TokenKind::LParen) {
            return false;
        }
        let Some(close_i) =
            (opener_i + 2..body_i).find(|&i| self.tokens[i].kind == TokenKind::RParen)
        else {
            return false;
        };
        let first_i = opener_i + 2;
        if first_i == close_i {
            let expected = match command.as_str() {
                "shocks" => "OVERWRITE or SURPRISE or LEARNT_IN or HETEROGENEITY",
                "mshocks" => "OVERWRITE or LEARNT_IN or RELATIVE_TO_INITVAL",
                "heteroskedastic_shocks" => "OVERWRITE",
                "shock_paths" => "OVERWRITE or LEARNT_IN",
                "endval" => "ALL_VALUES_REQUIRED or LEARNT_IN",
                "perfect_foresight_controlled_paths" => "LEARNT_IN",
                _ => return false,
            };
            return self.shock_opener_refuse(close_i, &command, expected);
        }
        if first_i > close_i {
            return false;
        }
        let (allowed, expected): (&[&str], &str) = match command.as_str() {
            "shocks" => (
                &["overwrite", "surprise", "learnt_in"],
                "OVERWRITE or SURPRISE or LEARNT_IN or HETEROGENEITY",
            ),
            "mshocks" => (
                &["overwrite", "learnt_in", "relative_to_initval"],
                "OVERWRITE or LEARNT_IN or RELATIVE_TO_INITVAL",
            ),
            "heteroskedastic_shocks" => (&["overwrite"], "OVERWRITE"),
            "shock_paths" => (&["overwrite", "learnt_in"], "OVERWRITE or LEARNT_IN"),
            "endval" => (
                &["all_values_required", "learnt_in"],
                "ALL_VALUES_REQUIRED or LEARNT_IN",
            ),
            "perfect_foresight_controlled_paths" => (&["learnt_in"], "LEARNT_IN"),
            _ => return false,
        };
        if self.tokens[first_i].kind != TokenKind::Ident {
            return self.shock_opener_refuse(first_i, &command, expected);
        }
        let first = self.tokens[first_i].text(self.src).to_ascii_lowercase();
        if !allowed.contains(&first.as_str()) {
            return self.shock_opener_refuse(first_i, &command, expected);
        }
        let mut next_i = first_i + 1;
        if first == "learnt_in" {
            if self.tokens.get(next_i).map(|t| t.kind) != Some(TokenKind::Eq) {
                return self.shock_opener_refuse(next_i, &command, "EQUAL");
            }
            let value_i = next_i + 1;
            let Some((_, after_value)) = self.period_point_at(value_i, false) else {
                return self.shock_opener_refuse(value_i, &command, "DATE or INT_NUMBER");
            };
            next_i = after_value;
        }
        if matches!(command.as_str(), "mshocks" | "shock_paths") {
            let later_expected = if command == "mshocks" {
                "OVERWRITE or LEARNT_IN or RELATIVE_TO_INITVAL or ')'"
            } else {
                "OVERWRITE or LEARNT_IN or ')'"
            };
            while next_i < close_i {
                if self.tokens[next_i].kind == TokenKind::Comma {
                    return false; // Existing comma gate owns this syntax.
                }
                if self.tokens[next_i].kind != TokenKind::Ident {
                    return self.shock_opener_refuse(next_i, &command, later_expected);
                }
                let word = self.tokens[next_i].text(self.src).to_ascii_lowercase();
                if !allowed.contains(&word.as_str()) {
                    return self.shock_opener_refuse(next_i, &command, later_expected);
                }
                next_i += 1;
                if word == "learnt_in" {
                    if self.tokens.get(next_i).map(|t| t.kind) != Some(TokenKind::Eq) {
                        return self.shock_opener_refuse(next_i, &command, "EQUAL");
                    }
                    let value_i = next_i + 1;
                    let Some((_, after_value)) = self.period_point_at(value_i, false) else {
                        return self.shock_opener_refuse(value_i, &command, "DATE or INT_NUMBER");
                    };
                    next_i = after_value;
                }
            }
            return false;
        }
        if command != "shocks" {
            return next_i < close_i && self.shock_opener_refuse(next_i, &command, "')'");
        }
        if next_i >= close_i {
            return false;
        }
        if self.tokens[next_i].kind != TokenKind::Comma {
            return self.shock_opener_refuse(next_i, &command, "COMMA or ')'");
        }
        let second_i = next_i + 1;
        let (allowed_second, expected_second): (&[&str], &str) = if first == "overwrite" {
            (
                &["surprise", "learnt_in"],
                "SURPRISE or LEARNT_IN or HETEROGENEITY",
            )
        } else {
            (&["overwrite"], "OVERWRITE")
        };
        if second_i >= close_i || self.tokens[second_i].kind != TokenKind::Ident {
            return self.shock_opener_refuse(second_i, &command, expected_second);
        }
        let second = self.tokens[second_i].text(self.src).to_ascii_lowercase();
        if !allowed_second.contains(&second.as_str()) {
            return self.shock_opener_refuse(second_i, &command, expected_second);
        }
        let mut end_i = second_i + 1;
        if second == "learnt_in" {
            if self.tokens.get(end_i).map(|t| t.kind) != Some(TokenKind::Eq) {
                return self.shock_opener_refuse(end_i, &command, "EQUAL");
            }
            let value_i = end_i + 1;
            let Some((_, after_value)) = self.period_point_at(value_i, false) else {
                return self.shock_opener_refuse(value_i, &command, "DATE or INT_NUMBER");
            };
            end_i = after_value;
        }
        end_i < close_i && self.shock_opener_refuse(end_i, &command, "')'")
    }

    pub(super) fn record_shock_shape_refuses(
        &mut self,
        opener_i: usize,
        body_i: usize,
        body_end_i: usize,
        kind: ShockBlockKind,
    ) {
        let command = self.tokens[opener_i].text(self.src).to_string();
        if self.record_shock_opener_refuse(opener_i, body_i, Some(kind)) {
            return;
        }
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
                self.model.shape_refuses.push(
                    ShapeRefuse::official(self.tokens[body_end_i].span, &command, message)
                        .with_parse_order(self.token_origins[body_end_i].start),
                );
            }
        }
        if kind == ShockBlockKind::Regular {
            for i in body_i..body_end_i {
                let row_start = i == body_i || self.tokens[i - 1].kind == TokenKind::Semi;
                if !row_start || !self.incomplete_regular_shock_var(i, body_end_i) {
                    continue;
                }
                let next_i = i + 3;
                let token = &self.tokens[next_i];
                let expected = if self.word_at(next_i, "end") {
                    "PERIODS"
                } else {
                    "PERIODS or STDERR"
                };
                let unexpected = if token.kind == TokenKind::Eof {
                    "end of file".to_string()
                } else {
                    self.bison_token_name(next_i)
                };
                self.model.shape_refuses.push(
                    ShapeRefuse::official(
                        token.span,
                        &command,
                        format!("syntax error, unexpected {unexpected}, expecting {expected}"),
                    )
                    .with_parse_order(self.token_origins[next_i].start),
                );
            }
        }
        if matches!(
            kind,
            ShockBlockKind::Surprise
                | ShockBlockKind::LearntIn
                | ShockBlockKind::Multiplicative
                | ShockBlockKind::Heteroskedastic
        ) {
            for i in body_i..body_end_i.saturating_sub(2) {
                let row_start = i == body_i || self.tokens[i - 1].kind == TokenKind::Semi;
                if row_start
                    && self.word_at(i, "var")
                    && self.tokens[i + 1].kind == TokenKind::Ident
                    && self.tokens[i + 2].kind == TokenKind::Comma
                {
                    self.model.shape_refuses.push(
                        ShapeRefuse::official(
                            self.tokens[i + 2].span,
                            &command,
                            "syntax error, unexpected COMMA, expecting ';'",
                        )
                        .with_parse_order(self.token_origins[i + 2].start),
                    );
                    break;
                }
                if self.word_at(i, "var")
                    && self.tokens[i + 1].kind == TokenKind::Ident
                    && self.tokens[i + 2].kind == TokenKind::Eq
                {
                    self.model.shape_refuses.push(
                        ShapeRefuse::official(
                            self.tokens[i + 2].span,
                            &command,
                            "syntax error, unexpected EQUAL, expecting ';'",
                        )
                        .with_parse_order(self.token_origins[i + 2].start),
                    );
                }
                if self.word_at(i, "var")
                    && self.tokens[i + 1].kind == TokenKind::Ident
                    && self.tokens[i + 2].kind == TokenKind::Semi
                    && self.word_at(i + 3, "stderr")
                {
                    self.model.shape_refuses.push(
                        ShapeRefuse::official(
                            self.tokens[i + 3].span,
                            &command,
                            "syntax error, unexpected STDERR, expecting PERIODS",
                        )
                        .with_parse_order(self.token_origins[i + 3].start),
                    );
                }
            }
        }
        if kind != ShockBlockKind::Heterogeneous {
            for i in body_i..body_end_i {
                let row_start = i == body_i || self.tokens[i - 1].kind == TokenKind::Semi;
                if row_start
                    && matches!(
                        kind,
                        ShockBlockKind::Surprise
                            | ShockBlockKind::LearntIn
                            | ShockBlockKind::Multiplicative
                            | ShockBlockKind::Heteroskedastic
                    )
                    && (self.word_at(i, "corr") || self.word_at(i, "skew"))
                {
                    let message = if self.word_at(i, "corr") {
                        "syntax error, unexpected CORR, expecting VAR"
                    } else {
                        "syntax error, unexpected SKEW, expecting VAR"
                    };
                    self.model.shape_refuses.push(
                        ShapeRefuse::official(self.tokens[i].span, &command, message)
                            .with_parse_order(self.token_origins[i].start),
                    );
                    break;
                }
                if !self.word_at(i, "periods") {
                    continue;
                }
                let Some(end_i) =
                    (i + 1..body_end_i).find(|&j| self.tokens[j].kind == TokenKind::Semi)
                else {
                    continue;
                };
                let op_i = end_i + 1;
                if op_i >= body_end_i {
                    continue;
                }
                let operation = self.tokens[op_i].text(self.src).to_ascii_lowercase();
                let message = match (kind, operation.as_str()) {
                    (ShockBlockKind::Heteroskedastic, "add") => {
                        Some("syntax error, unexpected ADD, expecting VALUES or SCALES")
                    }
                    (ShockBlockKind::Heteroskedastic, "multiply") => {
                        Some("syntax error, unexpected MULTIPLY, expecting VALUES or SCALES")
                    }
                    (
                        ShockBlockKind::Regular
                        | ShockBlockKind::Surprise
                        | ShockBlockKind::LearntIn
                        | ShockBlockKind::Multiplicative,
                        "scales",
                    ) => {
                        Some("syntax error, unexpected SCALES, expecting VALUES or ADD or MULTIPLY")
                    }
                    _ => None,
                };
                if let Some(message) = message {
                    self.model.shape_refuses.push(
                        ShapeRefuse::official(self.tokens[op_i].span, &command, message)
                            .with_parse_order(self.token_origins[op_i].start),
                    );
                    break;
                }
            }
        }
        if command.eq_ignore_ascii_case("mshocks") {
            for i in opener_i + 1..body_i {
                if self.tokens[i].kind == TokenKind::Comma {
                    self.model.shape_refuses.push(ShapeRefuse::official(
                        self.tokens[i].span, &command,
                        "syntax error, unexpected COMMA, expecting OVERWRITE or LEARNT_IN or RELATIVE_TO_INITVAL or ')'",
                    ).with_parse_order(self.token_origins[i].start));
                    break;
                }
            }
        }
    }
}
