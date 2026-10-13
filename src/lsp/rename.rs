//! Complete ordinary-symbol edits over current, compatible model owners.

use super::*;

pub(super) struct Decision {
    pub range: Range,
    pub edit: WorkspaceEdit,
    roots: Vec<Url>,
}

struct View {
    root: Url,
    revision: String,
    declaration: (String, Span),
    kind: &'static str,
    targets: Vec<(String, Span)>,
    model: Model,
    tags: Vec<rename_tags::Reference>,
}

fn ordinary(kind: &str) -> bool {
    matches!(kind, "var" | "varexo" | "varexo_det" | "parameters")
}

/// Parser-owned name slots. An equal spelling in a file/function/label slot
/// is not evidence that the token refers to the selected ordinary symbol.
fn binding_sites(model: &Model, name: &str) -> HashSet<Span> {
    use crate::engine::language::model::{DottedHead, PathTarget, ShockKind};
    let mut sites = HashSet::new();
    let mut add = |symbol, span| {
        if model.name(symbol) == name {
            sites.insert(span);
        }
    };
    for event in &model.symbol_type_events {
        add(event.name, event.span);
    }
    for declaration in &model.predetermined {
        add(declaration.name, declaration.span);
    }
    for write in &model.write_targets {
        if model.name(write.name) == name {
            for token in &model.expanded_tokens[write.token_range.clone()] {
                if token.kind == TokenKind::Ident && token.text(&model.source) == name {
                    add(write.name, token.span);
                }
            }
        }
    }
    for usage in &model.model_expression_uses {
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
        add(row.name, row.span);
    }
    for row in model.varobs.iter().chain(&model.varexobs) {
        add(row.name, row.span);
    }
    for row in &model.instrument_uses {
        add(row.name, row.span);
    }
    for row in &model.irf_shocks_options {
        for (symbol, span) in &row.names {
            add(*symbol, *span);
        }
    }
    for row in &model.change_type_statements {
        for (symbol, span) in &row.names {
            add(*symbol, *span);
        }
    }
    for row in &model.var_removed {
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
        add(*symbol, *span);
    }
    for row in &model.optim_weights {
        add(row.first, row.first_span);
        if let (Some(symbol), Some(span)) = (row.second, row.second_span) {
            add(symbol, span);
        }
    }
    for row in &model.init2shocks_blocks {
        for target in &row.rows {
            add(target.endo, target.endo_span);
            add(target.exo, target.exo_span);
        }
    }
    for row in &model.shock_groups {
        for (symbol, span) in &row.members {
            add(*symbol, *span);
        }
    }
    for row in &model.generate_irfs {
        for (symbol, span) in &row.exos {
            add(*symbol, *span);
        }
    }
    for row in &model.shock_blocks {
        for target in &row.scheduled {
            add(target.name, target.name_span);
        }
        for target in &row.stochastic {
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
        for stanza in &row.stanzas {
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
                for reference in &value.path_refs {
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
        for order in statement.token_range.clone() {
            statement_at[order] = Some(index);
        }
    }
    for row in &model.dotted_statements {
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
                        add(*symbol, *span);
                    }
                }
            }
        }
    }
    sites
}

fn capture(inner: &mut Inner, root: Url, name: &str, replacement: &str) -> Option<View> {
    let revision = inner.root_revision(&root)?;
    let model = inner.workspace.get_effective_model(root.as_str())?.clone();
    let report = inner.workspace.expand_report(root.as_str())?;
    if !report.complete
        || !report.model_map.complete
        || !inner.workspace.includes_complete(root.as_str())
    {
        return None;
    }
    let tags = rename_tags::collect(inner, &root, &model);
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
        (Some((index, _)), Some(tag)) if tag.declaration_order < event_order(index)? => {
            tag.written.clone()?
        }
        (Some((_, event)), _) => inner
            .workspace
            .map_effective_origin(root.as_str(), event.span)?,
        (None, Some(tag)) => tag.written.clone()?,
        (None, None) => return None,
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
            model.final_symbol_kind(event.name)?
        }
        _ if latest_tag.is_some() => "var",
        _ => return None,
    };
    // A retype through a model-local binding requires that binding's guards.
    if !ordinary(kind) || event.is_some_and(|(_, event)| !ordinary(event.kind.as_str())) {
        return None;
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
        return None;
    }
    let mut targets = Vec::new();
    let mut target_set = HashSet::new();
    let mut written_tokens: HashMap<String, HashSet<Span>> = HashMap::new();
    let mut mappings = HashMap::new();
    let binding_sites = binding_sites(&model, name);
    let mut eligible = vec![false; model.expanded_tokens.len()];
    for statement in &model.statements {
        if statement.native {
            continue;
        }
        let mut depth = 0usize;
        for order in statement.token_range.clone() {
            let token = model.expanded_tokens.get(order)?;
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
        if token.kind != TokenKind::Ident || token.text(&model.source) != name {
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
            return None;
        }
        let target = mappings
            .entry(token.span)
            .or_insert_with(|| {
                inner
                    .workspace
                    .map_effective_origin(root.as_str(), token.span)
            })
            .clone()?;
        let source = inner.workspace.source_for_normalized_key(&target.0)?;
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
            return None;
        }
        if target_set.insert(target.clone()) {
            targets.push(target);
        }
        renamed_tokens[order].lexeme = Some(replacement.to_string());
    }
    for tag in tags.iter().filter(|tag| tag.name == name) {
        let target = tag.written.clone()?;
        if target_set.insert(target.clone()) {
            targets.push(target);
        }
    }
    rename_tags::rewrite(&model, &mut renamed_tokens, &tags, name, replacement)?;
    if !target_set.contains(&declaration) {
        return None;
    }
    // A literal macro site may execute as a different name or as native text
    // in another copy. Changing it would change a binding outside this edit.
    for (order, token) in model.expanded_tokens.iter().enumerate() {
        let Some(target) = mappings
            .entry(token.span)
            .or_insert_with(|| {
                inner
                    .workspace
                    .map_effective_origin(root.as_str(), token.span)
            })
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
                || !binding_sites.contains(&token.span))
        {
            return None;
        }
    }
    if name != replacement {
        let (renamed, _) =
            crate::engine::language::parser::parse_expanded(&model.source, renamed_tokens);
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
            return None;
        }
    }
    Some(View {
        root,
        revision,
        declaration,
        kind,
        targets,
        model,
        tags,
    })
}

pub(super) fn decide(
    inner: &mut Inner,
    pos: &TextDocumentPositionParams,
    new_name: Option<&str>,
) -> Option<Decision> {
    let uri = &pos.text_document.uri;
    let doc = inner.document(uri)?;
    let text = doc.text.clone();
    let version = doc.version;
    let byte = LineIndex::new(&text).offset_utf16(&text, span_pos(pos.position));
    let mut roots = inner.known_owner_roots(uri);
    if roots.is_empty() && (is_model_root(uri) || uri.scheme() == "untitled") {
        roots.push(uri.clone());
    }
    if roots.is_empty() || !matches!(uri.scheme(), "file" | "untitled") {
        return None;
    }
    let file = crate::engine::workspace::include_resolver::normalize_uri(uri.as_str());
    let selected_tag = roots.iter().find_map(|root| {
        let model = inner.workspace.get_effective_model(root.as_str())?.clone();
        rename_tags::collect(inner, root, &model)
            .into_iter()
            .find_map(|tag| {
                let (target, span) = tag.written?;
                (target == file && span.start <= byte && byte <= span.end)
                    .then_some((tag.name, span))
            })
    });
    let (name, selected_span) = selected_tag.or_else(|| ident_at(&text, byte))?;
    let replacement = new_name.unwrap_or(&name);
    if !is_legal_ident(&name)
        || !is_legal_ident(replacement)
        || new_name.is_some() && replacement == name
    {
        return None;
    }
    let selected = (
        crate::engine::workspace::include_resolver::normalize_uri(uri.as_str()),
        selected_span,
    );
    let mut views = Vec::new();
    for root in roots {
        let view = capture(inner, root, &name, replacement)?;
        if !view.targets.contains(&selected) {
            return None;
        }
        views.push(view);
    }
    // Close over every known owner of every edited source. Equal spelling in
    // an unrelated root never adds that root to this closure.
    let mut index = 0;
    while index < views.len() {
        let files: HashSet<_> = views[index]
            .targets
            .iter()
            .map(|(file, _)| file.clone())
            .collect();
        for file in files {
            let target_uri = naming_edit_url(inner, &views[index].root, &file)?;
            for owner in inner.known_owner_roots(&target_uri) {
                if views.iter().any(|view| view.root == owner) {
                    continue;
                }
                views.push(capture(inner, owner, &name, replacement)?);
            }
        }
        index += 1;
    }
    let binding = (&views[0].declaration, views[0].kind);
    if views
        .iter()
        .any(|view| (&view.declaration, view.kind) != binding)
    {
        return None;
    }
    let targets = views
        .iter()
        .flat_map(|view| view.targets.iter().cloned())
        .collect();
    for view in &views {
        if !rename_tags::compatible(inner, &view.root, &view.model, &view.tags, &targets, &name) {
            return None;
        }
    }
    let mut changes: HashMap<Url, Vec<TextEdit>> = HashMap::new();
    let mut sources = HashMap::new();
    for view in &views {
        for (file, span) in &view.targets {
            if !sources.contains_key(file) {
                let target_uri = naming_edit_url(inner, &view.root, file)?;
                if !matches!(target_uri.scheme(), "file" | "untitled") {
                    return None;
                }
                if target_uri.scheme() == "file" {
                    let path = target_uri.to_file_path().ok()?;
                    match std::fs::metadata(&path) {
                        Ok(metadata)
                            if !metadata.is_file() || metadata.permissions().readonly() =>
                        {
                            return None
                        }
                        Err(_) if inner.document(&target_uri).is_none() => return None,
                        _ => {}
                    }
                }
                let text = inner.workspace.source_for_normalized_key(file)?.to_owned();
                sources.insert(file.clone(), (target_uri, LineIndex::new(&text), text));
            }
            let (target_uri, index, source) = sources.get(file)?;
            let edit = TextEdit {
                range: span_range(index, source, *span),
                new_text: replacement.to_string(),
            };
            let edits = changes.entry(target_uri.clone()).or_default();
            if !edits.contains(&edit) {
                edits.push(edit);
            }
        }
    }
    if changes.is_empty()
        || inner.document(uri)?.version != version
        || inner.document(uri)?.text != text
        || inner.workspace.get_source(uri.as_str())? != text
        || sources.iter().any(|(file, (target, _, _))| {
            crate::engine::workspace::include_resolver::normalize_uri(target.as_str()) != *file
        })
    {
        return None;
    }
    for view in &views {
        if inner.root_revision(&view.root).as_ref() != Some(&view.revision)
            || !inner
                .workspace
                .input_snapshot_is_current(view.root.as_str())
        {
            return None;
        }
    }
    let edit = versioned_workspace_edit(inner, changes);
    Some(Decision {
        range: span_range(&LineIndex::new(&text), &text, selected_span),
        edit,
        roots: views.into_iter().map(|view| view.root).collect(),
    })
}

/// Native Rename loads saved inputs before applying through workspace.applyEdit.
/// A second capture after those loads proves the complete decision again.
pub(super) fn capture_command(inner: &mut Inner, arguments: &[Value]) -> Option<Value> {
    let params: RenameParams = serde_json::from_value(arguments.first()?.clone()).ok()?;
    if !is_legal_ident(&params.new_name) {
        return None;
    }
    let pos = &params.text_document_position;
    let (edit, roots) = match locals::rename(inner, pos, Some(&params.new_name)) {
        Some(edit) => {
            let mut roots = inner.known_owner_roots(&pos.text_document.uri);
            if roots.is_empty() {
                roots.push(pos.text_document.uri.clone());
            }
            (edit?, roots)
        }
        None => {
            let decision = decide(inner, pos, Some(&params.new_name))?;
            (decision.edit, decision.roots)
        }
    };
    let DocumentChanges::Edits(documents) = edit.document_changes.as_ref()? else {
        return None;
    };
    let mut source_uris: HashSet<Url> = documents
        .iter()
        .map(|document| document.text_document.uri.clone())
        .collect();
    for root in &roots {
        source_uris.insert(root.clone());
        let identity = crate::engine::workspace::include_resolver::normalize_uri(root.as_str());
        for file in inner.workspace.input_stamps(root.as_str()).keys() {
            if inner.workspace.source_for_normalized_key(file).is_some()
                && inner.workspace.owner_roots(file).contains(&identity)
            {
                source_uris.insert(naming_edit_url(inner, root, file)?);
            }
        }
    }
    let mut source_uris: Vec<_> = source_uris.into_iter().collect();
    source_uris.sort_by(|left, right| left.as_str().cmp(right.as_str()));
    let mut sources = Vec::new();
    let mut bytes = 0usize;
    for uri in source_uris {
        let text = inner.workspace.get_source(uri.as_str())?;
        bytes = bytes.checked_add(text.len())?;
        if bytes > crate::engine::comparison::compare_snapshots::MAX_SNAPSHOT_SOURCE_BYTES {
            return None;
        }
        sources.push(json!({ "uri": uri, "text": text, "version": inner.document(&uri).map(|document| document.version) }));
    }
    let mut owners = Vec::new();
    for root in roots {
        owners.push(json!({ "root_uri": root, "revision": inner.root_revision(&root)? }));
        if !inner.workspace.input_snapshot_is_current(root.as_str()) {
            return None;
        }
    }
    owners.sort_by(|left, right| left["root_uri"].as_str().cmp(&right["root_uri"].as_str()));
    Some(json!({ "schema_version": 1, "edit": edit, "sources": sources, "owners": owners }))
}
