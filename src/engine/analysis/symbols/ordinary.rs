//! Accepted ordinary name slots and complete Rename decisions.
use super::*;

pub(super) fn ordinary(kind: &str) -> bool {
    matches!(kind, "var" | "varexo" | "varexo_det" | "parameters")
}

fn renameable(kind: &str) -> bool {
    ordinary(kind) || matches!(kind, "trend_var" | "log_trend_var")
}

/// Dynare statements own their execution tokens. Native blocks and tokens
/// outside those statements have no Dynare symbol binding.
pub(super) fn native_orders(model: &Model) -> HashSet<usize> {
    let mut native: HashSet<_> = (0..model.expanded_tokens.len())
        .take_while(|_| !crate::engine::control::cancelled())
        .collect();
    for statement in &model.statements {
        if crate::engine::control::cancelled() {
            break;
        }
        if !statement.native && statement.name != "verbatim" {
            for order in statement.token_range.clone() {
                if crate::engine::control::cancelled() {
                    break;
                }
                native.remove(&order);
            }
        }
    }
    native
}

/// Preserve the execution proof when accepted sites share written macro spans.
pub(super) fn excluded_nonordinary_orders(model: &Model) -> HashSet<usize> {
    let callbacks = crate::engine::analysis::named_callbacks::collect(model);
    let mut excluded: HashSet<_> = model
        .dotted_statements
        .iter()
        .enumerate()
        .filter(|(index, _)| !callbacks.dotted.contains(index))
        .flat_map(|(_, statement)| statement.active_tokens.clone())
        .collect();
    excluded.extend(model.named_slots.iter().map(|slot| slot.order));
    let commands = crate::engine::diagnostics::check_d_pac::named_callbacks(model);
    for command in &model.semi_structural_commands {
        for option in &command.options {
            if !matches!(
                option.value,
                crate::engine::language::model::SemiStructuralValue::Symbol { .. }
            ) {
                continue;
            }
            let ordinary = command.kind
                == crate::engine::language::model::SemiStructuralKind::VarExpectationModel
                && option.name.eq_ignore_ascii_case("variable")
                && commands.variable_lookups.contains(&command.parse_order)
                || command.kind == crate::engine::language::model::SemiStructuralKind::PacModel
                    && option.name.eq_ignore_ascii_case("discount")
                    && command.callback_completed;
            if !ordinary {
                excluded.insert(option.callback_order.saturating_sub(1));
            }
        }
    }
    excluded
}

/// Parser-owned name slots. An equal spelling in a file/function/label slot
/// is not evidence that the token refers to the selected ordinary symbol.
pub(super) fn binding_sites(model: &Model, name: &str) -> HashSet<Span> {
    let Some(name) = model.intern.lookup(name) else {
        return HashSet::new();
    };
    binding_site_inventory(model)
        .remove(&name)
        .unwrap_or_default()
}

pub(super) fn binding_site_inventory(model: &Model) -> HashMap<Name, HashSet<Span>> {
    use crate::engine::language::model::{
        DottedHead, PathTarget, SemiStructuralKind, SemiStructuralValue, ShockKind,
    };
    let mut sites = HashMap::<Name, HashSet<Span>>::new();
    let mut add = |symbol, span| {
        sites.entry(symbol).or_default().insert(span);
    };
    for event in &model.symbol_type_events {
        if crate::engine::control::cancelled() {
            break;
        }
        add(event.name, event.span);
    }
    for declaration in &model.predetermined {
        if crate::engine::control::cancelled() {
            break;
        }
        add(declaration.name, declaration.span);
    }
    for write in &model.write_targets {
        if crate::engine::control::cancelled() {
            break;
        }
        for token in &model.expanded_tokens[write.token_range.clone()] {
            if crate::engine::control::cancelled() {
                break;
            }
            if token.kind == TokenKind::Ident && token.text(&model.source) == model.name(write.name)
            {
                add(write.name, token.span);
            }
        }
    }
    for usage in &model.model_expression_uses {
        if crate::engine::control::cancelled() {
            break;
        }
        add(usage.name, usage.span);
    }
    for (symbol, span, _) in model
        .outside_expression_uses
        .iter()
        .chain(&model.steady_state_rhs_uses)
    {
        add(*symbol, *span);
    }
    for row in &model.command_symbols {
        if crate::engine::control::cancelled() {
            break;
        }
        add(row.name, row.span);
    }
    let model_callbacks = crate::engine::diagnostics::check_d_pac::named_callbacks(model);
    for command in &model.semi_structural_commands {
        if crate::engine::control::cancelled() {
            break;
        }
        if command.kind != SemiStructuralKind::PacModel
            && !model_callbacks
                .variable_lookups
                .contains(&command.parse_order)
        {
            continue;
        }
        for option in &command.options {
            if crate::engine::control::cancelled() {
                break;
            }
            if (command.kind == SemiStructuralKind::PacModel
                && option.name.eq_ignore_ascii_case("discount")
                || command.kind == SemiStructuralKind::VarExpectationModel
                    && option.name.eq_ignore_ascii_case("variable"))
                && let SemiStructuralValue::Symbol { name: symbol, span } = &option.value
            {
                add(*symbol, *span);
            }
        }
    }
    for row in model.varobs.iter().chain(&model.varexobs) {
        if crate::engine::control::cancelled() {
            break;
        }
        add(row.name, row.span);
    }
    for row in &model.instrument_uses {
        if crate::engine::control::cancelled() {
            break;
        }
        add(row.name, row.span);
    }
    for row in &model.irf_shocks_options {
        if crate::engine::control::cancelled() {
            break;
        }
        for (symbol, span) in &row.names {
            if crate::engine::control::cancelled() {
                break;
            }
            add(*symbol, *span);
        }
    }
    for row in &model.change_type_statements {
        if crate::engine::control::cancelled() {
            break;
        }
        for (symbol, span) in &row.names {
            if crate::engine::control::cancelled() {
                break;
            }
            add(*symbol, *span);
        }
    }
    for row in &model.var_removed {
        if crate::engine::control::cancelled() {
            break;
        }
        add(row.name, row.name_span);
    }
    for row in model
        .estimated_params
        .iter()
        .chain(&model.estimated_params_init)
        .chain(&model.estimated_params_bounds)
        .chain(&model.estimated_params_remove)
    {
        add(row.name, row.name_span);
        if let (Some(symbol), Some(span)) = (row.corr_with, row.corr_with_span) {
            add(symbol, span);
        }
    }
    for (symbol, span) in &model.observation_trends {
        if crate::engine::control::cancelled() {
            break;
        }
        add(*symbol, *span);
    }
    for block in &model.deterministic_trends {
        for row in &block.rows {
            add(row.name, row.name_span);
        }
    }
    for block in &model.conditional_forecast_paths {
        for row in &block.rows {
            if row.parse_order.is_some()
                && row.value_expressions.iter().all(|value| value.completed)
            {
                add(row.name, row.name_span);
            }
        }
    }
    for block in &model.moment_calibration {
        for row in &block.rows {
            add(row.first, row.first_span);
            add(row.second, row.second_span);
        }
    }
    for block in &model.irf_calibration {
        for row in &block.rows {
            add(row.endogenous, row.endogenous_span);
            add(row.exogenous, row.exogenous_span);
        }
    }
    for row in &model.optim_weights {
        if crate::engine::control::cancelled() {
            break;
        }
        add(row.first, row.first_span);
        if let (Some(symbol), Some(span)) = (row.second, row.second_span) {
            add(symbol, span);
        }
    }
    for row in &model.init2shocks_blocks {
        if crate::engine::control::cancelled() {
            break;
        }
        for target in &row.rows {
            if crate::engine::control::cancelled() {
                break;
            }
            add(target.endo, target.endo_span);
            add(target.exo, target.exo_span);
        }
    }
    for row in &model.shock_groups {
        if crate::engine::control::cancelled() {
            break;
        }
        for (symbol, span) in &row.members {
            if crate::engine::control::cancelled() {
                break;
            }
            add(*symbol, *span);
        }
    }
    for row in &model.generate_irfs {
        if crate::engine::control::cancelled() {
            break;
        }
        for (symbol, span) in &row.exos {
            if crate::engine::control::cancelled() {
                break;
            }
            add(*symbol, *span);
        }
    }
    for row in &model.shock_blocks {
        if crate::engine::control::cancelled() {
            break;
        }
        for target in &row.scheduled {
            if crate::engine::control::cancelled() {
                break;
            }
            add(target.name, target.name_span);
        }
        for target in &row.stochastic {
            if crate::engine::control::cancelled() {
                break;
            }
            let symbols = match &target.kind {
                ShockKind::Var(symbol) | ShockKind::Stderr(symbol) => vec![*symbol],
                ShockKind::Cov(symbols) | ShockKind::Skew(symbols) => symbols.clone(),
                ShockKind::Corr { a, b } => vec![*a, *b],
            };
            for token in model.expanded_tokens[target.parse_order..]
                .iter()
                .take_while(|token| !matches!(token.kind, TokenKind::Eq | TokenKind::Semi))
            {
                if token.kind == TokenKind::Ident
                    && let Some(symbol) = symbols
                        .iter()
                        .copied()
                        .find(|symbol| model.name(*symbol) == token.text(&model.source))
                {
                    add(symbol, token.span);
                }
            }
        }
    }
    for row in model.shock_paths.iter().chain(&model.controlled_paths) {
        if crate::engine::control::cancelled() {
            break;
        }
        for stanza in &row.stanzas {
            if crate::engine::control::cancelled() {
                break;
            }
            match stanza.target {
                PathTarget::Exogenous { name: symbol, span } => add(symbol, span),
                PathTarget::Controlled {
                    exogenize,
                    exogenize_span,
                    endogenize,
                    endogenize_span,
                } => {
                    add(exogenize, exogenize_span);
                    add(endogenize, endogenize_span);
                }
            }
            for value in &stanza.values {
                if crate::engine::control::cancelled() {
                    break;
                }
                for reference in &value.path_refs {
                    if crate::engine::control::cancelled() {
                        break;
                    }
                    if model
                        .symbol_kind_in_context(reference.name, reference.symbol_type_context)
                        .is_some()
                    {
                        add(reference.name, reference.ident_span);
                    }
                }
            }
        }
    }
    let token_spans: HashMap<_, _> = model
        .expanded_tokens
        .iter()
        .take_while(|_| !crate::engine::control::cancelled())
        .map(|token| (token.span.start, token.span))
        .collect();
    for (symbol, start) in model
        .homotopy_rows
        .iter()
        .map(|row| (row.name, row.span.start))
        .chain(
            model
                .osr_params_bounds
                .iter()
                .map(|row| (row.name, row.span.start)),
        )
    {
        if let Some(span) = token_spans.get(&start) {
            add(symbol, *span);
        }
    }
    let callbacks = crate::engine::analysis::named_callbacks::collect(model);
    for (_, row) in model
        .dotted_statements
        .iter()
        .enumerate()
        .filter(|(index, _)| callbacks.dotted.contains(index))
    {
        if crate::engine::control::cancelled() {
            break;
        }
        let target_start = model
            .expanded_tokens
            .get(row.active_tokens.start)
            .map(|token| token.span);
        // Both heads are read from this exact accepted parser execution.
        let source_start = model
            .expanded_tokens
            .get(row.active_tokens.clone())
            .and_then(|tokens| {
                tokens
                    .iter()
                    .position(|token| token.kind == TokenKind::Eq)
                    .and_then(|index| tokens.get(index + 1))
            })
            .map(|token| token.span);
        for (head, start) in std::iter::once((&row.head, target_start))
            .chain(row.copy_source.as_ref().map(|head| (head, source_start)))
        {
            match head {
                DottedHead::Param { first, .. } => {
                    if let Some(span) = start {
                        add(*first, span);
                    }
                }
                DottedHead::Std {
                    first, first_span, ..
                } => add(*first, *first_span),
                DottedHead::Corr {
                    first,
                    first_span,
                    second,
                    second_span,
                    ..
                } => {
                    add(*first, *first_span);
                    add(*second, *second_span);
                }
                DottedHead::Vec { names } => {
                    for (symbol, span) in names {
                        if crate::engine::control::cancelled() {
                            break;
                        }
                        add(*symbol, *span);
                    }
                }
            }
        }
    }
    sites
}

pub(super) struct Decision {
    pub declaration: (String, Span),
    pub kind: &'static str,
    pub targets: Vec<(String, Span)>,
}

pub(super) fn decision(
    view: &SemanticModel,
    name: &str,
    replacement: &str,
) -> Result<Decision, Refusal> {
    check_cancelled()?;
    if !view.complete {
        return Err(INCOMPLETE);
    }
    let model = &view.model;
    let tags = &view.tags;
    let (declaration, kind) = declaration(view, name)?;
    let event = model
        .symbol_type_events
        .iter()
        .find(|event| model.name(event.name) == name);
    // A retype through a model-local binding requires that binding's guards.
    if kind == "external_function"
        || event.is_some_and(|event| event.kind.as_str() == "external_function")
    {
        return Err(Refusal::new(
            "EXTERNAL_IMPLEMENTATION",
            "An external function name also identifies its MATLAB implementation. Rename cannot update that implementation and its callers outside this model; no edits were proposed.",
        ));
    }
    if kind == "database_variable" {
        return Err(Refusal::new(
            "EXTERNAL_DATABASE_MEMBER",
            "This name identifies a member of an external database. Rename cannot update that database's fields; no edits were proposed.",
        ));
    }
    if !is_legal_ident(name) || !is_legal_ident(replacement) {
        return Err(IDENTIFIER);
    }
    if !renameable(kind) || event.is_some_and(|event| !renameable(event.kind.as_str())) {
        return Err(BINDING);
    }
    if model
        .shock_paths
        .iter()
        .chain(&model.controlled_paths)
        .flat_map(|block| &block.stanzas)
        .flat_map(|stanza| &stanza.values)
        .filter(|value| value.completed)
        .flat_map(|value| &value.path_refs)
        .any(|reference| {
            !reference.call
                && model.name(reference.name) == name
                && reference.namespace.as_ref().is_some_and(|namespace| {
                    !matches!(
                        namespace.as_str(),
                        "init" | "initval" | "self" | "prev" | "learnt_in"
                    ) && model.databases.iter().any(|database| {
                        database
                            .names
                            .iter()
                            .any(|(database, _)| model.name(*database) == namespace)
                    })
                })
        })
    {
        return Err(Refusal::new(
            "EXTERNAL_DATABASE_MEMBER",
            "This symbol is also used as an explicit field of an external database. Rename cannot update that database's column; no edits were proposed.",
        ));
    }
    if name != replacement
        && model
            .symbol_type_events
            .iter()
            .any(|event| model.name(event.name) == replacement)
        || name != replacement
            && tags
                .iter()
                .any(|tag| tag.declaration && tag.name == replacement)
    {
        return Err(COLLISION);
    }
    if name != replacement && !binding_sites(model, replacement).is_empty() {
        return Err(BINDING_CAPTURE);
    }
    let mut targets = Vec::new();
    let mut target_set = HashSet::new();
    let mut mappings = HashMap::new();
    let binding_sites = binding_sites(model, name);
    let native = native_orders(model);
    let mut eligible = vec![false; model.expanded_tokens.len()];
    for statement in &model.statements {
        if crate::engine::control::cancelled() {
            break;
        }
        if statement.native {
            continue;
        }
        let mut depth = 0usize;
        for order in statement.token_range.clone() {
            if crate::engine::control::cancelled() {
                break;
            }
            let token = model.expanded_tokens.get(order).ok_or(MAPPING)?;
            eligible[order] = !(depth > 0
                && model
                    .expanded_tokens
                    .get(order + 1)
                    .is_some_and(|next| next.kind == TokenKind::Eq)
                || order > 0 && model.expanded_tokens[order - 1].kind == TokenKind::Dot);
            match token.kind {
                TokenKind::LParen | TokenKind::LBrack => depth += 1,
                TokenKind::RParen | TokenKind::RBrack => depth = depth.saturating_sub(1),
                _ => {}
            }
        }
    }
    let mut renamed_tokens = model.expanded_tokens.clone();
    for (order, token) in model.expanded_tokens.iter().enumerate() {
        if crate::engine::control::cancelled() {
            break;
        }
        if token.kind != TokenKind::Ident || token.text(&model.source) != name {
            continue;
        }
        if native.contains(&order) {
            continue;
        }
        if !binding_sites.contains(&token.span)
            && view.named.iter().any(|fact| {
                fact.declarations
                    .iter()
                    .chain(&fact.uses)
                    .any(|site| site.order == order)
            })
        {
            continue;
        }
        // Native MATLAB text has no Dynare binding. It is not a rename target.
        if !eligible[order] && !binding_sites.contains(&token.span) {
            continue;
        }
        if !binding_sites.contains(&token.span) {
            // The parser did not establish an ordinary binding at this slot.
            // Withhold the operation instead of guessing or returning a
            // partial set of known uses.
            return Err(BINDING);
        }
        let target = mappings
            .entry(token.span)
            .or_insert_with(|| view.written(token.span))
            .clone()
            .ok_or(MAPPING)?;
        view.literal_identifier(&target, name)?;
        if target_set.insert(target.clone()) {
            targets.push(target);
        }
        renamed_tokens[order].lexeme = Some(replacement.to_string());
    }
    for tag in tags.iter().filter(|tag| tag.name == name) {
        if crate::engine::control::cancelled() {
            break;
        }
        let target = tag.written.clone().ok_or(MAPPING)?;
        if target_set.insert(target.clone()) {
            targets.push(target);
        }
    }
    tags::rewrite(model, &mut renamed_tokens, tags, name, replacement).ok_or(MAPPING)?;
    if !target_set.contains(&declaration) {
        return Err(MAPPING);
    }
    // A literal macro site may execute as a different name or as native text
    // in another copy. Changing it would change a binding outside this edit.
    for (order, token) in model.expanded_tokens.iter().enumerate() {
        if crate::engine::control::cancelled() {
            break;
        }
        let Some(target) = mappings
            .entry(token.span)
            .or_insert_with(|| view.written(token.span))
            .as_ref()
        else {
            continue;
        };
        if target_set.contains(target)
            && !tags
                .iter()
                .any(|tag| tag.order == Some(order) && tag.written.as_ref() == Some(target))
            && (token.kind != TokenKind::Ident
                || token.text(&model.source) != name
                || renamed_tokens[order].text(&model.source) != replacement
                || native.contains(&order)
                || !binding_sites.contains(&token.span))
        {
            return Err(BINDING);
        }
    }
    if name != replacement {
        check_cancelled()?;
        let (renamed, _) = crate::engine::language::parser::parse_expanded_context_controlled(
            &model.source,
            renamed_tokens,
            None,
            model.macro_work_limit,
            model.analysis_context.clone(),
            model.macro_discarded.clone(),
        );
        check_cancelled()?;
        if !crate::engine::workspace::model_map::parser_complete(&renamed)
            || model.statements.len() != renamed.statements.len()
            || model
                .statements
                .iter()
                .zip(&renamed.statements)
                .any(|(before, after)| {
                    before.kind != after.kind
                        || before.native != after.native
                        || before.token_range != after.token_range
                })
            || model.symbol_type_events.len() != renamed.symbol_type_events.len()
            || model
                .symbol_type_events
                .iter()
                .zip(&renamed.symbol_type_events)
                .any(|(before, after)| {
                    let expected = if model.name(before.name) == name {
                        replacement
                    } else {
                        model.name(before.name)
                    };
                    renamed.name(after.name) != expected
                        || before.kind != after.kind
                        || before.changed != after.changed
                })
        {
            return Err(BINDING);
        }
    }
    check_cancelled()?;
    Ok(Decision {
        declaration,
        kind,
        targets,
    })
}

pub(super) fn declaration(
    view: &SemanticModel,
    name: &str,
) -> Result<((String, Span), &'static str), Refusal> {
    view.ordinary_declarations()
        .ok_or(CANCELLED)?
        .declaration(view, name)
}

struct Events {
    first: usize,
    last: usize,
    accepted_kind: Option<crate::engine::language::model::SymbolKind>,
}

/// One snapshot's declaration lookup and accepted final kinds. Event order,
/// rather than source position, distinguishes repeated macro executions.
pub(super) struct DeclarationFacts {
    events: HashMap<Name, Events>,
    orders: Vec<Option<usize>>,
    tags: HashMap<String, (usize, usize)>,
    dimensions: HashMap<Name, Option<Name>>,
}

#[cfg(test)]
thread_local! {
    pub(super) static DECLARATION_EVENT_VISITS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

impl DeclarationFacts {
    pub(super) fn collect(view: &SemanticModel) -> Self {
        let mut events = HashMap::<Name, Events>::new();
        for (index, event) in view.model.symbol_type_events.iter().enumerate() {
            if crate::engine::control::cancelled() {
                break;
            }
            #[cfg(test)]
            DECLARATION_EVENT_VISITS.with(|count| count.set(count.get() + 1));
            let facts = events.entry(event.name).or_insert(Events {
                first: index,
                last: index,
                accepted_kind: None,
            });
            facts.last = index;
            if view.accepted_events.contains(&index) {
                facts.accepted_kind = Some(event.kind);
            }
        }
        let mut orders = vec![None; view.model.symbol_type_events.len()];
        for (event, range) in &view.model.type_event_occurrences {
            if crate::engine::control::cancelled() {
                break;
            }
            if let Some(order) = orders.get_mut(*event) {
                order.get_or_insert(range.end);
            }
        }
        let mut tags = HashMap::<String, (usize, usize)>::new();
        for (index, tag) in view.tags.iter().enumerate() {
            if crate::engine::control::cancelled() {
                break;
            }
            if tag.declaration {
                let entry = tags
                    .entry(tag.name.clone())
                    .or_insert((index, tag.declaration_order));
                entry.1 = entry.1.max(tag.declaration_order);
            }
        }
        let mut dimensions = HashMap::new();
        for row in &view.model.written_declarations {
            if crate::engine::control::cancelled() {
                break;
            }
            dimensions
                .entry(row.declaration.name)
                .or_insert_with(|| row.declaration.heterogeneity.map(|(name, _)| name));
        }
        Self {
            events,
            orders,
            tags,
            dimensions,
        }
    }

    pub(super) fn accepted_kind(
        &self,
        name: Name,
    ) -> Option<crate::engine::language::model::SymbolKind> {
        self.events.get(&name)?.accepted_kind
    }

    pub(super) fn dimension(&self, name: Name) -> Option<Name> {
        self.dimensions.get(&name).copied().flatten()
    }

    pub(super) fn declaration(
        &self,
        view: &SemanticModel,
        name: &str,
    ) -> Result<((String, Span), &'static str), Refusal> {
        let events = view
            .model
            .intern
            .lookup(name)
            .and_then(|name| self.events.get(&name));
        let first =
            events.map(|events| (events.first, &view.model.symbol_type_events[events.first]));
        let tag = self.tags.get(name).map(|(first, _)| &view.tags[*first]);
        let declaration = match (first, tag) {
            (Some((index, _)), Some(tag))
                if tag.declaration_order < self.orders[index].ok_or(MAPPING)? =>
            {
                tag.written.clone().ok_or(MAPPING)?
            }
            (Some((_, event)), _) => view
                .written(if event.kind.as_str() == "external_function" {
                    view.external_name_span(event.span)
                } else {
                    event.span
                })
                .ok_or(MAPPING)?,
            (None, Some(tag)) => tag.written.clone().ok_or(MAPPING)?,
            (None, None) => return Err(BINDING),
        };
        let latest_tag = self.tags.get(name).map(|(_, order)| *order);
        let kind = match events {
            Some(events)
                if latest_tag
                    .is_none_or(|order| self.orders[events.last].is_some_and(|at| at > order)) =>
            {
                view.model.symbol_type_events[events.last].kind.as_str()
            }
            _ if latest_tag.is_some() => "var",
            _ => return Err(BINDING),
        };
        Ok((declaration, kind))
    }
}
