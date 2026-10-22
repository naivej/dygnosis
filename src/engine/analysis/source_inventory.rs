//! File-local written structure. These records never become accepted model facts.

use std::collections::{BTreeMap, HashSet};
use std::sync::{Arc, atomic::AtomicBool};

use crate::engine::analysis_configuration::{DirectiveSite, source_revision};
use crate::engine::language::lexer::{Token, TokenKind, tokenize};
use crate::engine::language::macro_expand::{
    discover_directive_sites_with_revision, scan_macro_tokens,
};
use crate::engine::language::macro_expr::MacroBudget;
use crate::engine::language::model::{ExecutionStep, MacroDirective, Model, StatementKind};
use crate::engine::language::parser::parse_expanded_controlled;
use crate::engine::language::span::Span;
use crate::engine::macro_work::MacroWorkLimit;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SourceItemKind {
    Declaration,
    Block,
    Command,
    Equation,
    MacroDefinition,
    MacroDirective,
    Conditional,
    Branch,
    Loop,
    Fragment,
}

/// One branch or loop body on the written path, independent of execution.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SourcePathStep {
    pub opener: Span,
    pub branch: Span,
    pub loop_body: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct SourceItem {
    /// File-local identity. The file revision guards its bytes separately.
    pub source_id: String,
    pub kind: SourceItemKind,
    pub span: Span,
    pub selection: Span,
    pub name: Option<String>,
    pub written_kind: Option<String>,
    pub path: Vec<SourcePathStep>,
    /// A proven containing written item, never an inferred semantic owner.
    pub parent: Option<usize>,
    pub tags: BTreeMap<String, String>,
    pub template: bool,
    pub header_end: u32,
    pub function: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SourceFold {
    pub span: Span,
    pub comment: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SourceContext {
    pub language: &'static str,
    pub parent: Option<String>,
}

/// A retained projection of the existing written-folding parse. It stores no
/// temporary Model, expressions, bindings, or effective equation numbers.
pub(crate) struct SourceInventory {
    pub source_revision: String,
    pub items: Vec<SourceItem>,
    pub folds: Vec<SourceFold>,
    pub sites: Vec<DirectiveSite>,
    pub macro_tokens: Vec<Token>,
    /// Inert written lexical tokens, shared with syntax presentation filters.
    pub lexical_tokens: Vec<Token>,
    pub coverage_cause: Option<String>,
    pub work_used: usize,
    pub parse_passes: usize,
    /// Written syntax can be complete even when guarded target discovery fails.
    structure_complete: bool,
    opaque: Vec<Span>,
    contexts: Vec<(u32, Vec<SourcePathStep>)>,
    interpolations: Vec<Span>,
    directive_spans: Vec<Span>,
}

struct MacroFrame<'a> {
    opener: &'a MacroDirective,
    parent: Vec<SourcePathStep>,
    branches: Vec<&'a MacroDirective>,
    item: usize,
}

impl SourceInventory {
    /// Reserve source work before scanning or parsing. A canceled build must
    /// not enter the document cache; a bounded result keeps its coverage cause.
    pub(crate) fn build(
        text: &str,
        work_limit: MacroWorkLimit,
        cancellation: Option<Arc<AtomicBool>>,
    ) -> Option<Self> {
        let mut budget = MacroBudget::with_limit(work_limit, cancellation);
        if crate::engine::control::flag_cancelled(budget.cancellation().as_deref()) {
            return None;
        }
        let mut result = Self {
            source_revision: String::new(),
            items: Vec::new(),
            folds: Vec::new(),
            sites: Vec::new(),
            macro_tokens: Vec::new(),
            lexical_tokens: Vec::new(),
            coverage_cause: None,
            work_used: 0,
            parse_passes: 0,
            structure_complete: false,
            opaque: Vec::new(),
            contexts: vec![(0, Vec::new())],
            interpolations: Vec::new(),
            directive_spans: Vec::new(),
        };
        result.build_inner(text, &mut budget);
        if crate::engine::control::flag_cancelled(budget.cancellation().as_deref()) {
            return None;
        }
        result.work_used = work_limit.get() - budget.remaining_work();
        Some(result)
    }

    fn reserve(&mut self, budget: &mut MacroBudget, amount: usize) -> bool {
        if budget.spend_work(amount.max(1)).is_ok() {
            true
        } else {
            self.coverage_cause = Some(
                "Written source inspection reached the work allowance; later source items are unavailable."
                    .into(),
            );
            false
        }
    }

    fn build_inner(&mut self, text: &str, budget: &mut MacroBudget) {
        // The parser's newline normalization preserves byte offsets: CRLF stays
        // CRLF, and each lone CR becomes one LF. Identity guards the raw text.
        if !self.reserve(budget, text.len().saturating_mul(3)) {
            return;
        }
        self.source_revision = source_revision(text);
        let normalized = crate::engine::language::parser::normalize_newlines(text);
        self.macro_tokens = scan_macro_tokens(&normalized);
        self.interpolations = self
            .macro_tokens
            .iter()
            .filter(|token| token.kind == TokenKind::MacroInterp)
            .map(|token| token.span)
            .collect();
        self.directive_spans = self
            .macro_tokens
            .iter()
            .filter(|token| token.kind == TokenKind::MacroDir)
            .map(|token| token.span)
            .collect();
        let directives: Vec<_> = self
            .macro_tokens
            .iter()
            .filter(|token| token.kind == TokenKind::MacroDir)
            .map(|token| {
                let raw = token.text(&normalized);
                let kind =
                    crate::engine::language::macro_expand::directive_name(raw).to_ascii_lowercase();
                MacroDirective {
                    argument: raw
                        .get(
                            raw.find(&kind)
                                .map_or(raw.len(), |start| start + kind.len())..,
                        )
                        .map(str::trim)
                        .filter(|argument| !argument.is_empty())
                        .map(str::to_owned),
                    kind,
                    span: token.span,
                }
            })
            .collect();
        self.macro_structure(&normalized, &directives, budget);
        if self.coverage_cause.is_some() {
            return;
        }
        if !self.reserve(budget, text.len().saturating_mul(3)) {
            return;
        }
        let inert = inert_source_from_tokens(&normalized, &self.macro_tokens);
        self.lexical_tokens = tokenize(&inert);
        let (model, _) = parse_expanded_controlled(
            &inert,
            self.lexical_tokens.clone(),
            budget.cancellation(),
            budget.work_limit(),
        );
        self.parse_passes += 1;
        self.collect_model(text, &model, budget);
        if self.coverage_cause.is_some() {
            return;
        }
        // Keep folding's one recovery pass. The retained source rows and folds
        // consume the same recovered facts, rather than parsing per feature.
        let mut recovery = inert.as_bytes().to_vec();
        let mut changed = false;
        for statement in &model.statements {
            if !self.reserve(budget, 1) {
                return;
            }
            if statement.kind != StatementKind::Block || statement.complete {
                continue;
            }
            if let Some(head) = model.expanded_tokens
                [statement.opener_range.end..statement.token_range.end]
                .iter()
                .find(|token| {
                    token.kind == TokenKind::Ident
                        && crate::engine::language::command_skip::is_pin_statement_keyword(
                            token.text(&inert),
                        )
                        && crate::engine::language::native_line::initial_content_start(
                            &inert,
                            line_start(&inert, token.span.start as usize),
                        ) == token.span.start as usize
                })
            {
                for byte in &mut recovery[statement.span.start as usize..head.span.start as usize] {
                    if !matches!(*byte, b'\n' | b'\r') {
                        *byte = b' ';
                    }
                }
                changed = true;
            }
        }
        if changed {
            if !self.reserve(budget, text.len().saturating_mul(2)) {
                return;
            }
            let recovery = String::from_utf8(recovery).expect("source masks preserve UTF-8");
            let (model, _) = parse_expanded_controlled(
                &recovery,
                tokenize(&recovery),
                budget.cancellation(),
                budget.work_limit(),
            );
            self.parse_passes += 1;
            self.collect_model(text, &model, budget);
        }
        if self.coverage_cause.is_some() || !self.reserve(budget, text.len()) {
            return;
        }
        self.folds
            .extend(comment_folds(&inert, &self.lexical_tokens));
        self.finish_items();
        self.structure_complete = true;
        match discover_directive_sites_with_revision(
            None,
            text,
            &self.source_revision,
            &self.macro_tokens,
            budget,
        ) {
            Ok(sites) => self.sites = sites,
            Err(error) => {
                self.coverage_cause = Some(format!(
                    "Macro directive targets are unavailable: {} Written source rows remain available.",
                    error.message,
                ));
            }
        }
    }

    fn item(
        &self,
        kind: SourceItemKind,
        span: Span,
        selection: Span,
        name: Option<String>,
    ) -> SourceItem {
        SourceItem {
            source_id: format!("source:{}:{}:{kind:?}", span.start, span.end),
            kind,
            span,
            selection,
            name,
            written_kind: None,
            path: self.path_at(selection.start).to_vec(),
            parent: None,
            tags: BTreeMap::new(),
            template: intersects(&self.interpolations, span),
            header_end: selection.end,
            function: false,
        }
    }

    fn macro_structure(
        &mut self,
        text: &str,
        directives: &[MacroDirective],
        budget: &mut MacroBudget,
    ) {
        let mut stack: Vec<MacroFrame<'_>> = Vec::new();
        let mut path = Vec::new();
        for directive in directives {
            if !self.reserve(budget, 1 + path.len()) {
                return;
            }
            let kind = directive.kind.as_str();
            let row_kind = if conditional(kind) {
                SourceItemKind::Conditional
            } else if kind == "for" {
                SourceItemKind::Loop
            } else if matches!(kind, "else" | "elseif") {
                SourceItemKind::Branch
            } else if kind == "define" {
                SourceItemKind::MacroDefinition
            } else {
                SourceItemKind::MacroDirective
            };
            let mut row = self.item(
                row_kind,
                directive.span,
                directive.span,
                Some(format!("@#{kind}")),
            );
            row.written_kind = Some(kind.into());
            row.path = path.clone();
            if kind == "define"
                && let Some((name, target, function)) = macro_name(text, directive.span)
            {
                row.name = Some(name);
                row.selection = target;
                row.function = function;
            }
            let item = self.items.len();
            self.items.push(row);
            if conditional(kind) || kind == "for" {
                if stack.len() >= 64 {
                    self.coverage_cause = Some(
                        "Written source inspection reached the macro nesting limit; later source items are unavailable."
                            .into(),
                    );
                    return;
                }
                stack.push(MacroFrame {
                    opener: directive,
                    parent: path.clone(),
                    branches: vec![directive],
                    item,
                });
                path.push(SourcePathStep {
                    opener: directive.span,
                    branch: directive.span,
                    loop_body: kind == "for",
                });
                self.contexts.push((directive.span.end, path.clone()));
            } else if matches!(kind, "else" | "elseif") {
                if let Some(frame) = stack.last_mut()
                    && conditional(&frame.opener.kind)
                {
                    frame.branches.push(directive);
                    path.clone_from(&frame.parent);
                    path.push(SourcePathStep {
                        opener: frame.opener.span,
                        branch: directive.span,
                        loop_body: false,
                    });
                    self.contexts.push((directive.span.end, path.clone()));
                }
            } else if let Some(frame) = stack.pop_if(|frame| {
                kind == "endif" && conditional(&frame.opener.kind)
                    || kind == "endfor" && frame.opener.kind == "for"
            }) {
                self.items[frame.item].span.end = directive.span.end;
                self.folds.push(region(Span {
                    start: frame.opener.span.start,
                    end: directive.span.end,
                }));
                if conditional(&frame.opener.kind) {
                    for (index, branch) in frame.branches.iter().enumerate() {
                        let start = if index == 0 {
                            let end = branch.span.end as usize;
                            if end > 0 && text.as_bytes()[end - 1] == b'\n' {
                                end
                            } else {
                                next_line(text, end)
                            }
                        } else {
                            branch.span.start as usize
                        };
                        let boundary = frame.branches.get(index + 1).unwrap_or(&directive);
                        let end = line_start(text, boundary.span.start as usize);
                        if start < end {
                            self.folds.push(region(Span::new(start, end)));
                        }
                    }
                }
                path = frame.parent;
                self.contexts.push((directive.span.start, path.clone()));
            }
        }
    }

    fn path_at(&self, offset: u32) -> &[SourcePathStep] {
        let index = self.contexts.partition_point(|(start, _)| *start <= offset);
        &self.contexts[index.saturating_sub(1)].1
    }

    pub(crate) fn macro_path(&self, byte: u32) -> &[SourcePathStep] {
        self.path_at(byte)
    }

    /// A literal occurrence anchor can prove its own macro path. A containing
    /// block's broad span cannot prove that its conditional children emitted.
    pub(crate) fn literal_anchor_on_path(&self, anchor: Span, byte: u32) -> bool {
        !anchor.is_empty()
            && !intersects(&self.interpolations, anchor)
            && !intersects(&self.directive_spans, anchor)
            && self.path_at(anchor.start) == self.path_at(byte)
            && self.path_at(anchor.end - 1) == self.path_at(byte)
    }

    fn local_span(&self, span: Span, block: bool) -> bool {
        self.path_at(span.start) == self.path_at(span.end.saturating_sub(1))
            && (block || !intersects(&self.directive_spans, span))
    }

    fn collect_model(&mut self, text: &str, model: &Model, budget: &mut MacroBudget) {
        let mut proven = HashSet::new();
        self.opaque
            .extend(model.execution_steps.iter().filter_map(|step| {
                if let ExecutionStep::Opaque(span) = step {
                    Some(*span)
                } else {
                    None
                }
            }));
        for statement in &model.statements {
            if !self.reserve(budget, 1 + self.path_at(statement.span.start).len()) {
                return;
            }
            let Some(span) = token_span(model, &statement.token_range) else {
                continue;
            };
            if !statement.native
                && statement.complete
                && self.path_at(span.start) == self.path_at(span.end - 1)
            {
                self.folds.push(region(span));
            }
            if statement.native || statement.kind == StatementKind::Assignment {
                self.opaque.push(statement.span);
                continue;
            }
            let block = statement.kind == StatementKind::Block;
            let header =
                token_span(model, &statement.opener_range).unwrap_or(statement.keyword_span);
            let literal_header = !intersects(&self.interpolations, statement.keyword_span);
            let complete = statement.complete && self.local_span(span, block) && literal_header;
            let kind = if !complete {
                SourceItemKind::Fragment
            } else {
                match statement.kind {
                    StatementKind::Declaration | StatementKind::Dimension => {
                        SourceItemKind::Declaration
                    }
                    StatementKind::Block => SourceItemKind::Block,
                    _ => SourceItemKind::Command,
                }
            };
            let mut row = self.item(
                kind,
                span,
                statement.keyword_span,
                Some(statement.name.clone()),
            );
            row.written_kind = Some(statement.name.clone());
            row.header_end = header.end;
            if complete {
                proven.insert(statement.id);
            }
            if statement.name == "verbatim" {
                self.opaque.push(span);
            }
            self.items.push(row);
        }
        for declaration in &model.written_declarations {
            if !self.reserve(
                budget,
                1 + self.path_at(declaration.declaration.span.start).len(),
            ) {
                return;
            }
            if !proven.contains(&declaration.statement_id) {
                continue;
            }
            let target = declaration.declaration.span;
            let literal = text.get(target.start as usize..target.end as usize);
            let name = model.name(declaration.declaration.name);
            if literal != Some(name) || intersects(&self.interpolations, target) {
                continue;
            }
            let mut row = self.item(
                SourceItemKind::Declaration,
                target,
                target,
                Some(name.to_owned()),
            );
            row.written_kind = Some(declaration.written_kind.clone());
            self.items.push(row);
        }
        for equation in &model.written_equations {
            if !self.reserve(budget, 1 + self.path_at(equation.equation.span.start).len()) {
                return;
            }
            let Some(span) = token_span(model, &equation.token_range) else {
                continue;
            };
            let complete = proven.contains(&equation.statement_id) && self.local_span(span, false);
            let mut row = self.item(
                if complete {
                    SourceItemKind::Equation
                } else {
                    SourceItemKind::Fragment
                },
                span,
                model.expanded_tokens[equation.token_range.start].span,
                None,
            );
            // Interpolated tags remain template text, never fabricated literal tags.
            let mut tag_end = span.start;
            let tokens = &model.expanded_tokens[equation.token_range.clone()];
            let mut at = 0;
            while tokens
                .get(at)
                .is_some_and(|token| token.kind == TokenKind::LBrack)
            {
                let Some(close) = tokens[at..]
                    .iter()
                    .position(|token| token.kind == TokenKind::RBrack)
                else {
                    break;
                };
                at += close + 1;
                tag_end = tokens[at - 1].span.end;
            }
            if !intersects(
                &self.interpolations,
                Span {
                    start: span.start,
                    end: tag_end,
                },
            ) {
                row.tags.clone_from(&equation.equation.tag_map);
            }
            row.name = row.tags.get("name").cloned();
            self.items.push(row);
        }
    }

    fn finish_items(&mut self) {
        self.items.sort_by_key(|row| {
            (
                row.span.start,
                std::cmp::Reverse(row.span.end),
                row.kind as u8,
            )
        });
        self.items.dedup_by(|a, b| {
            a.source_id == b.source_id && a.span == b.span && a.selection == b.selection
        });
        let mut parents: Vec<usize> = Vec::new();
        for index in 0..self.items.len() {
            let span = self.items[index].span;
            while parents.last().is_some_and(|parent| {
                let outer = self.items[*parent].span;
                span.start < outer.start || span.end > outer.end || span == outer
            }) {
                parents.pop();
            }
            self.items[index].parent = parents.last().copied();
            if matches!(
                self.items[index].kind,
                SourceItemKind::Block | SourceItemKind::Conditional | SourceItemKind::Loop
            ) {
                parents.push(index);
            }
        }
        self.folds
            .sort_by_key(|fold| (fold.span.start, fold.span.end, fold.comment));
        self.folds.dedup();
    }

    /// Context uses literal, complete source boundaries. Fragments withhold a
    /// guessed expression context; native MATLAB and verbatim stay opaque.
    pub(crate) fn context(&self, byte: u32) -> Option<SourceContext> {
        if !self.structure_complete
            || self
                .opaque
                .iter()
                .any(|span| span.start <= byte && byte < span.end)
        {
            return None;
        }
        let row = self
            .items
            .iter()
            .filter(|row| {
                matches!(
                    row.kind,
                    SourceItemKind::Block
                        | SourceItemKind::Command
                        | SourceItemKind::Declaration
                        | SourceItemKind::Fragment
                ) && row.span.start <= byte
                    && byte < row.span.end
            })
            .min_by_key(|row| row.span.end - row.span.start);
        let Some(row) = row else {
            return Some(SourceContext {
                language: "top_level",
                parent: None,
            });
        };
        if row.kind == SourceItemKind::Fragment {
            return None;
        }
        let body = row.kind == SourceItemKind::Block && byte >= row.header_end;
        Some(SourceContext {
            language: if body
                && matches!(
                    row.name.as_deref(),
                    Some("model" | "model_replace" | "epilogue")
                ) {
                "model_expression"
            } else if body {
                "expression"
            } else {
                "top_level"
            },
            parent: row.written_kind.clone(),
        })
    }

    pub(crate) fn compatible_candidates(&self, byte: u32) -> impl Iterator<Item = &SourceItem> {
        let path = self.path_at(byte);
        self.items.iter().filter(move |row| {
            row.kind == SourceItemKind::Declaration
                && row.name.is_some()
                && row.span == row.selection
                && !row.path.iter().any(|candidate| {
                    path.iter().any(|current| {
                        current.opener == candidate.opener && current.branch != candidate.branch
                    })
                })
        })
    }
}

pub(crate) fn inert_source_from_tokens(text: &str, tokens: &[Token]) -> String {
    let mut inert = text.as_bytes().to_vec();
    for token in tokens {
        let replacement = match token.kind {
            TokenKind::MacroDir | TokenKind::MacroPrefix => b' ',
            TokenKind::MacroInterp => b'x',
            _ => continue,
        };
        for byte in &mut inert[token.span.start as usize..token.span.end as usize] {
            if !matches!(*byte, b'\n' | b'\r') {
                *byte = replacement;
            }
        }
    }
    String::from_utf8(inert).expect("macro masks preserve UTF-8")
}

fn macro_name(text: &str, span: Span) -> Option<(String, Span, bool)> {
    let source = text.get(span.start as usize..span.end as usize)?;
    let directive = source.strip_prefix("@#")?.trim_start();
    let after = directive.get("define".len()..)?;
    let name_source = after.trim_start();
    let start = span.start as usize + source.len() - name_source.len();
    let end = name_source
        .bytes()
        .take_while(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
        .count();
    if end == 0 || name_source.as_bytes()[0].is_ascii_digit() {
        return None;
    }
    Some((
        name_source[..end].into(),
        Span::new(start, start + end),
        name_source[end..].trim_start().starts_with('('),
    ))
}

fn token_span(model: &Model, range: &std::ops::Range<usize>) -> Option<Span> {
    Some(Span {
        start: model.expanded_tokens.get(range.start)?.span.start,
        end: model
            .expanded_tokens
            .get(range.end.checked_sub(1)?)?
            .span
            .end,
    })
}

fn intersects(spans: &[Span], span: Span) -> bool {
    let at = spans.partition_point(|candidate| candidate.end <= span.start);
    spans
        .get(at)
        .is_some_and(|candidate| candidate.start < span.end)
}
fn conditional(kind: &str) -> bool {
    matches!(kind, "if" | "ifdef" | "ifndef")
}
fn region(span: Span) -> SourceFold {
    SourceFold {
        span,
        comment: false,
    }
}
fn line_start(text: &str, offset: usize) -> usize {
    text[..offset].rfind('\n').map_or(0, |index| index + 1)
}
fn next_line(text: &str, offset: usize) -> usize {
    text[offset..]
        .find('\n')
        .map_or(text.len(), |index| offset + index + 1)
}

fn comment_folds(text: &str, tokens: &[Token]) -> Vec<SourceFold> {
    let strings: Vec<_> = tokens
        .iter()
        .filter(|token| matches!(token.kind, TokenKind::String | TokenKind::Latex))
        .map(|token| token.span)
        .collect();
    let mut strings = strings.iter().peekable();
    let bytes = text.as_bytes();
    let mut offset = 0;
    let mut folds = Vec::new();
    while offset < bytes.len() {
        while strings
            .peek()
            .is_some_and(|span| span.end as usize <= offset)
        {
            strings.next();
        }
        if let Some(span) = strings.peek()
            && span.start as usize == offset
        {
            offset = span.end as usize;
        } else if bytes[offset..].starts_with(b"//") || bytes[offset] == b'%' {
            offset = next_line(text, offset);
        } else if bytes[offset..].starts_with(b"/*") {
            let start = offset;
            let Some(end) = text[offset + 2..].find("*/") else {
                break;
            };
            offset += end + 4;
            folds.push(SourceFold {
                span: Span::new(start, offset),
                comment: true,
            });
        } else {
            offset += 1;
        }
    }
    folds
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inventory(text: &str) -> SourceInventory {
        SourceInventory::build(text, Default::default(), None).unwrap()
    }

    #[test]
    fn conditional_candidates_keep_written_kinds_and_sibling_paths() {
        // Guards union parsing turning alternative declarations into one binding.
        let text = "@#if UNKNOWN\nvar y;\n@#else\nparameters y;\n@#endif\n";
        let source = inventory(text);
        let candidates: Vec<_> = source
            .items
            .iter()
            .filter(|row| {
                row.kind == SourceItemKind::Declaration && row.name.as_deref() == Some("y")
            })
            .collect();
        assert_eq!(candidates.len(), 2);
        assert_eq!(candidates[0].written_kind.as_deref(), Some("var"));
        assert_eq!(candidates[1].written_kind.as_deref(), Some("parameters"));
        assert_ne!(candidates[0].path, candidates[1].path);
        let at = text.find("var y").unwrap() as u32;
        let compatible: Vec<_> = source.compatible_candidates(at).collect();
        assert_eq!(compatible.len(), 1);
        assert_eq!(compatible[0].written_kind.as_deref(), Some("var"));
        assert!(
            source.coverage_cause.is_none(),
            "{:?}",
            source.coverage_cause
        );
    }

    #[test]
    fn generated_names_remain_templates_and_literal_tags_survive() {
        // Guards placeholder names escaping as candidates and losing written
        // tags merely because the equation body contains an interpolation.
        let text = "@#for co in []\nvar literal GDP_@{co};\n@#endfor\nmodel;\n@#for co in []\n[name='output'] GDP_@{co} = @{VALUE};\n@#endfor\nend;\n";
        let source = inventory(text);
        assert!(
            source
                .items
                .iter()
                .any(|row| row.kind == SourceItemKind::Declaration
                    && row.name.as_deref() == Some("literal"))
        );
        assert!(!source.items.iter().any(|row| {
            row.kind == SourceItemKind::Declaration
                && row
                    .name
                    .as_deref()
                    .is_some_and(|name| name.starts_with("GDP_"))
        }));
        let equation = source
            .items
            .iter()
            .find(|row| row.kind == SourceItemKind::Equation)
            .unwrap();
        assert!(equation.template);
        assert_eq!(
            equation.tags.get("name").map(String::as_str),
            Some("output")
        );
    }

    #[test]
    fn split_blocks_and_statements_are_fragments_without_guessed_scope() {
        // Guards borrowing a sibling arm's delimiter and inventing a complete
        // declaration across a macro choice.
        let text = "@#if UNKNOWN\nmodel;\ny=0;\n@#else\nend;\n@#endif\nvar\n@#if UNKNOWN\na\n@#else\nb\n@#endif\n;\n";
        let source = inventory(text);
        assert!(
            !source
                .items
                .iter()
                .any(|row| row.kind == SourceItemKind::Block)
        );
        assert!(
            !source
                .items
                .iter()
                .any(|row| row.kind == SourceItemKind::Declaration
                    && matches!(row.name.as_deref(), Some("a" | "b")))
        );
        assert!(
            source
                .items
                .iter()
                .any(|row| row.kind == SourceItemKind::Fragment)
        );
        assert!(source.context(text.find("y=0").unwrap() as u32).is_none());
    }

    #[test]
    fn original_bytes_and_native_boundaries_remain_exact() {
        // Guards normalized-coordinate drift and native text gaining Dynare
        // outline entries or expression context.
        let text = "// café\r\n@#define FLAG = 0\r\nvar y;\r\ndisp('model;');\r\nverbatim;\r\nvar fake;\r\nend;\r\nmodel;\r\ny=0;\r\nend;\r\n";
        let source = inventory(text);
        assert_eq!(source.source_revision, source_revision(text));
        let flag = source
            .items
            .iter()
            .find(|row| row.name.as_deref() == Some("FLAG"))
            .unwrap();
        assert_eq!(
            &text[flag.selection.start as usize..flag.selection.end as usize],
            "FLAG"
        );
        assert!(source.context(text.find("disp").unwrap() as u32).is_none());
        assert!(
            source
                .context(text.find("var fake").unwrap() as u32)
                .is_none()
        );
        assert!(
            !source
                .items
                .iter()
                .any(|row| row.name.as_deref() == Some("fake"))
        );
    }

    #[test]
    fn bounded_and_canceled_inspection_withholds_unbuilt_facts() {
        // Guards a source budget failure becoming a successful empty inventory,
        // and canceled source facts entering a shared document cache.
        let source =
            SourceInventory::build("var y;", MacroWorkLimit::new(1).unwrap(), None).unwrap();
        assert!(source.coverage_cause.is_some());
        assert!(source.items.is_empty());
        let flag = Arc::new(AtomicBool::new(true));
        assert!(SourceInventory::build("var y;", Default::default(), Some(flag)).is_none());
    }

    #[test]
    fn lone_carriage_return_keeps_macro_and_model_boundaries() {
        // Guards the cached inventory scanning raw lone CR as ordinary text,
        // while the existing parser treats that byte as a line boundary.
        let text = "/* 🚀 */ var y;\r\n/* α */ model;\r@#for k in 1:2\r\n[name='row'] y=@{k};\r\n@#endfor\nend;\r\n";
        let source = inventory(text);
        assert_eq!(source.source_revision, source_revision(text));
        let loop_row = source
            .items
            .iter()
            .find(|row| row.kind == SourceItemKind::Loop)
            .unwrap();
        assert_eq!(
            &text[loop_row.selection.start as usize..loop_row.selection.end as usize],
            "@#for k in 1:2"
        );
        let model = source
            .items
            .iter()
            .find(|row| row.kind == SourceItemKind::Block && row.name.as_deref() == Some("model"))
            .unwrap();
        assert!(source.folds.iter().any(|fold| fold.span == model.span));
        assert!(source.folds.iter().any(|fold| fold.span == loop_row.span));
        assert!(
            source.coverage_cause.is_none(),
            "{:?}",
            source.coverage_cause
        );
    }
}
