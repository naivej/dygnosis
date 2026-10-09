use std::collections::{BTreeMap, BTreeSet};

use crate::model::{Decl, Model};
use crate::model_diff::{
    final_symbol_declarations, last_assignment, normalize_equation, normalize_expr,
    parameter_value_changed, ModelDiff, ShockSetting,
};
use crate::model_info::assigned_number;
use crate::timing::TimingAnalysis;

use super::schema::*;

pub(super) fn populate(before: &Model, after: &Model, diff: &mut ModelDiff) {
    parameters(before, after, diff);
    symbols(before, after, diff);
    equation_fields(diff);
    shocks(diff);
    diff.coverage.families = vec![
        coverage(
            SemanticFamily::Parameters,
            &["expression", "evaluated_value"],
        ),
        coverage(
            SemanticFamily::Symbols,
            &[
                "kind",
                "dimension",
                "long_name",
                "tex_name",
                "log_transform",
                "predetermined",
            ],
        ),
        FamilyCoverage {
            family: SemanticFamily::Equations,
            availability: Availability::Partial,
            fields: vec!["expression".into(), "tags".into()],
            limits: vec![ComparisonLimit::new(
                "equation_detail_pending",
                "Token, timing and reference detail is not available in this comparison stage.",
                "semantic_equations",
            )],
        },
        coverage(
            SemanticFamily::Shocks,
            &[
                "form",
                "role",
                "target",
                "block",
                "dimension",
                "domain",
                "written_target",
                "group",
                "group_explicit",
                "related_setup",
                "related_target",
                "references",
                "measure",
                "operation",
                "periods",
                "values",
                "learnt_in",
                "relative_to_initval",
                "released_exogenous",
                "overwrite",
                "status",
            ],
        ),
    ];
}

fn coverage(family: SemanticFamily, fields: &[&str]) -> FamilyCoverage {
    FamilyCoverage {
        family,
        availability: Availability::Complete,
        fields: fields.iter().map(|field| (*field).into()).collect(),
        limits: Vec::new(),
    }
}

fn assignment_expression(model: &Model, name: &str) -> FieldState {
    last_assignment(model, name)
        .map(|assignment| FieldState::text(&assignment.expression))
        .unwrap_or_else(FieldState::absent)
}

fn evaluated_value(model: &Model, name: &str) -> FieldState {
    if last_assignment(model, name).is_none() {
        return FieldState::absent();
    }
    FieldState::number(assigned_number(model, name))
}

fn parameters(before: &Model, after: &Model, diff: &mut ModelDiff) {
    for (index, change) in diff.changed_parameter_values.iter().enumerate() {
        let mut row = SemanticRow::new(
            SemanticFamily::Parameters,
            ChangeKind::Changed,
            &change.name,
        );
        row.pointer = format!("/changed_parameter_values/{index}");
        row.before = Some(parameter_side(before, &change.name));
        row.after = Some(parameter_side(after, &change.name));
        let mut expression = FieldChange::new(
            "expression",
            "Assigned expression",
            assignment_expression(before, &change.name),
            assignment_expression(after, &change.name),
        );
        expression.changed = normalize_expr(&change.old_raw) != normalize_expr(&change.new_raw)
            || expression.before.state != expression.after.state;
        let mut value = FieldChange::new(
            "evaluated_value",
            "Evaluated value",
            evaluated_value(before, &change.name),
            evaluated_value(after, &change.name),
        );
        value.changed = parameter_value_changed(change.old_value, change.new_value)
            || value.before.state != value.after.state;
        value.numeric_difference = match (change.old_value, change.new_value) {
            (Some(before), Some(after)) if before.is_finite() && after.is_finite() => {
                Some(after - before).filter(|difference| difference.is_finite())
            }
            _ => None,
        };
        if expression.changed {
            row.facets.push(ChangeFacet::Expression);
        }
        if value.changed {
            row.facets.push(ChangeFacet::ParameterValue);
        }
        row.fields = vec![expression, value];
        diff.semantic.push_row(row);
    }
}

fn parameter_side(model: &Model, name: &str) -> RowSide {
    let mut side = RowSide::named(name, ComparisonScope::global());
    side.provenance = last_assignment(model, name).map(|assignment| OccurrenceProvenance {
        span: assignment.span,
        parse_order: Some(assignment.active_tokens.start),
        equation_id: None,
        statement_id: model
            .statements
            .iter()
            .find(|statement| {
                !assignment.active_tokens.is_empty()
                    && statement.token_range.start <= assignment.active_tokens.start
                    && assignment.active_tokens.end <= statement.token_range.end
            })
            .map(|statement| statement.id),
    });
    side
}

fn symbol_side(model: &Model, decl: &Decl) -> RowSide {
    let dimension = model
        .final_heterogeneity(decl)
        .map(|name| model.name(name).to_string());
    let mut side = RowSide::named(
        model.name(decl.name),
        ComparisonScope {
            domain: if dimension.is_some() {
                "heterogeneous"
            } else {
                "aggregate"
            }
            .into(),
            dimension,
            block: None,
        },
    );
    side.provenance = Some(OccurrenceProvenance {
        span: decl.span,
        parse_order: Some(decl.parse_order),
        equation_id: None,
        statement_id: model
            .written_declarations
            .iter()
            .find(|written| {
                written.declaration.parse_order == decl.parse_order
                    && written.declaration.name == decl.name
            })
            .map(|written| written.statement_id),
    });
    side
}

fn symbol_values(
    model: &Model,
    decl: Option<&&Decl>,
    timing: &TimingAnalysis,
) -> BTreeMap<&'static str, FieldState> {
    let Some(decl) = decl else {
        return BTreeMap::new();
    };
    let kind = model.final_kind_or_written_if_excluded(decl.name);
    let dimension = model.final_heterogeneity(decl).map(|name| model.name(name));
    BTreeMap::from([
        ("kind", FieldState::optional_text(kind)),
        ("dimension", FieldState::optional_text(dimension)),
        (
            "long_name",
            FieldState::optional_text(decl.long_name.as_deref()),
        ),
        (
            "tex_name",
            FieldState::optional_text(decl.tex_name.as_deref()),
        ),
        ("log_transform", FieldState::boolean(decl.log_transform)),
        (
            "predetermined",
            FieldState::boolean(timing.is_predetermined(decl.name)),
        ),
    ])
}

fn symbols(before: &Model, after: &Model, diff: &mut ModelDiff) {
    let old_timing = TimingAnalysis::new(before);
    let new_timing = TimingAnalysis::new(after);
    let old = final_symbol_declarations(before);
    let new = final_symbol_declarations(after);
    let names: BTreeSet<_> = old.keys().chain(new.keys()).collect();
    for name in names {
        let old_decl = old.get(name);
        let new_decl = new.get(name);
        let old_fields = symbol_values(before, old_decl, &old_timing);
        let new_fields = symbol_values(after, new_decl, &new_timing);
        if old_fields == new_fields {
            continue;
        }
        let change = match (old_decl, new_decl) {
            (None, _) => ChangeKind::Added,
            (_, None) => ChangeKind::Removed,
            _ => ChangeKind::Changed,
        };
        let mut row = SemanticRow::new(SemanticFamily::Symbols, change, name);
        row.before = old_decl.map(|decl| symbol_side(before, decl));
        row.after = new_decl.map(|decl| symbol_side(after, decl));
        row.pointer = symbol_legacy_pointer(diff, name, change).unwrap_or_default();
        for (field, label, facet) in [
            ("kind", "Symbol kind", ChangeFacet::SymbolKind),
            ("dimension", "Dimension", ChangeFacet::Scope),
            ("long_name", "Long name", ChangeFacet::Label),
            ("tex_name", "TeX label", ChangeFacet::Label),
            ("log_transform", "Log flag", ChangeFacet::LogTransform),
            (
                "predetermined",
                "Predetermined convention",
                ChangeFacet::PredeterminedConvention,
            ),
        ] {
            let detail = FieldChange::new(
                field,
                label,
                old_fields
                    .get(field)
                    .cloned()
                    .unwrap_or_else(FieldState::absent),
                new_fields
                    .get(field)
                    .cloned()
                    .unwrap_or_else(FieldState::absent),
            );
            if detail.changed && !row.facets.contains(&facet) {
                row.facets.push(facet);
            }
            row.fields.push(detail);
        }
        // A new/removed parameter or a kind transition has no legacy common
        // calibration row. Keep its calibration with its one owning object row.
        if change != ChangeKind::Changed
            || !diff
                .changed_parameter_values
                .iter()
                .any(|parameter| parameter.name == *name)
        {
            let old_is_parameter = old_decl
                .is_some_and(|decl| before.final_symbol_kind(decl.name) == Some("parameters"));
            let new_is_parameter = new_decl
                .is_some_and(|decl| after.final_symbol_kind(decl.name) == Some("parameters"));
            if old_is_parameter || new_is_parameter {
                row.fields.push(FieldChange::new(
                    "expression",
                    "Assigned expression",
                    if old_is_parameter {
                        assignment_expression(before, name)
                    } else {
                        FieldState::absent()
                    },
                    if new_is_parameter {
                        assignment_expression(after, name)
                    } else {
                        FieldState::absent()
                    },
                ));
                row.fields.push(FieldChange::new(
                    "evaluated_value",
                    "Evaluated value",
                    if old_is_parameter {
                        evaluated_value(before, name)
                    } else {
                        FieldState::absent()
                    },
                    if new_is_parameter {
                        evaluated_value(after, name)
                    } else {
                        FieldState::absent()
                    },
                ));
            }
        }
        diff.semantic.push_row(row);
    }
}

fn symbol_legacy_pointer(diff: &ModelDiff, name: &str, change: ChangeKind) -> Option<String> {
    if change == ChangeKind::Changed {
        return diff
            .symbols_changed
            .iter()
            .position(|row| row.name == name)
            .map(|index| format!("/symbols_changed/{index}"));
    }
    let lists = if change == ChangeKind::Added {
        [
            ("added_endogenous", &diff.added_endogenous),
            ("added_exogenous", &diff.added_exogenous),
            ("added_parameters", &diff.added_parameters),
        ]
    } else {
        [
            ("removed_endogenous", &diff.removed_endogenous),
            ("removed_exogenous", &diff.removed_exogenous),
            ("removed_parameters", &diff.removed_parameters),
        ]
    };
    lists.into_iter().find_map(|(key, rows)| {
        rows.iter()
            .position(|row| row == name)
            .map(|index| format!("/{key}/{index}"))
    })
}

fn tags(tags: &BTreeMap<String, String>) -> FieldState {
    FieldState::present(FieldValue::Record(
        tags.iter()
            .map(|(key, value)| (key.clone(), FieldValue::Text(value.clone())))
            .collect(),
    ))
}

fn equation_fields(diff: &mut ModelDiff) {
    let mut rows = Vec::new();
    equation_rows(
        &mut rows,
        "",
        &diff.added_equations,
        &diff.removed_equations,
        &diff.changed_equations,
    );
    for (index, group) in diff.heterogeneous_equations.iter().enumerate() {
        equation_rows(
            &mut rows,
            &format!("/heterogeneous_equations/{index}"),
            &group.added,
            &group.removed,
            &group.changed,
        );
    }
    for row in rows {
        diff.semantic.push_row(row);
    }
}

fn equation_side(
    name: Option<&str>,
    index: usize,
    domain: &str,
    dimension: Option<&str>,
) -> RowSide {
    let mut side = RowSide::named(
        name.unwrap_or("Equation"),
        ComparisonScope {
            domain: domain.into(),
            dimension: dimension.map(str::to_string),
            block: None,
        },
    );
    side.equation_index = Some(index);
    side
}

fn equation_rows(
    rows: &mut Vec<SemanticRow>,
    prefix: &str,
    added: &[crate::model_diff::IndexedEquation],
    removed: &[crate::model_diff::IndexedEquation],
    changed: &[crate::model_diff::EquationChange],
) {
    for (list, entries, change) in [
        ("added", added, ChangeKind::Added),
        ("removed", removed, ChangeKind::Removed),
    ] {
        for (index, equation) in entries.iter().enumerate() {
            let mut row = SemanticRow::new(
                SemanticFamily::Equations,
                change,
                equation.name.as_deref().unwrap_or("Equation"),
            );
            row.pointer = if prefix.is_empty() {
                format!("/{list}_equations/{index}")
            } else {
                format!("{prefix}/{list}/{index}")
            };
            let side = equation_side(
                equation.name.as_deref(),
                equation.index,
                &equation.domain,
                equation.dimension.as_deref(),
            );
            let expression = FieldState::text(&equation.text);
            let tag_values = tags(&equation.tags);
            if change == ChangeKind::Added {
                row.after = Some(side);
                row.fields = vec![
                    FieldChange::new("expression", "Expression", FieldState::absent(), expression),
                    FieldChange::new("tags", "Tags", FieldState::absent(), tag_values),
                ];
            } else {
                row.before = Some(side);
                row.fields = vec![
                    FieldChange::new("expression", "Expression", expression, FieldState::absent()),
                    FieldChange::new("tags", "Tags", tag_values, FieldState::absent()),
                ];
            }
            row.facets = vec![ChangeFacet::Expression, ChangeFacet::Tags];
            rows.push(row);
        }
    }
    for (index, equation) in changed.iter().enumerate() {
        let mut row = SemanticRow::new(
            SemanticFamily::Equations,
            ChangeKind::Changed,
            equation
                .name_new
                .as_deref()
                .or(equation.name_old.as_deref())
                .unwrap_or("Equation"),
        );
        row.pointer = if prefix.is_empty() {
            format!("/changed_equations/{index}")
        } else {
            format!("{prefix}/changed/{index}")
        };
        row.before = Some(equation_side(
            equation.name_old.as_deref(),
            equation.index_old,
            &equation.domain,
            equation.dimension.as_deref(),
        ));
        row.after = Some(equation_side(
            equation.name_new.as_deref(),
            equation.index_new,
            &equation.domain,
            equation.dimension.as_deref(),
        ));
        let mut expression = FieldChange::new(
            "expression",
            "Expression",
            FieldState::text(&equation.text_old),
            FieldState::text(&equation.text_new),
        );
        expression.changed =
            normalize_equation(&equation.text_old) != normalize_equation(&equation.text_new);
        let tag_values = FieldChange::new(
            "tags",
            "Tags",
            tags(&equation.tags_old),
            tags(&equation.tags_new),
        );
        if expression.changed {
            row.facets.push(ChangeFacet::Expression);
        }
        if tag_values.changed {
            row.facets.push(ChangeFacet::Tags);
        }
        row.fields = vec![expression, tag_values];
        rows.push(row);
    }
}

fn list(value: Option<&Vec<String>>) -> FieldState {
    value
        .map(|values| {
            FieldState::present(FieldValue::List(
                values.iter().cloned().map(FieldValue::Text).collect(),
            ))
        })
        .unwrap_or_else(FieldState::absent)
}

fn shock_values(setting: Option<&ShockSetting>) -> BTreeMap<&'static str, FieldState> {
    let Some(setting) = setting else {
        return BTreeMap::new();
    };
    let mut fields = BTreeMap::from([
        ("block", FieldState::text(&setting.block)),
        (
            "dimension",
            FieldState::optional_text(setting.heterogeneity.as_deref()),
        ),
        (
            "domain",
            FieldState::optional_text(setting.domain.as_deref()),
        ),
        (
            "written_target",
            FieldState::optional_text(setting.written_target.as_deref()),
        ),
        ("group", FieldState::optional_text(setting.group.as_deref())),
        (
            "related_setup",
            FieldState::optional_text(setting.related_setup.as_deref()),
        ),
        (
            "related_target",
            FieldState::optional_text(setting.related_target.as_deref()),
        ),
        ("references", list(setting.references.as_ref())),
        (
            "measure",
            FieldState::optional_text(setting.measure.as_deref()),
        ),
        (
            "operation",
            FieldState::optional_text(setting.operation.as_deref()),
        ),
        ("periods", list(setting.periods.as_ref())),
        ("values", list(setting.values.as_ref())),
        (
            "released_exogenous",
            FieldState::optional_text(setting.released_exogenous.as_deref()),
        ),
        ("overwrite", FieldState::boolean(setting.overwrite)),
        ("status", FieldState::text(&setting.status)),
    ]);
    fields.insert(
        "group_explicit",
        setting
            .group_explicit
            .map(FieldState::boolean)
            .unwrap_or_else(FieldState::absent),
    );
    fields.insert(
        "relative_to_initval",
        setting
            .relative_to_initval
            .map(FieldState::boolean)
            .unwrap_or_else(FieldState::absent),
    );
    fields.insert(
        "learnt_in",
        setting
            .learnt_in
            .as_ref()
            .map(|period| {
                FieldState::present(FieldValue::Record(BTreeMap::from([
                    ("kind".into(), FieldValue::Text(period.kind.clone())),
                    ("text".into(), FieldValue::Text(period.text.clone())),
                ])))
            })
            .unwrap_or_else(FieldState::absent),
    );
    fields
}

fn shock_side(name: &str, setting: &ShockSetting) -> RowSide {
    let mut side = RowSide::named(
        name,
        ComparisonScope {
            domain: if setting.heterogeneity.is_some() {
                "heterogeneous"
            } else {
                "aggregate"
            }
            .into(),
            dimension: setting.heterogeneity.clone(),
            block: Some(setting.block.clone()),
        },
    );
    side.occurrence = Some(setting.occurrence_id);
    side.provenance = setting.source_span.map(|span| OccurrenceProvenance {
        span,
        parse_order: setting.assignment_tokens.as_ref().map(|range| range.start),
        equation_id: None,
        statement_id: None,
    });
    side
}

fn shocks(diff: &mut ModelDiff) {
    for (index, change) in diff.shock_setup_changes.iter().enumerate() {
        let kind = match change.change.as_str() {
            "added" => ChangeKind::Added,
            "removed" => ChangeKind::Removed,
            _ => ChangeKind::Changed,
        };
        let mut row = SemanticRow::new(SemanticFamily::Shocks, kind, &change.target);
        row.count_unit = CountUnit::AcceptedOccurrence;
        row.pointer = format!("/shock_setup_changes/{index}");
        row.before = change
            .before
            .as_ref()
            .map(|setting| shock_side(&change.target, setting));
        row.after = change
            .after
            .as_ref()
            .map(|setting| shock_side(&change.target, setting));
        row.facets.push(ChangeFacet::ShockSetup);
        let old = shock_values(change.before.as_ref());
        let new = shock_values(change.after.as_ref());
        for (field, label) in [("form", "Form"), ("role", "Role"), ("target", "Target")] {
            let value = match field {
                "form" => &change.form,
                "role" => &change.role,
                _ => &change.target,
            };
            row.fields.push(FieldChange::new(
                field,
                label,
                if change.before.is_some() {
                    FieldState::text(value)
                } else {
                    FieldState::absent()
                },
                if change.after.is_some() {
                    FieldState::text(value)
                } else {
                    FieldState::absent()
                },
            ));
        }
        for (field, label) in [
            ("block", "Block"),
            ("dimension", "Dimension"),
            ("domain", "Domain"),
            ("written_target", "Written target"),
            ("group", "Shock group"),
            ("group_explicit", "Explicit group"),
            ("related_setup", "Related setup"),
            ("related_target", "Related target"),
            ("references", "Written references"),
            ("measure", "Measure"),
            ("operation", "Operation"),
            ("periods", "Periods"),
            ("values", "Values"),
            ("learnt_in", "Learning period"),
            ("relative_to_initval", "Relative to initial value"),
            ("released_exogenous", "Released exogenous"),
            ("overwrite", "Overwrite"),
            ("status", "Context status"),
        ] {
            row.fields.push(FieldChange::new(
                field,
                label,
                old.get(field).cloned().unwrap_or_else(FieldState::absent),
                new.get(field).cloned().unwrap_or_else(FieldState::absent),
            ));
        }
        diff.semantic.push_row(row);
    }
}
