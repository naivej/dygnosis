use super::*;

fn dimension(model: &Model, written: &WrittenDeclaration) -> FieldState {
    FieldState::optional_text(
        written
            .declaration
            .heterogeneity
            .map(|(name, _)| model.name(name)),
    )
}

fn primary_written<'a>(model: &'a Model, side: Option<&RowSide>) -> Option<&'a WrittenDeclaration> {
    let side = side?;
    let proof = side.provenance.as_ref()?;
    let order = proof.parse_order?;
    let statement = proof.statement_id?;
    model.written_declarations.iter().find(|written| {
        written.statement_id == statement
            && written.declaration.parse_order == order
            && model.name(written.declaration.name) == side.name
            && (written.token_range.contains(&order) || written.token_range.end == order)
            && model.statements.get(statement).is_some_and(|parent| {
                parent.id == statement
                    && parent.token_range.start <= written.token_range.start
                    && written.token_range.end <= parent.token_range.end
            })
    })
}

/// Enrich an existing primary object only when it owns this exact declaration.
pub(super) fn enrich_primary(
    before: &Model,
    after: &Model,
    diff: &mut ModelDiff,
    claims: &mut TokenClaims,
) -> [BTreeSet<usize>; 2] {
    let mut owned = [BTreeSet::new(), BTreeSet::new()];
    for row in diff.semantic.rows.iter_mut().filter(|row| {
        row.family == SemanticFamily::Symbols && row.count_unit == CountUnit::FinalFact
    }) {
        let old = primary_written(before, row.before.as_ref());
        let new = primary_written(after, row.after.as_ref());
        if old.is_none() && new.is_none() {
            continue;
        }
        for (index, model, side, written, row_side) in [
            (0, before, Side::Before, old, &mut row.before),
            (1, after, Side::After, new, &mut row.after),
        ] {
            if let Some(written) = written {
                owned[index].insert(written.declaration.parse_order);
                claims.claim(side, written.statement_id, written.token_range.clone());
                if let Some(row_side) = row_side {
                    row_side.context = occurrences::statement_side(
                        model,
                        written.statement_id,
                        &row_side.name,
                        row_side.scope.clone(),
                    )
                    .context;
                }
            }
        }
        let field = FieldChange::new(
            "written_dimension",
            "Written declaration dimension",
            old.map(|written| dimension(before, written))
                .unwrap_or_else(FieldState::absent),
            new.map(|written| dimension(after, written))
                .unwrap_or_else(FieldState::absent),
        );
        if field.changed && !row.facets.contains(&ChangeFacet::Scope) {
            row.facets.push(ChangeFacet::Scope);
        }
        row.fields.push(field);
        let kind = FieldChange::new(
            "written_kind",
            "Written declaration kind",
            old.map(|written| FieldState::text(&written.written_kind))
                .unwrap_or_else(FieldState::absent),
            new.map(|written| FieldState::text(&written.written_kind))
                .unwrap_or_else(FieldState::absent),
        );
        if kind.changed && !row.facets.contains(&ChangeFacet::SymbolKind) {
            row.facets.push(ChangeFacet::SymbolKind);
        }
        row.fields.push(kind);
    }
    if let Some(coverage) = diff
        .coverage
        .families
        .iter_mut()
        .find(|entry| entry.family == SemanticFamily::Symbols)
    {
        coverage.fields.push("written_dimension".into());
        coverage.fields.push("written_kind".into());
        coverage.fields.sort();
        coverage.fields.dedup();
    }
    owned
}

pub(super) fn collect(model: &Model, facts: &mut Vec<CapturedFact>, owned: &BTreeSet<usize>) {
    for (index, written) in model.written_declarations.iter().enumerate() {
        if written.written_kind == "model_local_variable"
            || owned.contains(&written.declaration.parse_order)
        {
            continue;
        }
        let name = model.name(written.declaration.name);
        let mut value = fact(
            model,
            SemanticFamily::Symbols,
            "written_declaration",
            name,
            vec![name.into()],
            Some(written.declaration.parse_order),
            index,
        );
        // The retained WrittenDeclaration proves this per-object token owner.
        value.side = occurrences::statement_side(
            model,
            written.statement_id,
            name,
            value.side.scope.clone(),
        );
        if let Some(proof) = &mut value.side.provenance {
            proof.span = written.declaration.span;
            proof.parse_order = Some(written.declaration.parse_order);
        }
        field(
            &mut value,
            "written_dimension",
            dimension(model, written),
            ChangeFacet::Scope,
        );
        text(
            &mut value,
            "written_kind",
            &written.written_kind,
            ChangeFacet::SymbolKind,
        );
        field(
            &mut value,
            "long_name",
            FieldState::optional_text(written.declaration.long_name.as_deref()),
            ChangeFacet::Label,
        );
        field(
            &mut value,
            "tex_name",
            FieldState::optional_text(written.declaration.tex_name.as_deref()),
            ChangeFacet::Label,
        );
        field(
            &mut value,
            "log_transform",
            FieldState::boolean(written.declaration.log_transform),
            ChangeFacet::LogTransform,
        );
        value.claims.push(written.token_range.clone());
        facts.push(value);
    }
}
