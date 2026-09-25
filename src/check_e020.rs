//! E023 / E024 / E025 / E020 undeclared-and-related diagnostics.

use std::collections::{HashMap, HashSet};

use crate::diagnostic::{Diagnostic, Severity};
use crate::intern::Name;
use crate::model::{Decl, Equation, Model};
use crate::span::Span;

pub fn check_e020(model: &Model) -> Vec<Diagnostic> {
    let mut out = check_e023(model);
    out.extend(check_e024(model));
    out.extend(check_e025(model));
    out.extend(check_undeclared_equations(model));
    out
}

/// Aggregate-written equations plus every per-dimension heterogeneous body, in
/// file order. The official preprocessor resolves symbols while it parses each
/// body, so the shipped walkers see both (probe `r32`: an unknown symbol in a
/// heterogeneous equation refuses at parse).
pub(crate) fn all_model_equations(model: &Model) -> Vec<&Equation> {
    let mut eqs: Vec<&Equation> = model.equations.iter().collect();
    eqs.extend(
        model
            .heterogeneous_models
            .iter()
            .flat_map(|block| block.equations.iter()),
    );
    eqs
}

/// One data tree per list: the aggregate model, then each heterogeneous block.
/// `AddLocalVariable` is per tree, so a `#` name may be defined once in each.
pub(crate) fn equation_trees(model: &Model) -> Vec<Vec<&Equation>> {
    let mut agg: Vec<&Equation> = model.equations.iter().collect();
    agg.sort_by_key(|eq| (eq.span.start, eq.span.end));
    let mut trees = vec![agg];
    for block in &model.heterogeneous_models {
        let mut eqs: Vec<&Equation> = block.equations.iter().collect();
        eqs.sort_by_key(|eq| (eq.span.start, eq.span.end));
        trees.push(eqs);
    }
    trees
}

fn local_names(model: &Model, eqs: &[&Equation]) -> HashSet<Name> {
    eqs.iter()
        .filter_map(|eq| model_local_name(model, eq).map(|(name, _)| name))
        .collect()
}

/// Earliest `#` definition of each name, across every tree.
fn earliest_local_defs(model: &Model, trees: &[Vec<&Equation>]) -> HashMap<Name, u32> {
    let mut earliest = HashMap::new();
    for tree in trees {
        for eq in tree {
            let Some((name, _)) = model_local_name(model, eq) else {
                continue;
            };
            earliest
                .entry(name)
                .and_modify(|start: &mut u32| {
                    if eq.span.start < *start {
                        *start = eq.span.start;
                    }
                })
                .or_insert(eq.span.start);
        }
    }
    earliest
}

fn check_e023(model: &Model) -> Vec<Diagnostic> {
    let endo: HashSet<Name> = model.endogenous.iter().map(|d| d.name).collect();
    let mut seen = HashSet::new();
    let mut diagnostics = Vec::new();
    for decl in &model.predetermined {
        if !seen.insert(decl.name) {
            continue;
        }
        if endo.contains(&decl.name) {
            continue;
        }
        let name = model.name(decl.name);
        diagnostics.push(Diagnostic {
            span: decl.span,
            severity: Severity::Error,
            code: "E023".to_string(),
            message: format!(
                "Predetermined variable '{name}' is not declared as an endogenous variable."
            ),
            fix: None,
            tags: Vec::new(),
        });
    }
    diagnostics
}

fn check_e024(model: &Model) -> Vec<Diagnostic> {
    let det: HashSet<Name> = model
        .deterministic_exogenous
        .iter()
        .map(|d| d.name)
        .collect();
    if det.is_empty() {
        return Vec::new();
    }
    let mut seen = HashSet::new();
    let mut diagnostics = Vec::new();
    for eq in all_model_equations(model) {
        for r in model.ident_refs(eq) {
            if r.timing == 0 || !det.contains(&r.name) || !seen.insert(r.name) {
                continue;
            }
            let end = r.timing_span.map(|t| t.end).unwrap_or(r.span.end);
            let name = model.name(r.name);
            diagnostics.push(Diagnostic {
                span: Span {
                    start: r.span.start,
                    end,
                },
                severity: Severity::Error,
                code: "E024".to_string(),
                message: format!(
                    "Exogenous deterministic variable {name} cannot be given a lead or a lag"
                ),
                fix: None,
                tags: Vec::new(),
            });
        }
    }
    diagnostics
}

fn check_e025(model: &Model) -> Vec<Diagnostic> {
    let declared = declared_symbol_names(model);
    let mut eqs = all_model_equations(model);
    eqs.sort_by_key(|eq| (eq.span.start, eq.span.end));

    let mut seen_shadowing = HashSet::new();
    let mut diagnostics = Vec::new();

    for eq in &eqs {
        let Some((name, span)) = model_local_name(model, eq) else {
            continue;
        };
        if declared.contains(&name) && seen_shadowing.insert(name) {
            diagnostics.push(shadowing_diag(model, name, span));
        }
    }

    // The pound-LHS refusal is per tree. A definition in an earlier tree has
    // already made the name a model local, so a later tree may use it before
    // its own definition. A use before any definition, in the tree that then
    // defines it, still refuses.
    let trees = equation_trees(model);
    let earliest = earliest_local_defs(model, &trees);
    for tree in &trees {
        let first_definition = local_def_starts(model, tree);
        let mut visible_locals = HashSet::new();
        let mut seen_early = HashSet::new();
        for eq in tree {
            let local = model_local_name(model, eq).map(|(n, _)| n);
            for r in model.ident_refs(eq) {
                if local == Some(r.name) {
                    continue;
                }
                let Some(&def_start) = first_definition.get(&r.name) else {
                    continue;
                };
                if visible_locals.contains(&r.name)
                    || declared.contains(&r.name)
                    || seen_early.contains(&r.name)
                {
                    continue;
                }
                if eq.span.start >= def_start {
                    continue;
                }
                if earliest
                    .get(&r.name)
                    .is_some_and(|start| *start < def_start)
                {
                    continue;
                }
                seen_early.insert(r.name);
                let name = model.name(r.name);
                diagnostics.push(Diagnostic {
                    span: r.span,
                    severity: Severity::Error,
                    code: "E025".to_string(),
                    message: format!(
                        "{name} has wrong type or was already used on the right-hand side. You cannot use it on the left-hand side of a pound ('#') expression"
                    ),
                    fix: None,
                    tags: Vec::new(),
                });
            }
            if let Some(n) = local {
                visible_locals.insert(n);
            }
        }
    }

    for eq in &model.steady_state_equations {
        let Some((name, span)) = model_local_name(model, eq) else {
            continue;
        };
        if !declared.contains(&name) || !seen_shadowing.insert(name) {
            continue;
        }
        diagnostics.push(shadowing_diag(model, name, span));
    }
    diagnostics
}

fn shadowing_diag(model: &Model, name: Name, span: Span) -> Diagnostic {
    let name = model.name(name);
    Diagnostic {
        span,
        severity: Severity::Error,
        code: "E025".to_string(),
        message: format!(
            "{name} has wrong type or was already used on the right-hand side. You cannot use it on the left-hand side of a pound ('#') expression"
        ),
        fix: None,
        tags: Vec::new(),
    }
}

fn check_undeclared_equations(model: &Model) -> Vec<Diagnostic> {
    let local_declared = all_declared_name_strings(model);
    // Equations a surgery statement removed still refuse an undeclared name: 7.1 resolves
    // symbols while it parses the model block, before the removal statement runs.
    let removed_equations = || {
        model
            .equation_surgery
            .iter()
            .flat_map(|surgery| surgery.removed.iter().map(|row| &row.equation))
    };
    // A `#` local is visible only inside its own tree. A name defined in
    // another tree before this use crashes 7.2 with no sentence
    // (`UnknownLocalVariableException`); a use before every definition is
    // `Unknown symbol`.
    let trees = equation_trees(model);
    let mut pounds: Vec<HashSet<Name>> =
        trees.iter().map(|tree| local_names(model, tree)).collect();
    for eq in removed_equations() {
        if let Some((name, _)) = model_local_name(model, eq) {
            pounds[0].insert(name);
        }
    }
    let earliest = earliest_local_defs(model, &trees);
    let shocks: HashSet<Name> = model.shocks_vars.iter().copied().collect();
    let assigned: HashSet<Name> = model
        .param_assignments
        .iter()
        .chain(&model.helper_assignments)
        .map(|a| a.name)
        .collect();
    let exo_names: HashSet<String> = model
        .exogenous
        .iter()
        .map(|d| model.name(d.name).to_string())
        .collect();
    let endo_names: HashSet<String> = model
        .endogenous
        .iter()
        .map(|d| model.name(d.name).to_string())
        .collect();
    let param_names: HashSet<String> = model
        .parameters
        .iter()
        .map(|d| model.name(d.name).to_string())
        .collect();
    let exo_underscore = exo_names.iter().any(|e| e.ends_with('_'));

    let mut eqs: Vec<(&Equation, bool)> = model
        .equations
        .iter()
        .map(|eq| (eq, false))
        .chain(
            model
                .heterogeneous_models
                .iter()
                .flat_map(|block| block.equations.iter())
                .map(|eq| (eq, false)),
        )
        .chain(removed_equations().map(|eq| (eq, true)))
        .collect();
    eqs.sort_by_key(|(eq, _)| (eq.span.start, eq.span.end));

    let mut seen = HashSet::new();
    let mut diagnostics = Vec::new();
    for (eq, removed) in eqs {
        let visible = visible_names(model, eq);
        for r in model.ident_refs(eq) {
            let ref_name = model.name(r.name);
            if is_skipped_ref(ref_name) {
                continue;
            }
            // The declaration was live when this removed equation was written.
            if removed && model.dropped_by_surgery_after(r.name, eq.span.start) {
                continue;
            }
            if visible.contains(&r.name)
                || local_hides_unknown(model, eq, r.name, r.span.start, &pounds, &earliest)
            {
                continue;
            }
            if model.mod_file_locals.contains(&r.name)
                || model.external_function_names.contains(&r.name)
            {
                continue;
            }
            if ref_name == "_" || ref_name.ends_with("__") {
                continue;
            }
            if !seen.insert(r.name) {
                continue;
            }
            diagnostics.push(undeclared_diag(
                eq,
                r.span,
                ref_name,
                r.name,
                UndeclaredNames {
                    local: &local_declared,
                    shocks: &shocks,
                    assigned: &assigned,
                    exo: &exo_names,
                    endo: &endo_names,
                    params: &param_names,
                    exo_underscore,
                },
            ));
        }
    }
    diagnostics
}

struct UndeclaredNames<'a> {
    local: &'a HashSet<String>,
    shocks: &'a HashSet<Name>,
    assigned: &'a HashSet<Name>,
    exo: &'a HashSet<String>,
    endo: &'a HashSet<String>,
    params: &'a HashSet<String>,
    exo_underscore: bool,
}

fn undeclared_diag(
    eq: &Equation,
    span: Span,
    ref_name: &str,
    ref_id: Name,
    names: UndeclaredNames<'_>,
) -> Diagnostic {
    let mut msg = format!("Undeclared identifier '{ref_name}' in equation");
    if !eq.name.is_empty() {
        msg.push_str(" '");
        msg.push_str(&eq.name);
        msg.push('\'');
    }
    msg.push('.');

    let similar = find_similar_names(ref_name, names.local);
    if !similar.is_empty() {
        msg.push_str(" Did you mean: ");
        msg.push_str(&similar.join(", "));
        msg.push('?');
    }

    let ref_matches_exo_pattern = ref_name.ends_with('_') && names.exo_underscore;
    let definite_decl = names.shocks.contains(&ref_id)
        || names.assigned.contains(&ref_id)
        || ref_matches_exo_pattern;

    let mut is_typo_replace = false;
    if !similar.is_empty() && !definite_decl {
        let best = &similar[0];
        if prefix_hamming_diffs(ref_name, best) <= 2 {
            is_typo_replace = true;
            msg = format!("Undeclared identifier '{ref_name}' in equation");
            if !eq.name.is_empty() {
                msg.push_str(" '");
                msg.push_str(&eq.name);
                msg.push('\'');
            }
            msg.push_str(&format!(
                ". This is likely a typo for '{best}'. Fix: replace '{ref_name}' with '{best}'."
            ));
        }
    }

    if !is_typo_replace {
        if names.shocks.contains(&ref_id) {
            msg.push_str(&format!(
                " Since '{ref_name}' is referenced in the shocks block, it is an exogenous variable. Fix: add '{ref_name}' to the 'varexo' declaration."
            ));
        } else if ref_matches_exo_pattern {
            let mut exo_sorted: Vec<&String> = names.exo.iter().collect();
            exo_sorted.sort();
            let sample: Vec<&str> = exo_sorted.iter().take(2).map(|s| s.as_str()).collect();
            msg.push_str(&format!(
                " Since '{ref_name}' follows the naming pattern of exogenous shock variables (like {}), it is likely an exogenous variable. Fix: add '{ref_name}' to the 'varexo' declaration.",
                sample.join(", ")
            ));
        } else if names.assigned.contains(&ref_id) {
            msg.push_str(&format!(
                " Since '{ref_name}' has a value assignment in the file, it is likely a parameter. Fix: add '{ref_name}' to the 'parameters' declaration."
            ));
        } else if !similar.is_empty() {
            let sim_in_exo = similar.iter().all(|s| names.exo.contains(s));
            let sim_in_params = similar.iter().all(|s| names.params.contains(s));
            let sim_in_endo = similar.iter().all(|s| names.endo.contains(s));
            if sim_in_exo {
                msg.push_str(&format!(
                    " Similar declared names ({}) are all exogenous variables. Fix: add '{ref_name}' to the 'varexo' declaration.",
                    similar.join(", ")
                ));
            } else if sim_in_params {
                msg.push_str(&format!(
                    " Similar declared names ({}) are all parameters. Fix: add '{ref_name}' to the 'parameters' declaration.",
                    similar.join(", ")
                ));
            } else if sim_in_endo {
                msg.push_str(&format!(
                    " Similar declared names ({}) are all endogenous variables. Fix: add '{ref_name}' to the 'var' declaration.",
                    similar.join(", ")
                ));
            } else {
                msg.push_str(&format!(
                    " Fix: replace '{ref_name}' with the correct name, or add '{ref_name}' to a var/varexo/parameters declaration."
                ));
            }
        } else {
            msg.push_str(&format!(
                " Fix: add '{ref_name}' to a var, varexo, or parameters declaration."
            ));
        }
    }

    Diagnostic {
        span,
        severity: Severity::Error,
        code: "E020".to_string(),
        message: msg,
        fix: None,
        tags: Vec::new(),
    }
}

fn visible_names(model: &Model, eq: &Equation) -> HashSet<Name> {
    let nested_ok = |decl: &Decl| {
        model
            .model_block
            .is_some_and(|block| nested_or_equal(block, decl.span))
    };
    let mut visible = HashSet::new();
    for decl in visibility_decls(model) {
        if nested_ok(decl) || decl.span.start <= eq.span.start {
            visible.insert(decl.name);
        }
    }
    // Trend variables and epilogue names are not `Decl`s but the model block
    // may still use each one from its declaration onward.
    for (name, span) in model
        .trend_vars
        .iter()
        .map(|t| (t.name, t.span))
        .chain(model.epilogue.iter().map(|a| (a.name, a.span)))
    {
        if span.start <= eq.span.start {
            visible.insert(name);
        }
    }
    visible
}

fn visibility_decls(model: &Model) -> Vec<&Decl> {
    let mut decls = Vec::new();
    decls.extend(&model.endogenous);
    decls.extend(&model.exogenous);
    decls.extend(&model.deterministic_exogenous);
    decls.extend(&model.parameters);
    decls.extend(&model.predetermined);
    decls
}

fn nested_or_equal(outer: Span, inner: Span) -> bool {
    outer.start <= inner.start && inner.end <= outer.end
}

fn declared_symbol_names(model: &Model) -> HashSet<Name> {
    model
        .endogenous
        .iter()
        .chain(&model.exogenous)
        .chain(&model.parameters)
        .map(|d| d.name)
        .collect()
}

fn all_declared_name_strings(model: &Model) -> HashSet<String> {
    model
        .endogenous
        .iter()
        .chain(&model.exogenous)
        .chain(&model.parameters)
        .map(|d| model.name(d.name).to_string())
        .collect()
}

/// `true` when `name` is not an unknown symbol in `eq`'s tree.
///
/// Same-tree `#` definitions hide it (an early use is E025, not E020). A
/// definition in another tree that starts before this use is the crash shape
/// with no official sentence, so it stays quiet too.
fn local_hides_unknown(
    model: &Model,
    eq: &Equation,
    name: Name,
    use_start: u32,
    pounds: &[HashSet<Name>],
    earliest: &HashMap<Name, u32>,
) -> bool {
    let tree = equation_tree_index(model, eq);
    if pounds.get(tree).is_some_and(|pound| pound.contains(&name)) {
        return true;
    }
    earliest
        .get(&name)
        .is_some_and(|def_start| *def_start <= use_start)
}

fn equation_tree_index(model: &Model, eq: &Equation) -> usize {
    model
        .heterogeneous_models
        .iter()
        .position(|block| block.equations.iter().any(|other| std::ptr::eq(other, eq)))
        .map(|index| index + 1)
        .unwrap_or(0)
}

fn local_def_starts(model: &Model, eqs: &[&Equation]) -> HashMap<Name, u32> {
    let mut first = HashMap::new();
    for eq in eqs {
        let Some((name, _)) = model_local_name(model, eq) else {
            continue;
        };
        first.entry(name).or_insert(eq.span.start);
    }
    first
}

fn model_local_name(model: &Model, eq: &Equation) -> Option<(Name, Span)> {
    if !eq.text.trim().starts_with('#') {
        return None;
    }
    let id = eq.lhs_expr?;
    model.exprs.walk_idents(id).next().map(|r| (r.name, r.span))
}

fn find_similar_names(name: &str, declared: &HashSet<String>) -> Vec<String> {
    let name_lower = name.to_lowercase();
    let name_len = name.chars().count();
    let mut declared_sorted: Vec<&String> = declared.iter().collect();
    declared_sorted.sort();
    let mut candidates: Vec<(usize, String)> = Vec::new();
    for d in declared_sorted {
        let d_lower = d.to_lowercase();
        if d_lower == name_lower {
            continue;
        }
        let d_len = d.chars().count();
        let len_delta = name_len.abs_diff(d_len);
        if len_delta <= 2 {
            let diffs = prefix_hamming_diffs(name, d);
            if diffs <= 2 {
                candidates.push((diffs, d.clone()));
            }
        } else if name_lower.starts_with(&d_lower) || d_lower.starts_with(&name_lower) {
            candidates.push((len_delta, d.clone()));
        }
    }
    candidates.sort();
    candidates.into_iter().take(3).map(|(_, d)| d).collect()
}

fn prefix_hamming_diffs(name: &str, other: &str) -> usize {
    let name_lower: Vec<char> = name.to_lowercase().chars().collect();
    let other_lower: Vec<char> = other.to_lowercase().chars().collect();
    let mismatches = name_lower
        .iter()
        .zip(other_lower.iter())
        .filter(|(a, b)| a != b)
        .count();
    mismatches + name.chars().count().abs_diff(other.chars().count())
}

fn is_skipped_ref(name: &str) -> bool {
    const BUILTINS: &[&str] = &[
        "exp",
        "log",
        "ln",
        "log2",
        "log10",
        "sqrt",
        "cbrt",
        "abs",
        "sign",
        "sin",
        "cos",
        "tan",
        "asin",
        "acos",
        "atan",
        "sinh",
        "cosh",
        "tanh",
        "asinh",
        "acosh",
        "atanh",
        "floor",
        "ceil",
        "round",
        "min",
        "max",
        "normpdf",
        "normcdf",
        "norminv",
        "logncdf",
        "erf",
        "erfc",
        "inf",
        "nan",
        "steady_state",
        "expectation",
        "pac_expectation",
        "diff",
        "adl",
        "end",
    ];
    BUILTINS.iter().any(|b| name.eq_ignore_ascii_case(b))
}
