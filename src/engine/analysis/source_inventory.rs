//! File-local written structure. These records never become accepted model facts.

use std::collections::{BTreeMap, HashSet};
use std::sync::{Arc, atomic::AtomicBool};

use crate::engine::analysis_configuration::DirectiveSite;
use crate::engine::language::lexer::{Token, TokenKind, tokenize};
use crate::engine::language::macro_expr::MacroBudget;
use crate::engine::language::macro_source::MacroSource;
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
    /// Outline stops a conditional arm before the next sibling or endif.
    /// Source inspection keeps the full conditional or directive span above.
    pub branch_end: Option<u32>,
    pub selection: Span,
    pub name: Option<String>,
    /// Literal display label override for a named source construct.
    pub outline_name: Option<String>,
    /// Exact source token to select for `outline_name`.
    pub outline_selection: Option<Span>,
    /// Named child identities retained from the existing parser pass.
    pub outline_children: Vec<SourceOutlineChild>,
    /// RHS bytes for a model-local definition selected at its name token.
    pub local_definition: Option<Span>,
    pub written_kind: Option<String>,
    pub declaration_metadata: Option<Box<SourceDeclarationMetadata>>,
    pub path: Vec<SourcePathStep>,
    /// A proven containing written item, never an inferred semantic owner.
    pub parent: Option<usize>,
    pub tags: BTreeMap<String, String>,
    pub template: bool,
    pub header_end: u32,
    pub function: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct SourceOutlineChild {
    pub name: String,
    pub span: Span,
    pub selection: Span,
    pub detail: Option<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct SourceDeclarationMetadata {
    pub long_name: Option<String>,
    pub tex_name: Option<String>,
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
    pub sites: Arc<[DirectiveSite]>,
    pub macro_tokens: Arc<[Token]>,
    /// Inert written lexical tokens, shared with syntax presentation filters.
    pub lexical_tokens: Vec<Token>,
    pub coverage_cause: Option<String>,
    /// Target discovery can fail while written structure remains complete.
    pub target_coverage_cause: Option<String>,
    pub work_used: usize,
    pub parse_passes: usize,
    /// Written syntax can be complete even when guarded target discovery fails.
    structure_complete: bool,
    targets_complete: bool,
    opaque: Vec<Span>,
    /// Binding-independent written verbatim boundaries. Generic opaque/native
    /// recovery and assignments are not proof for a source edit decision.
    native_proof: Vec<Span>,
    contexts: Vec<(u32, Vec<SourcePathStep>)>,
    interpolations: Vec<Span>,
    directive_spans: Vec<Span>,
}

struct MacroFrame<'a> {
    opener: &'a MacroDirective,
    parent: Vec<SourcePathStep>,
    branches: Vec<(&'a MacroDirective, usize)>,
    item: usize,
}

impl SourceInventory {
    fn empty() -> Self {
        Self {
            source_revision: String::new(),
            items: Vec::new(),
            folds: Vec::new(),
            sites: Arc::from([]),
            macro_tokens: Arc::from([]),
            lexical_tokens: Vec::new(),
            coverage_cause: None,
            target_coverage_cause: None,
            work_used: 0,
            parse_passes: 0,
            structure_complete: false,
            targets_complete: false,
            opaque: Vec::new(),
            native_proof: Vec::new(),
            contexts: vec![(0, Vec::new())],
            interpolations: Vec::new(),
            directive_spans: Vec::new(),
        }
    }

    /// A resource stop in MacroSource acquisition has no reusable scan. Keep
    /// its outcome without retrying acquisition or presenting empty success.
    pub(crate) fn unavailable(cause: String) -> Self {
        let mut result = Self::empty();
        result.coverage_cause = Some(cause);
        result
    }

    /// Retain the same macro scan and guarded sites used by execution. Only
    /// the written folding/structure pass consumes this method's work budget.
    pub(crate) fn build_from_macro_source(
        source: Arc<MacroSource>,
        work_limit: MacroWorkLimit,
        cancellation: Option<Arc<AtomicBool>>,
    ) -> Option<Self> {
        let mut budget = MacroBudget::with_limit(work_limit, cancellation);
        if crate::engine::control::flag_cancelled(budget.cancellation().as_deref()) {
            return None;
        }
        let mut result = Self::empty();
        result.macro_tokens = Arc::clone(&source.tokens);
        result.sites = Arc::clone(&source.sites);
        result.targets_complete = source.syntax_refusal.is_none();
        result.target_coverage_cause = source.syntax_refusal.as_ref().map(|refusal| {
            format!(
                "Macro directive targets are unavailable: {}",
                refusal.message,
            )
        });
        if result.reserve(&mut budget, source.source_revision.len()) {
            result.source_revision.clone_from(&source.source_revision);
            result.build_inner(&source, &mut budget);
        }
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

    fn build_inner(&mut self, source: &MacroSource, budget: &mut MacroBudget) {
        let text = source.original.as_str();
        let normalized = source.normalized.as_str();
        // Alias retained facts; do not repeat macro scanning, normalization,
        // hashing or target validation in a source presentation request.
        if !self.reserve(budget, self.macro_tokens.len()) {
            return;
        }
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
                let raw = token.text(normalized);
                let kind =
                    crate::engine::language::macro_expand::directive_name(raw).to_ascii_lowercase();
                MacroDirective {
                    argument: None,
                    kind,
                    span: token.span,
                }
            })
            .collect();
        self.macro_structure(normalized, &directives, budget);
        if self.coverage_cause.is_some() {
            return;
        }
        if !self.reserve(budget, text.len().saturating_mul(3)) {
            return;
        }
        let inert = inert_source_from_tokens(normalized, &self.macro_tokens);
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
        self.coverage_cause = self
            .target_coverage_cause
            .as_ref()
            .map(|cause| format!("{cause} Written source rows remain available."));
    }

    pub(crate) fn structure_complete(&self) -> bool {
        self.structure_complete
    }

    pub(crate) fn structure_cause(&self) -> Option<&str> {
        (!self.structure_complete)
            .then_some(self.coverage_cause.as_deref())
            .flatten()
    }

    pub(crate) fn targets_complete(&self) -> bool {
        self.targets_complete
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
            branch_end: None,
            selection,
            name,
            outline_name: None,
            outline_selection: None,
            outline_children: Vec::new(),
            local_definition: None,
            written_kind: None,
            declaration_metadata: None,
            path: self.path_at(selection.start).to_vec(),
            parent: None,
            tags: BTreeMap::new(),
            template: intersects(&self.interpolations, span),
            header_end: selection.end,
            function: false,
        }
    }

    fn outline_name(
        &mut self,
        item: usize,
        name: String,
        selection: Span,
        budget: &mut MacroBudget,
    ) -> bool {
        if !self.reserve(budget, name.len() + std::mem::size_of::<Span>()) {
            return false;
        }
        let row = &mut self.items[item];
        row.outline_name = Some(name);
        row.outline_selection = Some(selection);
        true
    }

    fn outline_child(
        &mut self,
        item: usize,
        name: String,
        span: Span,
        selection: Span,
        detail: Option<String>,
        budget: &mut MacroBudget,
    ) -> bool {
        let retained = std::mem::size_of::<SourceOutlineChild>()
            + name.len()
            + detail.as_ref().map_or(0, String::len);
        if !self.reserve(budget, retained) {
            return false;
        }
        self.items[item].outline_children.push(SourceOutlineChild {
            name,
            span,
            selection,
            detail,
        });
        true
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
            if !self.reserve(budget, std::mem::size_of::<SourceItem>() + path.len()) {
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
                    branches: vec![(directive, item)],
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
                    frame.branches.push((directive, item));
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
                if conditional(&frame.opener.kind) {
                    for (index, (branch, item)) in frame.branches.iter().enumerate() {
                        let start = branch.span.start as usize;
                        let boundary = frame
                            .branches
                            .get(index + 1)
                            .map_or(directive, |(branch, _)| *branch);
                        let end = line_start(text, boundary.span.start as usize);
                        self.items[*item].branch_end = Some(end as u32);
                        if start < end {
                            self.folds.push(region(Span::new(start, end)));
                        }
                    }
                } else {
                    self.folds.push(region(Span {
                        start: frame.opener.span.start,
                        end: directive.span.end,
                    }));
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
        let mut item_by_statement: Vec<Option<usize>> = vec![None; model.statements.len()];
        self.opaque
            .extend(model.execution_steps.iter().filter_map(|step| {
                if let ExecutionStep::Opaque(span) = step {
                    Some(*span)
                } else {
                    None
                }
            }));
        for statement in &model.statements {
            if !self.reserve(
                budget,
                std::mem::size_of::<SourceItem>() + self.path_at(statement.span.start).len(),
            ) {
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
                if complete && !intersects(&self.interpolations, span) {
                    self.native_proof.push(span);
                }
            }
            item_by_statement[statement.id] = Some(self.items.len());
            self.items.push(row);
        }
        let statement_for_token = |order: usize| {
            let index = model
                .statements
                .partition_point(|statement| statement.token_range.start <= order);
            let statement = model.statements.get(index.checked_sub(1)?)?;
            statement
                .token_range
                .contains(&order)
                .then_some(statement.id)
        };
        let statement_for_span_start = |start: u32| {
            let index = model
                .statements
                .partition_point(|statement| statement.span.start < start);
            model
                .statements
                .get(index)
                .filter(|statement| statement.span.start == start)
                .map(|statement| statement.id)
        };
        for slot in &model.named_slots {
            let (statement_id, object_name) = match slot.owner {
                crate::engine::language::model::NamedSlotOwner::ModelCommand(index)
                    if slot.declaration =>
                {
                    let command = &model.semi_structural_commands[index];
                    let Some(statement_id) = statement_for_token(command.parse_order) else {
                        continue;
                    };
                    if !proven.contains(&statement_id) {
                        continue;
                    }
                    (statement_id, true)
                }
                crate::engine::language::model::NamedSlotOwner::DatabaseDeclaration => {
                    let Some(statement_id) = statement_for_token(slot.order) else {
                        continue;
                    };
                    if !proven.contains(&statement_id) {
                        continue;
                    }
                    (statement_id, false)
                }
                _ => continue,
            };
            let Some(item) = item_by_statement.get(statement_id).copied().flatten() else {
                continue;
            };
            let name = model.name(slot.name);
            if text.get(slot.span.start as usize..slot.span.end as usize) != Some(name)
                || intersects(&self.interpolations, slot.span)
            {
                continue;
            }
            if object_name {
                let label = format!("{}({name})", model.statements[statement_id].name);
                if !self.outline_name(item, label, slot.span, budget) {
                    return;
                }
            } else if !self.outline_child(
                item,
                name.to_owned(),
                slot.span,
                slot.span,
                Some("database".to_owned()),
                budget,
            ) {
                return;
            }
        }
        for block in &model.pac_target_info {
            let Some(statement_id) = statement_for_token(block.parse_order) else {
                continue;
            };
            if !proven.contains(&statement_id) {
                continue;
            }
            let Some(item) = item_by_statement.get(statement_id).copied().flatten() else {
                continue;
            };
            let name = model.name(block.name);
            if text.get(block.name_span.start as usize..block.name_span.end as usize) == Some(name)
                && !intersects(&self.interpolations, block.name_span)
            {
                let label = format!("pac_target_info({name})");
                if !self.outline_name(item, label, block.name_span, budget) {
                    return;
                }
            }
            for target in &block.target_tokens {
                let expression_start = target.start + 1;
                let expression = &model.expanded_tokens[expression_start..target.end];
                let Some(span) = token_span(model, target) else {
                    continue;
                };
                let expression_span = token_span(model, &(expression_start..target.end));
                let Some(expression_span) = expression_span else {
                    continue;
                };
                if intersects(&self.interpolations, expression_span) {
                    continue;
                }
                let label = format!(
                    "target {}",
                    crate::engine::language::parser::join_lexemes(&model.source, expression)
                );
                let selection = model.expanded_tokens[expression_start].span;
                if !self.outline_child(item, label, span, selection, None, budget) {
                    return;
                }
            }
            for row in &block.rows {
                let crate::engine::language::model::PacTargetInfoRow::Component(component) = row
                else {
                    continue;
                };
                let expression_span = token_span(model, &component.expression_tokens);
                let Some(expression_span) = expression_span else {
                    continue;
                };
                if intersects(&self.interpolations, expression_span) {
                    continue;
                }
                let expression = &model.expanded_tokens[component.expression_tokens.clone()];
                let name = format!(
                    "component {}",
                    crate::engine::language::parser::join_lexemes(&model.source, expression)
                );
                let Some(span) = token_span(model, &component.active_tokens) else {
                    continue;
                };
                let selection = model.expanded_tokens[component.expression_tokens.start].span;
                if !self.outline_child(item, name, span, selection, None, budget) {
                    return;
                }
            }
        }
        for block in &model.shock_group_blocks {
            let Some(statement_id) = statement_for_span_start(block.span.start) else {
                continue;
            };
            if !proven.contains(&statement_id) {
                continue;
            }
            let Some(item) = item_by_statement.get(statement_id).copied().flatten() else {
                continue;
            };
            if let Some(span) = block.group_span
                && text.get(span.start as usize..span.end as usize) == Some(block.group.as_str())
                && !intersects(&self.interpolations, span)
            {
                let label = format!("shock_groups({})", block.group);
                if !self.outline_name(item, label, span, budget) {
                    return;
                }
            }
            for group in &model.shock_groups[block.row_start..block.row_end] {
                if intersects(&self.interpolations, group.label_span) {
                    continue;
                }
                if !self.outline_child(
                    item,
                    group.label.clone(),
                    group.span,
                    group.label_span,
                    None,
                    budget,
                ) {
                    return;
                }
            }
        }
        for block in &model.init2shocks_blocks {
            let Some(statement_id) = statement_for_span_start(block.span.start) else {
                continue;
            };
            if !proven.contains(&statement_id) {
                continue;
            }
            let Some(item) = item_by_statement.get(statement_id).copied().flatten() else {
                continue;
            };
            if let Some(span) = block.group_span
                && text.get(span.start as usize..span.end as usize) == Some(block.group.as_str())
                && !intersects(&self.interpolations, span)
            {
                let label = format!("init2shocks({})", block.group);
                if !self.outline_name(item, label, span, budget) {
                    return;
                }
            }
        }
        for statement in &model.statements {
            if statement.name != "occbin_constraints" {
                continue;
            }
            if !proven.contains(&statement.id) {
                continue;
            }
            let Some(item) = item_by_statement.get(statement.id).copied().flatten() else {
                continue;
            };
            let from = model
                .occbin_constraints
                .partition_point(|constraint| constraint.parse_order < statement.token_range.start);
            for constraint in model.occbin_constraints[from..]
                .iter()
                .take_while(|constraint| constraint.parse_order < statement.token_range.end)
            {
                if intersects(&self.interpolations, constraint.name_span) {
                    continue;
                }
                if !self.outline_child(
                    item,
                    constraint.name.clone(),
                    constraint.span,
                    constraint.name_span,
                    None,
                    budget,
                ) {
                    return;
                }
            }
        }
        for instruction in &model.subsamples {
            let crate::engine::language::model::SubsampleInstruction::Declare {
                head,
                ranges,
                span,
                ..
            } = instruction
            else {
                continue;
            };
            let Some(statement_id) = statement_for_span_start(span.start) else {
                continue;
            };
            if !proven.contains(&statement_id) {
                continue;
            }
            let Some(item) = item_by_statement.get(statement_id).copied().flatten() else {
                continue;
            };
            let head = match head {
                crate::engine::language::model::SubsampleHead::Symbol(name, _)
                | crate::engine::language::model::SubsampleHead::Std(name, _) => {
                    model.name(*name).to_owned()
                }
                crate::engine::language::model::SubsampleHead::Corr(first, _, second, _) => {
                    format!("{},{}", model.name(*first), model.name(*second))
                }
            };
            for range in ranges {
                let name = model.name(range.name);
                if text.get(range.name_span.start as usize..range.name_span.end as usize)
                    != Some(name)
                    || intersects(&self.interpolations, range.name_span)
                {
                    continue;
                }
                if !self.outline_child(
                    item,
                    name.to_owned(),
                    range.span,
                    range.name_span,
                    Some(format!("named subsample · subsamples({head})")),
                    budget,
                ) {
                    return;
                }
            }
        }
        for (declaration_index, declaration) in model.written_declarations.iter().enumerate() {
            if !self.reserve(
                budget,
                std::mem::size_of::<SourceItem>()
                    + self.path_at(declaration.declaration.span.start).len(),
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
            let metadata = &declaration.declaration;
            // The parser's declaration token range selects the name only.
            // Adjacent retained names bound this name's trailing metadata.
            let metadata_span = Span {
                start: target.end,
                end: model
                    .written_declarations
                    .get(declaration_index + 1)
                    .filter(|next| next.statement_id == declaration.statement_id)
                    .map_or(
                        model.statements[declaration.statement_id].span.end,
                        |next| next.declaration.span.start,
                    ),
            };
            if (metadata.long_name.is_some() || metadata.tex_name.is_some())
                && !intersects(&self.interpolations, metadata_span)
            {
                if !self.reserve(
                    budget,
                    std::mem::size_of::<SourceDeclarationMetadata>()
                        + metadata.long_name.as_ref().map_or(0, String::len)
                        + metadata.tex_name.as_ref().map_or(0, String::len),
                ) {
                    return;
                }
                row.declaration_metadata = Some(Box::new(SourceDeclarationMetadata {
                    long_name: metadata.long_name.clone(),
                    tex_name: metadata.tex_name.clone(),
                }));
            }
            self.items.push(row);
        }
        for equation in &model.written_equations {
            if !self.reserve(
                budget,
                std::mem::size_of::<SourceItem>()
                    + self.path_at(equation.equation.span.start).len(),
            ) {
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
            if equation.equation.is_local {
                let tokens = &model.expanded_tokens[equation.token_range.clone()];
                let identifier = tokens.get(1).filter(|token| token.kind == TokenKind::Ident);
                let equals = tokens.iter().position(|token| token.kind == TokenKind::Eq);
                if let (Some(identifier), Some(equals)) = (identifier, equals) {
                    let identifier_name = identifier.text(&model.source);
                    if text.get(identifier.span.start as usize..identifier.span.end as usize)
                        == Some(identifier_name)
                        && !intersects(&self.interpolations, identifier.span)
                    {
                        row.name = Some(identifier_name.to_owned());
                        row.selection = identifier.span;
                        if let (Some(first), Some(last)) = (
                            tokens.get(equals + 1),
                            tokens.last().filter(|_| equals + 1 < tokens.len()),
                        ) {
                            row.local_definition = Some(Span {
                                start: first.span.start,
                                end: last.span.end,
                            });
                        }
                    }
                }
            }
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
        self.native_proof.sort_by_key(|span| (span.start, span.end));
        self.native_proof.dedup();
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

    /// Only literal, locally paired verbatim text is certified here. The pin's
    /// INITIAL identifier/native choice depends on symbol state, so the inert
    /// parser's native rows cannot certify it across sibling branches.
    pub(crate) fn is_proven_native(&self, span: Span) -> bool {
        if !self.structure_complete {
            return false;
        }
        let after = self
            .native_proof
            .partition_point(|native| native.start <= span.start);
        after.checked_sub(1).is_some_and(|index| {
            let native = self.native_proof[index];
            span.start >= native.start && span.end <= native.end
        })
    }

    /// Written identifiers for conservative source edit proofs. The retained
    /// lexer excludes comments and quoted text. Interpolation placeholders,
    /// macro directives and proven verbatim bodies are not literal name sites.
    /// Unknown fragments and all assignments remain potentially relevant.
    pub(crate) fn literal_identifiers(&self) -> impl Iterator<Item = &Token> {
        self.lexical_tokens.iter().filter(|token| {
            token.kind == TokenKind::Ident
                && !intersects(&self.interpolations, token.span)
                && !intersects(&self.directive_spans, token.span)
                && !self.is_proven_native(token.span)
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
    use crate::engine::analysis_configuration::source_revision;

    fn inventory(text: &str) -> SourceInventory {
        let source = MacroSource::build(Arc::new(text.to_owned()), &mut MacroBudget::new())
            .expect("test source macro scan should succeed");
        SourceInventory::build_from_macro_source(source, Default::default(), None)
            .expect("test inventory should not be canceled")
    }

    #[test]
    fn written_inventory_reuses_one_macro_source_scan_and_guarded_sites() {
        // Guards source intelligence rebuilding token/site arrays instead of
        // sharing the macro source already used by configured execution.
        let text = Arc::new("@#define FLAG = 0\r\nvar y;\rmodel;\r\ny=0;\rend;\n".to_owned());
        let shared = MacroSource::build(text, &mut MacroBudget::new()).unwrap();
        assert!(!shared.sites.is_empty());
        let first =
            SourceInventory::build_from_macro_source(Arc::clone(&shared), Default::default(), None)
                .unwrap();
        let second =
            SourceInventory::build_from_macro_source(Arc::clone(&shared), Default::default(), None)
                .unwrap();
        assert!(Arc::ptr_eq(&first.macro_tokens, &shared.tokens));
        assert!(Arc::ptr_eq(&first.sites, &shared.sites));
        assert!(Arc::ptr_eq(&first.macro_tokens, &second.macro_tokens));
        assert!(Arc::ptr_eq(&first.sites, &second.sites));
        assert_eq!(first.source_revision, shared.source_revision);
        assert!(first.structure_complete());
        assert!(first.targets_complete());
        assert!(first.structure_cause().is_none());
        assert!(first.target_coverage_cause.is_none());
        assert_eq!(first.parse_passes, 1);
    }

    #[test]
    fn macro_target_failure_and_written_structure_failure_remain_independent() {
        // Guards a cached macro syntax refusal hiding complete written syntax,
        // and a later source work stop erasing the known target refusal.
        let text = Arc::new("@#define FLAG =\nvar y;\nmodel; y=log(y); end;\n".to_owned());
        let byte = text.find("log(y)").unwrap() as u32;
        let shared = MacroSource::build(text, &mut MacroBudget::new()).unwrap();
        assert!(shared.syntax_refusal.is_some());
        let complete =
            SourceInventory::build_from_macro_source(Arc::clone(&shared), Default::default(), None)
                .unwrap();
        assert!(complete.structure_complete());
        assert!(!complete.targets_complete());
        assert!(complete.structure_cause().is_none());
        assert!(complete.target_coverage_cause.is_some());
        assert_eq!(complete.context(byte).unwrap().language, "model_expression");
        let partial =
            SourceInventory::build_from_macro_source(shared, MacroWorkLimit::new(1).unwrap(), None)
                .unwrap();
        assert!(!partial.structure_complete());
        assert!(!partial.targets_complete());
        assert!(
            partial
                .structure_cause()
                .unwrap()
                .contains("work allowance")
        );
        assert_eq!(
            partial.target_coverage_cause,
            complete.target_coverage_cause
        );
        assert!(partial.context(byte).is_none());
    }

    #[test]
    fn edit_proof_keeps_assignments_and_unknown_fragments_from_sibling_source() {
        // Guards using the inert parser's generic opaque/native classification
        // to omit a possible name after a declaration from another macro arm.
        let text = "@#if FLAG\nparameters p; p=1;\n@#else\nhelper=p;\nmodel;\np=;\n@#endif\n";
        let source = inventory(text);
        let identifiers: HashSet<_> = source
            .literal_identifiers()
            .map(|token| token.span)
            .collect();
        for needle in ["p=1", "p;\nmodel", "p=;"] {
            let at = text.find(needle).unwrap();
            let span = Span::new(at, at + 1);
            assert!(identifiers.contains(&span), "missing literal {needle}");
            assert!(!source.is_proven_native(span));
        }
        assert!(source.context(text.find("p=;").unwrap() as u32).is_none());
    }

    #[test]
    fn edit_proof_excludes_only_literal_lexical_and_verbatim_boundaries() {
        // Guards treating strings/comments/interpolation masks as identifier
        // edit sites, or expanding verbatim proof to native-looking calls.
        let text = "parameters p;\n// p\ndisp('p');\nhelper=p;\nverbatim;\nhelper=p;\nend;\nvar p_@{region};\nmodel; p=0; end;\n";
        let source = inventory(text);
        let identifiers: HashSet<_> = source
            .literal_identifiers()
            .map(|token| token.span)
            .collect();
        let native_looking = text.find("helper=p;").unwrap() + "helper=".len();
        assert!(identifiers.contains(&Span::new(native_looking, native_looking + 1)));
        let verbatim = text.rfind("helper=p;").unwrap() + "helper=".len();
        let verbatim_span = Span::new(verbatim, verbatim + 1);
        assert!(source.is_proven_native(verbatim_span));
        assert!(!identifiers.contains(&verbatim_span));
        let comment = text.find("// p").unwrap() + 3;
        assert!(!identifiers.contains(&Span::new(comment, comment + 1)));
        let string = text.find("'p'").unwrap() + 1;
        assert!(!identifiers.contains(&Span::new(string, string + 1)));
        assert!(
            !source.literal_identifiers().any(|token| text
                [token.span.start as usize..token.span.end as usize]
                .contains("@{"))
        );
        let equation = text.find("p=0").unwrap();
        assert!(identifiers.contains(&Span::new(equation, equation + 1)));
    }

    #[test]
    fn conditional_candidates_keep_written_kinds_and_sibling_paths() {
        // Guards union parsing turning alternative declarations into one binding.
        let text = "@#if UNKNOWN\nvar y;\n@#else\nparameters y;\n@#endif\n";
        let source = inventory(text);
        // Presentation ranges must not shorten source inspection's conditional
        // owner or enlarge a branch directive into its body.
        let conditional = source
            .items
            .iter()
            .find(|row| row.kind == SourceItemKind::Conditional)
            .unwrap();
        assert_eq!(
            &text[conditional.span.start as usize..conditional.span.end as usize],
            text.trim_end()
        );
        let branch = source
            .items
            .iter()
            .find(|row| row.kind == SourceItemKind::Branch)
            .unwrap();
        assert_eq!(
            &text[branch.span.start as usize..branch.span.end as usize],
            "@#else"
        );
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
        let macro_source =
            MacroSource::build(Arc::new("var y;".to_owned()), &mut MacroBudget::new()).unwrap();
        let source = SourceInventory::build_from_macro_source(
            macro_source.clone(),
            MacroWorkLimit::new(1).unwrap(),
            None,
        )
        .unwrap();
        assert!(source.coverage_cause.is_some());
        assert!(source.items.is_empty());
        let flag = Arc::new(AtomicBool::new(true));
        assert!(
            SourceInventory::build_from_macro_source(macro_source, Default::default(), Some(flag))
                .is_none()
        );
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
