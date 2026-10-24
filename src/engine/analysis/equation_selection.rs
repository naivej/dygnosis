//! Apply explicit invocation selectors after Parse/Check using retained facts.

use std::cell::OnceCell;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::engine::analysis_configuration::{EquationSelection, EquationSelectionMode};
use crate::engine::diagnostics::{self, Diagnostic, Severity, usage::Usage};
use crate::engine::language::intern::Name;
use crate::engine::language::macro_expr::MacroBudget;
use crate::engine::language::model::{Decl, Model, SymbolKind, SymbolTypeEvent};
use crate::engine::language::span::Span;

#[derive(Clone, Debug)]
pub(crate) struct InvocationAnalysis {
    pub diagnostics: Arc<Vec<Diagnostic>>,
    pub failure: Option<String>,
}

struct SelectionPlan {
    removed: Vec<bool>,
    excluded: HashSet<Name>,
    changes: Vec<(Decl, SymbolKind)>,
}

/// Share the lazy Check usage owner with selection, then discard its graph
/// before committing. Only diagnostic output survives with the root result.
pub(crate) fn apply(model: &mut Model, selection: &EquationSelection, budget: &mut MacroBudget) {
    let usage = OnceCell::new();
    let mut diagnostics = diagnostics::before_equation_selection(model, &usage);
    let blocked = model.macro_incomplete()
        || !model.parse_completed
        || diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity == Severity::Error);
    let prepared = (!blocked).then(|| {
        plan(
            model,
            selection,
            budget,
            usage.get_or_init(|| Usage::new(model)),
        )
    });
    drop(usage);
    let failure = if let Some(prepared) = prepared {
        match prepared {
            Ok(plan) => {
                commit(model, plan);
                diagnostics = diagnostics::after_equation_selection(model, diagnostics);
                None
            }
            Err(diagnostic) => {
                let cause = format!(
                    "Equation selection was not applied: {} Selected model facts are unavailable; change the tag values or reset Analysis Configuration.",
                    diagnostic.message
                );
                diagnostics.push(*diagnostic);
                Some(cause)
            }
        }
    } else {
        Some("Equation selection was not applied because parsing, macro expansion or pre-selection checks did not complete successfully. Selected model facts are unavailable; fix the reported source problems or reset Analysis Configuration.".into())
    };
    model.invocation_analysis = Some(InvocationAnalysis {
        diagnostics: Arc::new(diagnostics),
        failure,
    });
}

fn limit(anchor: Span) -> Box<Diagnostic> {
    Box::new(Diagnostic::new(
        anchor,
        Severity::Information,
        "I211",
        "Equation selection reached the work allowance or was cancelled. No selection was applied.",
    ))
}

fn plan(
    model: &Model,
    selection: &EquationSelection,
    budget: &mut MacroBudget,
    usage: &Usage<'_>,
) -> Result<SelectionPlan, Box<Diagnostic>> {
    let anchor = model.model_block.unwrap_or_default();
    let fail =
        |code, message: String| Box::new(Diagnostic::new(anchor, Severity::Error, code, message));
    let spend =
        |budget: &mut MacroBudget, amount| budget.spend_work(amount).map_err(|_| limit(anchor));
    spend(
        budget,
        model
            .equations
            .len()
            .saturating_mul(4)
            .saturating_add(selection.values.len())
            .saturating_add(1),
    )?;
    let mut unmatched: HashSet<&str> = selection.values.iter().map(String::as_str).collect();
    let mut removed = vec![false; model.equations.len()];
    let include = selection.mode == EquationSelectionMode::Include;
    // The pin consumes dynamic matches before querying the static-only list.
    for static_only in [false, true] {
        let mut matched = HashSet::new();
        for (index, equation) in model.equations.iter().enumerate() {
            spend(budget, 1)?;
            if equation.is_local || equation.static_tag != static_only {
                continue;
            }
            let hit = equation
                .tag_map
                .get(&selection.tag)
                .is_some_and(|value| unmatched.contains(value.as_str()));
            if hit {
                matched.insert(equation.tag_map[&selection.tag].as_str());
            }
            removed[index] = hit != include;
        }
        for value in matched {
            unmatched.remove(value);
        }
    }
    let declarations = model.final_endogenous();
    let endogenous: HashSet<_> = declarations
        .iter()
        .map(|declaration| declaration.name)
        .collect();
    let names: HashMap<_, _> = declarations
        .iter()
        .map(|declaration| (model.name(declaration.name), declaration.name))
        .collect();
    let mut excluded = HashSet::new();
    // Both lists validate ownership. Static-only owners never change type and
    // do not enter the duplicate-owner check.
    for static_only in [false, true] {
        let mut number = 0;
        for (index, equation) in model.equations.iter().enumerate() {
            spend(budget, 1)?;
            if equation.is_local || equation.static_tag != static_only {
                continue;
            }
            number += 1;
            if !removed[index] {
                continue;
            }
            let name = if let Some(name) = equation.tag_map.get("endogenous") {
                names.get(name.as_str()).copied()
            } else {
                let mut names = usage
                    .names_budgeted(equation.lhs_expr, budget)
                    .map_err(|_| limit(anchor))?;
                names.retain(|name| endogenous.contains(name));
                (names.len() == 1).then(|| *names.iter().next().unwrap())
            };
            let Some(name) = name else {
                return Err(fail(
                    "E336",
                    format!(
                        "Equation {number} has been excluded but it does not have a single variable on its left-hand side or an `endogenous` tag"
                    ),
                ));
            };
            if !static_only && !excluded.insert(name) {
                return Err(fail(
                    "E337",
                    format!(
                        "Variable {} was excluded twice via a model_remove or model_replace statement, or via the include_eqs or exclude_eqs option",
                        model.name(name)
                    ),
                ));
            }
        }
    }
    if !unmatched.is_empty() {
        return Err(fail(
            "E335",
            format!(
                "model_remove/model_replace/exclude_eqs/include_eqs: The equations specified by {} were not found.",
                selection
                    .values
                    .iter()
                    .map(|value| format!("{}={value}", selection.tag))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        ));
    }
    let static_count = model
        .equations
        .iter()
        .zip(&removed)
        .filter(|(row, remove)| row.static_tag && !**remove)
        .count();
    let dynamic_count = model
        .equations
        .iter()
        .zip(&removed)
        .filter(|(row, remove)| row.dynamic_tag && !**remove)
        .count();
    if static_count != dynamic_count {
        return Err(fail("E483", "exclude_eqs/include_eqs: You must remove the same number of equations marked `static` as equations marked `dynamic`.".into()));
    }
    let roots = model
        .equations
        .iter()
        .zip(&removed)
        .filter(|(row, remove)| !row.is_local && !**remove)
        .flat_map(|(row, _)| [row.lhs_expr, row.rhs_expr].into_iter().flatten());
    // One collection shares live local and constructor nodes across every
    // retained equation. Written references remain available for navigation.
    let used = usage
        .names_budgeted(roots, budget)
        .map_err(|_| limit(anchor))?;
    spend(
        budget,
        model
            .endogenous
            .len()
            .saturating_add(declarations.len())
            .saturating_add(excluded.len())
            .saturating_add(model.equations.len())
            .saturating_add(1),
    )?;
    let changes = declarations
        .into_iter()
        .filter(|declaration| excluded.contains(&declaration.name))
        .map(|declaration| {
            (
                declaration.clone(),
                if used.contains(&declaration.name) {
                    SymbolKind::Varexo
                } else {
                    SymbolKind::Excluded
                },
            )
        })
        .collect();
    Ok(SelectionPlan {
        removed,
        excluded,
        changes,
    })
}

#[cfg(test)]
thread_local! { static DECLARATION_VISITS: std::cell::Cell<usize> = const {std::cell::Cell::new(0)}; }

fn commit(model: &mut Model, plan: SelectionPlan) {
    // All failure and work checks precede mutation. Each list is partitioned
    // once; no declaration scan is nested inside the change loop.
    let mut index = 0;
    model.equations.retain(|_| {
        let keep = !plan.removed[index];
        index += 1;
        keep
    });
    model.endogenous.retain(|declaration| {
        #[cfg(test)]
        DECLARATION_VISITS.with(|visits| visits.set(visits.get() + 1));
        !plan.excluded.contains(&declaration.name)
    });
    let span = Span::new(model.source.len(), model.source.len());
    for (declaration, kind) in plan.changes {
        let name = declaration.name;
        if kind == SymbolKind::Varexo {
            model.exogenous.push(declaration);
        } else {
            model.excluded_endogenous.push(declaration);
        }
        model.symbol_type_events.push(SymbolTypeEvent {
            name,
            span,
            kind,
            changed: true,
        });
    }
}

#[cfg(test)]
fn select(
    model: &mut Model,
    selection: &EquationSelection,
    budget: &mut MacroBudget,
) -> Result<(), Box<Diagnostic>> {
    let prepared = plan(model, selection, budget, &Usage::new(model))?;
    commit(model, prepared);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(count: usize) -> String {
        let mut text = format!(
            "var {}; model;",
            (0..count)
                .map(|index| format!("x{index}"))
                .collect::<Vec<_>>()
                .join(" ")
        );
        for index in 0..count {
            text.push_str(&format!("[name='e{index}'] x{index}=x{index}(-1);"));
        }
        text.push_str("end;");
        text
    }

    #[test]
    fn selection_work_is_linear_and_cannot_commit_after_its_limit() {
        // Guards a selector/row nested scan and any partial membership/type
        // mutation when selection runs out of the root's work allowance.
        for count in [32, 256] {
            let mut model = crate::engine::language::parser::parse(&source(count));
            let selection = EquationSelection {
                mode: EquationSelectionMode::Exclude,
                tag: "name".into(),
                values: vec!["e0".into()],
            };
            let mut budget = MacroBudget::new();
            let before = budget.remaining_work();
            select(&mut model, &selection, &mut budget).unwrap();
            assert!(before - budget.remaining_work() < count * 24);
            assert_eq!(model.equations.len(), count - 1);
            let mut original = crate::engine::language::parser::parse(&source(count));
            let mut stopped = MacroBudget::with_limit(
                crate::engine::macro_work::MacroWorkLimit::new(2).unwrap(),
                None,
            );
            let diagnostic = select(&mut original, &selection, &mut stopped).unwrap_err();
            assert_eq!(diagnostic.code, "I211");
            assert_eq!(diagnostic.severity, Severity::Information);
            assert_eq!(original.equations.len(), count);
            assert_eq!(original.endogenous.len(), count);
            let mut many = crate::engine::language::parser::parse(&source(count));
            let mut include = selection.clone();
            include.mode = EquationSelectionMode::Include;
            DECLARATION_VISITS.with(|visits| visits.set(0));
            select(&mut many, &include, &mut MacroBudget::new()).unwrap();
            assert_eq!(many.endogenous.len(), 1);
            assert_eq!(DECLARATION_VISITS.with(|visits| visits.get()), count);
        }
    }
}
