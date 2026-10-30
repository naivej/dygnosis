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
    pub source_namespace: Option<String>,
    pub path: Vec<SourcePathStep>,
    /// A proven containing written item, never an inferred semantic owner.
    pub parent: Option<usize>,
    pub tags: BTreeMap<String, String>,
    pub template: bool,
    /// End of the block opener or equation-tag prefix (empty when no tags).
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

/// Parser-completed literal name role; it is not an accepted binding.
#[derive(Clone, Debug)]
pub(crate) struct SourceNameSite {
    pub span: Span,
    pub kind: &'static str,
    pub declaration: bool,
    pub namespace: Option<String>,
    pub owner: Option<usize>,
}

impl SourceNameSite {
    pub(crate) fn final_table(&self) -> bool {
        matches!(
            self.kind,
            "var_model"
                | "trend_component_model"
                | "var_expectation_model"
                | "pac_model"
                | "auxiliary_model"
        )
    }
    pub(crate) fn matches(&self, declaration: &Self) -> bool {
        self.namespace == declaration.namespace
            && (self.kind == declaration.kind
                || self.kind == "auxiliary_model"
                    && matches!(declaration.kind, "var_model" | "trend_component_model")
                || self.kind == "callable" && declaration.kind == "external_function")
    }
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
    /// Grammar-completed names in other namespaces, retained from the same parse.
    nonordinary_names: Vec<Span>,
    /// Literal ordinary-name slots read by the existing command-list parser.
    command_uses: Vec<Span>,
    pub named_sites: Vec<SourceNameSite>,
}

struct MacroFrame<'a> {
    opener: &'a MacroDirective,
    parent: Vec<SourcePathStep>,
    branches: Vec<(&'a MacroDirective, usize)>,
    item: usize,
}

impl SourceInventory {
    /// Conservative retained payload for the bounded source-only hover cache.
    pub(crate) fn retained_bytes(&self) -> usize {
        use std::mem::size_of;
        let strings = |value: &Option<String>| value.as_ref().map_or(0, String::capacity);
        size_of::<Self>()
            + self.source_revision.capacity()
            + self.items.capacity() * size_of::<SourceItem>()
            + self
                .items
                .iter()
                .map(|row| {
                    row.source_id.capacity()
                        + strings(&row.name)
                        + strings(&row.outline_name)
                        + strings(&row.written_kind)
                        + strings(&row.source_namespace)
                        + row.path.capacity() * size_of::<SourcePathStep>()
                        + row.outline_children.capacity() * size_of::<SourceOutlineChild>()
                        + row
                            .outline_children
                            .iter()
                            .map(|child| child.name.capacity() + strings(&child.detail))
                            .sum::<usize>()
                        + row
                            .tags
                            .iter()
                            // Charge a full sparsely occupied B-tree node per entry.
                            .map(|(key, value)| {
                                key.capacity()
                                    + value.capacity()
                                    + 11 * size_of::<(String, String)>()
                                    + 128
                            })
                            .sum::<usize>()
                        + row.declaration_metadata.as_ref().map_or(0, |metadata| {
                            size_of::<SourceDeclarationMetadata>()
                                + strings(&metadata.long_name)
                                + strings(&metadata.tex_name)
                        })
                })
                .sum::<usize>()
            + self.lexical_tokens.capacity() * size_of::<Token>()
            + self
                .lexical_tokens
                .iter()
                .map(|token| strings(&token.lexeme))
                .sum::<usize>()
            + self.folds.capacity() * size_of::<SourceFold>()
            + strings(&self.coverage_cause)
            + strings(&self.target_coverage_cause)
            + (self.opaque.capacity()
                + self.native_proof.capacity()
                + self.interpolations.capacity()
                + self.directive_spans.capacity()
                + self.nonordinary_names.capacity()
                + self.command_uses.capacity())
                * size_of::<Span>()
            + self.contexts.capacity() * size_of::<(u32, Vec<SourcePathStep>)>()
            + self.named_sites.capacity() * size_of::<SourceNameSite>()
            + self
                .named_sites
                .iter()
                .map(|site| strings(&site.namespace))
                .sum::<usize>()
            + self
                .contexts
                .iter()
                .map(|(_, path)| path.capacity() * size_of::<SourcePathStep>())
                .sum::<usize>()
    }

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
            nonordinary_names: Vec::new(),
            command_uses: Vec::new(),
            named_sites: Vec::new(),
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
            source_namespace: None,
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
        let mut literal_dimensions = HashSet::new();
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
            if !self.reserve(budget, std::mem::size_of::<Span>()) {
                return;
            }
            self.nonordinary_names.push(slot.span);
            if statement_for_token(slot.order).is_some_and(|statement| proven.contains(&statement))
                && text.get(slot.span.start as usize..slot.span.end as usize)
                    == Some(model.name(slot.name))
                && self.literal_anchor_on_path(slot.span, slot.span.start)
            {
                use crate::engine::language::model::NamedSlotKind;
                let kind = match slot.kind {
                    NamedSlotKind::Dimension => "heterogeneity_dimension",
                    NamedSlotKind::Database => "database",
                    NamedSlotKind::Model(kind) => kind.source_kind(),
                    NamedSlotKind::AuxiliaryModel(_) => "auxiliary_model",
                };
                if !self.reserve(budget, std::mem::size_of::<SourceNameSite>()) {
                    return;
                }
                self.named_sites.push(SourceNameSite {
                    span: slot.span,
                    kind,
                    declaration: slot.declaration,
                    namespace: None,
                    owner: None,
                });
                if kind == "heterogeneity_dimension"
                    && !slot.declaration
                    && let Some(statement) = statement_for_token(slot.order)
                {
                    if !self.reserve(budget, 32) {
                        return;
                    }
                    literal_dimensions.insert(statement);
                }
                if kind == "database" && !slot.declaration {
                    let at = model
                        .expanded_tokens
                        .partition_point(|token| token.span.end <= slot.span.end);
                    if model
                        .expanded_tokens
                        .get(at)
                        .is_some_and(|token| token.kind == TokenKind::Dot)
                        && let Some(field) = model.expanded_tokens.get(at + 1).filter(|token| {
                            token.kind == TokenKind::Ident
                                && self.literal_anchor_on_path(token.span, token.span.start)
                        })
                    {
                        if !self.reserve(budget, std::mem::size_of::<SourceNameSite>()) {
                            return;
                        }
                        self.named_sites.push(SourceNameSite {
                            span: field.span,
                            kind: "database_field",
                            declaration: false,
                            namespace: None,
                            owner: None,
                        });
                    }
                }
            }
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
        for call in &model.function_calls {
            use crate::engine::language::expr::ExprKind;
            if let ExprKind::Call { callee, .. } = &model.exprs.get(call.expr).kind
                && statement_for_token(call.order)
                    .is_some_and(|statement| proven.contains(&statement))
                && !crate::engine::language::parser::is_dynare_expression_builtin(
                    model.name(*callee),
                )
                && text.get(call.span.start as usize..call.span.end as usize)
                    == Some(model.name(*callee))
                && self.literal_anchor_on_path(call.span, call.span.start)
            {
                if !self.reserve(budget, std::mem::size_of::<SourceNameSite>()) {
                    return;
                }
                self.named_sites.push(SourceNameSite {
                    span: call.span,
                    kind: "external_function",
                    declaration: false,
                    namespace: None,
                    owner: None,
                });
            }
            // The inert parser has no declarations from an unexpanded include.
            // Its generic one-integer call can be a variable lead/lag. A later
            // written declaration must establish that role before presentation.
            if let ExprKind::Call { callee, args } = &model.exprs.get(call.expr).kind
                && !crate::engine::language::parser::is_dynare_expression_builtin(
                    model.name(*callee),
                )
                && let [argument] = args.as_slice()
            {
                let argument = match model.exprs.get(*argument).kind {
                    ExprKind::Unary { arg, .. } => arg,
                    _ => *argument,
                };
                if matches!(model.exprs.get(argument).kind, ExprKind::Number)
                    && model
                        .numeric_literals
                        .get(&argument)
                        .is_some_and(|value| value.is_finite() && value.fract() == 0.0)
                {
                    continue;
                }
            }
            if !self.reserve(budget, std::mem::size_of::<Span>()) {
                return;
            }
            self.nonordinary_names.push(call.span);
        }
        for usage in &model.command_symbols {
            if !self.reserve(budget, std::mem::size_of::<Span>()) {
                return;
            }
            self.command_uses.push(usage.span);
        }
        for command in &model.semi_structural_commands {
            if !statement_for_token(command.parse_order)
                .is_some_and(|statement| proven.contains(&statement))
            {
                continue;
            }
            for option in &command.options {
                use crate::engine::language::model::SemiStructuralValue;
                if let SemiStructuralValue::Expression(expression) = &option.value
                    && let Some(expr) = expression.expr
                {
                    for name in model.exprs.walk_idents(expr) {
                        if !self.reserve(budget, std::mem::size_of::<Span>()) {
                            return;
                        }
                        self.command_uses.push(name.span);
                    }
                } else if let SemiStructuralValue::Symbol { span, .. } = option.value {
                    // Typed named slots are excluded by nonordinary_names above.
                    if !self.reserve(budget, std::mem::size_of::<Span>()) {
                        return;
                    }
                    self.command_uses.push(span);
                }
            }
        }
        for external in &model.external_functions {
            if !statement_for_token(external.parse_order)
                .is_some_and(|statement| proven.contains(&statement))
            {
                continue;
            }
            if let Some((name, span)) = external.name
                && text
                    .get(span.start as usize..span.end as usize)
                    .map(|name| name.trim_matches(['\'', '"']))
                    == Some(model.name(name))
                && self.literal_anchor_on_path(span, span.start)
            {
                if !self.reserve(budget, std::mem::size_of::<SourceNameSite>()) {
                    return;
                }
                self.named_sites.push(SourceNameSite {
                    span,
                    kind: "external_function",
                    declaration: true,
                    namespace: None,
                    owner: None,
                });
            }
            for derivative in [external.first_deriv, external.second_deriv]
                .into_iter()
                .flatten()
            {
                if let crate::engine::language::model::DerivSpec::Named(name, span) = derivative
                    && text
                        .get(span.start as usize..span.end as usize)
                        .map(|name| name.trim_matches(['\'', '"']))
                        == Some(model.name(name))
                    && self.literal_anchor_on_path(span, span.start)
                {
                    if !self.reserve(budget, std::mem::size_of::<SourceNameSite>()) {
                        return;
                    }
                    self.named_sites.push(SourceNameSite {
                        span,
                        kind: "external_function",
                        declaration: true,
                        namespace: None,
                        owner: None,
                    });
                }
            }
        }
        for expr in model.trend_vars.iter().filter_map(|row| row.growth).chain(
            model
                .nonstationary_vars
                .iter()
                .filter_map(|row| row.deflator),
        ) {
            for name in model.exprs.walk_idents(expr) {
                if !self.reserve(budget, std::mem::size_of::<Span>()) {
                    return;
                }
                self.command_uses.push(name.span);
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
            if let crate::engine::language::model::SubsampleInstruction::Copy { target, .. } =
                instruction
            {
                use crate::engine::language::model::SubsampleHead;
                let (names, span) = match target {
                    SubsampleHead::Symbol(name, span) | SubsampleHead::Std(name, span) => {
                        (vec![model.name(*name)], *span)
                    }
                    SubsampleHead::Corr(first, span, second, _) => {
                        let mut names = vec![model.name(*first), model.name(*second)];
                        names.sort_unstable();
                        (names, *span)
                    }
                };
                let namespace = format!("subsamples({})", names.join(","));
                if !self.reserve(
                    budget,
                    std::mem::size_of::<SourceNameSite>() + namespace.len(),
                ) {
                    return;
                }
                self.named_sites.push(SourceNameSite {
                    span,
                    kind: "subsample_copy",
                    declaration: false,
                    namespace: Some(namespace),
                    owner: None,
                });
            }
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
            let reset_span = match instruction {
                crate::engine::language::model::SubsampleInstruction::Declare { head, .. } => {
                    match head {
                        crate::engine::language::model::SubsampleHead::Symbol(_, span)
                        | crate::engine::language::model::SubsampleHead::Std(_, span)
                        | crate::engine::language::model::SubsampleHead::Corr(_, span, _, _) => {
                            *span
                        }
                    }
                }
                _ => unreachable!(),
            };
            let mut reset_key: Vec<_> = head.split(',').collect();
            reset_key.sort_unstable();
            let reset_namespace = format!("subsamples({})", reset_key.join(","));
            if !self.reserve(
                budget,
                std::mem::size_of::<SourceNameSite>() + reset_namespace.len(),
            ) {
                return;
            }
            self.named_sites.push(SourceNameSite {
                span: reset_span,
                kind: "subsample_reset",
                declaration: false,
                namespace: Some(reset_namespace),
                owner: None,
            });
            for range in ranges {
                let name = model.name(range.name);
                if text.get(range.name_span.start as usize..range.name_span.end as usize)
                    != Some(name)
                    || intersects(&self.interpolations, range.name_span)
                {
                    continue;
                }
                let mut key: Vec<_> = head.split(',').collect();
                key.sort_unstable();
                let namespace = format!("subsamples({})", key.join(","));
                if !self.reserve(
                    budget,
                    std::mem::size_of::<SourceNameSite>() + namespace.len(),
                ) {
                    return;
                }
                self.named_sites.push(SourceNameSite {
                    span: range.name_span,
                    kind: "named_subsample",
                    declaration: true,
                    namespace: Some(namespace),
                    owner: None,
                });
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
        for usage in &model.subsample_uses {
            use crate::engine::language::model::DottedHead;
            if usage.checked {
                let spans: Vec<_> = match &usage.head {
                    DottedHead::Param { first, .. } => model
                        .dotted_statements
                        .get(usage.statement)
                        .and_then(|statement| {
                            model.expanded_tokens.get(statement.active_tokens.start)
                        })
                        .filter(|token| token.text(&model.source) == model.name(*first))
                        .map(|token| vec![token.span])
                        .unwrap_or_default(),
                    DottedHead::Std { first_span, .. } => vec![*first_span],
                    DottedHead::Corr {
                        first_span,
                        second_span,
                        ..
                    } => vec![*first_span, *second_span],
                    DottedHead::Vec { .. } => Vec::new(),
                };
                for span in spans {
                    if !self.reserve(budget, std::mem::size_of::<Span>()) {
                        return;
                    }
                    self.command_uses.push(span);
                }
            }
            let names = match &usage.head {
                DottedHead::Param { first, .. } | DottedHead::Std { first, .. } => {
                    vec![model.name(*first)]
                }
                DottedHead::Corr { first, second, .. } => {
                    let mut names = vec![model.name(*first), model.name(*second)];
                    names.sort_unstable();
                    names
                }
                DottedHead::Vec { .. } => continue,
            };
            let namespace = format!("subsamples({})", names.join(","));
            if !self.literal_anchor_on_path(usage.span, usage.span.start)
                || text.get(usage.span.start as usize..usage.span.end as usize)
                    != Some(model.name(usage.name))
            {
                continue;
            }
            if !self.reserve(
                budget,
                std::mem::size_of::<SourceNameSite>() + namespace.len(),
            ) {
                return;
            }
            self.named_sites.push(SourceNameSite {
                span: usage.span,
                kind: "named_subsample",
                declaration: false,
                namespace: Some(namespace),
                owner: None,
            });
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
            row.header_end = tag_end;
            let scope = match equation.dimension {
                None => Some("model".to_owned()),
                Some(dimension) if literal_dimensions.contains(&equation.statement_id) => {
                    Some(format!("model(heterogeneity={})", model.name(dimension)))
                }
                Some(_) => None,
            };
            if !self.reserve(budget, scope.as_ref().map_or(0, String::len)) {
                return;
            }
            row.source_namespace = scope;
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
            if row.local_definition.is_some() && complete {
                let namespace = row.source_namespace.clone();
                if !self.reserve(
                    budget,
                    std::mem::size_of::<SourceNameSite>()
                        + namespace.as_ref().map_or(0, String::len),
                ) {
                    return;
                }
                self.named_sites.push(SourceNameSite {
                    span: row.selection,
                    kind: "model_local_variable",
                    declaration: true,
                    namespace,
                    owner: None,
                });
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
        self.nonordinary_names
            .sort_by_key(|span| (span.start, span.end));
        self.nonordinary_names.dedup();
        self.command_uses.sort_by_key(|span| (span.start, span.end));
        self.command_uses.dedup();
        self.named_sites
            .sort_by_key(|site| (site.span.start, site.span.end));
        for site in &mut self.named_sites {
            if crate::engine::control::cancelled() {
                return;
            }
            let at = self
                .items
                .partition_point(|item| item.span.start <= site.span.start);
            site.owner = self.items[..at].iter().rposition(|item| {
                item.kind != SourceItemKind::Fragment && site.span.end <= item.span.end
            });
        }
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

    pub(crate) fn name_use(
        &self,
        item: &SourceItem,
        byte: u32,
    ) -> Option<(Span, Option<SourceNameSite>)> {
        let at = self
            .named_sites
            .partition_point(|site| site.span.end <= byte);
        let named = self.named_sites.get(at).filter(|site| {
            !matches!(
                site.kind,
                "database_field" | "subsample_copy" | "subsample_reset"
            ) && site.owner.is_some()
                && site.span.start <= byte
                && byte < site.span.end
                && self.literal_anchor_on_path(site.span, byte)
        });
        if let Some(span) = self.ordinary_name_use(item, byte) {
            let role = named
                .filter(|site| !site.declaration && site.kind == "external_function")
                .map(|site| {
                    let mut site = site.clone();
                    site.kind = "callable";
                    site
                });
            return Some((span, role));
        }
        self.structure_complete
            .then_some(named)
            .flatten()
            .map(|site| (site.span, Some(site.clone())))
    }

    pub(crate) fn named_declaration(
        &self,
        text: &str,
        site: &SourceNameSite,
    ) -> Option<SourceItem> {
        if !site.declaration {
            return None;
        }
        if site.kind == "model_local_variable" {
            let mut item = self.items.get(site.owner?)?.clone();
            item.kind = SourceItemKind::Declaration;
            item.written_kind = Some(site.kind.into());
            return Some(item);
        }
        let index = site.owner?;
        let owner = self.items.get(index)?;
        if owner.kind == SourceItemKind::Declaration
            && owner.span == site.span
            && owner.selection == site.span
            && owner.written_kind.as_deref() == Some(site.kind)
        {
            return Some(owner.clone());
        }
        Some(SourceItem {
            source_id: format!("{}:name:{}", owner.source_id, site.span.start),
            kind: SourceItemKind::Declaration,
            span: site.span,
            selection: site.span,
            header_end: site.span.end,
            branch_end: None,
            name: Some(
                text.get(site.span.start as usize..site.span.end as usize)?
                    .trim_matches(['\'', '"'])
                    .to_owned(),
            ),
            written_kind: Some(site.kind.to_owned()),
            parent: Some(index),
            source_namespace: site.namespace.clone(),
            outline_name: None,
            outline_selection: None,
            outline_children: Vec::new(),
            local_definition: None,
            declaration_metadata: None,
            tags: BTreeMap::new(),
            template: false,
            path: self.path_at(site.span.start).to_vec(),
            function: site.kind == "external_function",
        })
    }

    /// A literal ordinary-name use in an equation or parser-proven command-list
    /// token. This establishes no accepted binding, model value or timing.
    pub(crate) fn ordinary_name_use(&self, item: &SourceItem, byte: u32) -> Option<Span> {
        if !self.structure_complete || item.template {
            return None;
        }
        let tokens = &self.lexical_tokens;
        let at = tokens.partition_point(|token| token.span.end <= byte);
        let token = tokens.get(at).filter(|token| {
            token.kind == TokenKind::Ident
                && token.span.start <= byte
                && byte < token.span.end
                && self.literal_anchor_on_path(token.span, byte)
        })?;
        let equation = item.kind == SourceItemKind::Equation;
        let command_use = self
            .command_uses
            .binary_search_by_key(&token.span.start, |span| span.start)
            .is_ok();
        if equation && item.source_namespace.is_none() {
            return None;
        }
        if !equation
            && !(matches!(
                item.kind,
                SourceItemKind::Command | SourceItemKind::Declaration | SourceItemKind::Block
            ) && command_use)
        {
            return None;
        }
        if self
            .nonordinary_names
            .binary_search_by_key(&token.span.start, |span| span.start)
            .is_ok()
        {
            return None;
        }
        if !command_use
            && (at
                .checked_sub(1)
                .and_then(|at| tokens.get(at))
                .is_some_and(|token| token.kind == TokenKind::Dot)
                || tokens
                    .get(at + 1)
                    .is_some_and(|token| token.kind == TokenKind::Dot))
        {
            return None;
        }
        // The existing tag walk retains the prefix bound; tags are not
        // ordinary-name uses and need no second prefix-token walk here.
        if equation && token.span.start < item.header_end
            || item
                .local_definition
                .is_some_and(|rhs| token.span.start < rhs.start)
        {
            return None;
        }
        let block = |row: &SourceItem| {
            let mut parent = row.parent;
            while let Some(index) = parent {
                let row = &self.items[index];
                if row.kind == SourceItemKind::Block {
                    return Some(index);
                }
                parent = row.parent;
            }
            None
        };
        if equation
            && !block(item).is_some_and(|owner| {
                matches!(
                    self.items[owner].name.as_deref(),
                    Some("model" | "model_replace")
                )
            })
        {
            return None;
        }
        Some(token.span)
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
