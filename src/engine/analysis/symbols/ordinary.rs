//! Accepted ordinary name slots and complete Rename decisions.
use super::*;

pub(super) fn ordinary(kind: &str) -> bool {
    matches!(kind, "var" | "varexo" | "varexo_det" | "parameters")
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

/// Parser-owned name slots. An equal spelling in a file/function/label slot
/// is not evidence that the token refers to the selected ordinary symbol.
pub(super) fn binding_sites(model: &Model, name: &str) -> HashSet<Span> {
    use crate::engine::language::model::{
        DottedHead, PathTarget, SemiStructuralKind, SemiStructuralValue, ShockKind,
    };
    let mut sites = HashSet::new();
    let mut add = |symbol, span| {
        if model.name(symbol) == name {
            sites.insert(span);
        }
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
        if model.name(write.name) == name {
            for token in &model.expanded_tokens[write.token_range.clone()] {
                if crate::engine::control::cancelled() {
                    break;
                }
                if token.kind == TokenKind::Ident && token.text(&model.source) == name {
                    add(write.name, token.span);
                }
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
    for command in &model.semi_structural_commands {
        if crate::engine::control::cancelled() {
            break;
        }
        if command.kind != SemiStructuralKind::PacModel {
            continue;
        }
        for option in &command.options {
            if crate::engine::control::cancelled() {
                break;
            }
            if option.name.eq_ignore_ascii_case("discount")
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
            if symbols.iter().any(|symbol| model.name(*symbol) == name) {
                for token in model.expanded_tokens[target.parse_order..]
                    .iter()
                    .take_while(|token| !matches!(token.kind, TokenKind::Eq | TokenKind::Semi))
                {
                    if token.kind == TokenKind::Ident && token.text(&model.source) == name {
                        add(
                            symbols
                                .iter()
                                .copied()
                                .find(|symbol| model.name(*symbol) == name)
                                .unwrap(),
                            token.span,
                        );
                    }
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
                    if reference
                        .namespace
                        .as_deref()
                        .is_none_or(|namespace| namespace == "self")
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
    let mut statement_at = vec![None; model.expanded_tokens.len()];
    for (index, statement) in model.statements.iter().enumerate() {
        if crate::engine::control::cancelled() {
            break;
        }
        for order in statement.token_range.clone() {
            if crate::engine::control::cancelled() {
                break;
            }
            statement_at[order] = Some(index);
        }
    }
    for row in &model.dotted_statements {
        if crate::engine::control::cancelled() {
            break;
        }
        let target_start = token_spans.get(&row.span.start).copied();
        // copy_source is the accepted source binding. Its first token follows
        // the copy '=' in this parser execution, not in an arbitrary text scan.
        let source_start = statement_at
            .get(row.parse_order)
            .copied()
            .flatten()
            .and_then(|index| model.statements.get(index))
            .and_then(|statement| model.expanded_tokens.get(statement.token_range.clone()))
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
    if !is_legal_ident(name) || !is_legal_ident(replacement) {
        return Err(IDENTIFIER);
    }
    let model = &view.model;
    let tags = &view.tags;
    let (declaration, kind) = declaration(view, name)?;
    let event = model
        .symbol_type_events
        .iter()
        .find(|event| model.name(event.name) == name);
    // A retype through a model-local binding requires that binding's guards.
    if !ordinary(kind) || event.is_some_and(|event| !ordinary(event.kind.as_str())) {
        return Err(BINDING);
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
    let mut written_tokens: HashMap<String, HashSet<Span>> = HashMap::new();
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
        let source = view.sources.get(&target.0).ok_or(MAPPING)?;
        let literal_tokens = written_tokens.entry(target.0.clone()).or_insert_with(|| {
            tokenize(source)
                .into_iter()
                .filter(|written| written.kind == TokenKind::Ident)
                .map(|written| written.span)
                .collect()
        });
        if source.get(target.1.start as usize..target.1.end as usize) != Some(name)
            || !literal_tokens.contains(&target.1)
        {
            // An interpolation is not an editable literal identifier.
            return Err(MAPPING);
        }
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
        let (renamed, _) = crate::engine::language::parser::parse_expanded_controlled(
            &model.source,
            renamed_tokens,
            None,
            model.macro_work_limit,
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
    let model = &view.model;
    let tags = &view.tags;
    let event = model
        .symbol_type_events
        .iter()
        .enumerate()
        .find(|(_, event)| model.name(event.name) == name);
    let tag_declaration = tags.iter().find(|tag| tag.declaration && tag.name == name);
    let event_order = |index| {
        model
            .type_event_occurrences
            .iter()
            .find(|(event, _)| *event == index)
            .map(|(_, range)| range.end)
    };
    let declaration = match (event, tag_declaration) {
        (Some((index, _)), Some(tag))
            if tag.declaration_order < event_order(index).ok_or(MAPPING)? =>
        {
            tag.written.clone().ok_or(MAPPING)?
        }
        (Some((_, event)), _) => view.written(event.span).ok_or(MAPPING)?,
        (None, Some(tag)) => tag.written.clone().ok_or(MAPPING)?,
        (None, None) => return Err(BINDING),
    };
    let latest_tag = tags
        .iter()
        .filter(|tag| tag.declaration && tag.name == name)
        .map(|tag| tag.declaration_order)
        .max();
    let latest_event = model
        .symbol_type_events
        .iter()
        .enumerate()
        .rev()
        .find(|(_, event)| model.name(event.name) == name);
    let kind = match latest_event {
        Some((index, event))
            if latest_tag.is_none_or(|order| event_order(index).is_some_and(|at| at > order)) =>
        {
            model.final_symbol_kind(event.name).ok_or(MAPPING)?
        }
        _ if latest_tag.is_some() => "var",
        _ => return Err(BINDING),
    };
    Ok((declaration, kind))
}
