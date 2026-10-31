//! Counted model equations, per-use idents, and the W013 count gap.

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::engine::analysis::timing::{TimingAnalysis, TimingClass};
use crate::engine::language::model::{Complementarity, Equation, Model};
use crate::engine::language::span::Span;

/// One counted model equation (`!is_local && !static_tag`). `index` is 0-based
/// among those rows and is the identity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EquationRow {
    pub index: usize,
    pub name: String,
    pub text: String,
    pub span: Span,
    pub static_tag: bool,
    pub dynamic_tag: bool,
    pub idents: Vec<EquationIdent>,
    pub tags: BTreeMap<String, String>,
    pub complementarity: Option<Complementarity>,
}

/// Written equations from one heterogeneous model block. Equation indices
/// continue across blocks of the same dimension.
pub(crate) struct HeterogeneousEquationBlockRows {
    pub block_index: usize,
    pub dimension: String,
    pub equations: Vec<EquationRow>,
}

/// One identifier use in an equation (lhs then rhs; ident nodes only).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EquationIdent {
    pub name: String,
    /// Written offset in the equation's source.
    pub timing: i32,
    /// Offset after predetermined-variable convention conversion only.
    pub dynare_timing: i32,
    pub class: IdentClass,
    pub timing_class: Option<TimingClass>,
}

/// Final symbol class after successful type changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IdentClass {
    Endogenous,
    Varexo,
    VarexoDet,
    Parameter,
    ModelLocal,
    Undeclared,
}

impl IdentClass {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Endogenous => "endogenous",
            Self::Varexo => "varexo",
            Self::VarexoDet => "varexo_det",
            Self::Parameter => "parameter",
            Self::ModelLocal => "model_local",
            Self::Undeclared => "undeclared",
        }
    }
}

/// Equation vs endogenous counts. `delta = n_equations − n_endogenous`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CountGap {
    pub n_endogenous: usize,
    pub n_equations: usize,
    pub delta: i32,
    pub unreferenced_endogenous: Vec<String>,
    pub expected_delta: Option<i32>,
}

pub fn equations(model: &Model) -> Vec<EquationRow> {
    aggregate_rows(model, &EquationFacts::collect(model))
}

struct EquationFacts {
    timing: TimingAnalysis,
    local_sites: HashSet<(
        crate::engine::language::intern::Name,
        Span,
        Option<crate::engine::language::intern::Name>,
    )>,
}

#[cfg(test)]
thread_local! {
    pub(crate) static EQUATION_FACT_BUILDS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

impl EquationFacts {
    fn collect(model: &Model) -> Self {
        #[cfg(test)]
        EQUATION_FACT_BUILDS.with(|count| count.set(count.get() + 1));
        let timing = TimingAnalysis::new(model);
        let locals = crate::engine::analysis::model_locals::ModelLocals::collect(model);
        let local_sites = locals
            .uses
            .into_iter()
            .take_while(|_| !crate::engine::control::cancelled())
            .map(|usage| (usage.name, usage.span, usage.dimension))
            .collect();
        Self {
            timing,
            local_sites,
        }
    }
}

/// Both equation scopes use one final-type, timing and model-local pass.
pub(crate) fn equation_collections(
    model: &Model,
) -> (Vec<EquationRow>, Vec<HeterogeneousEquationBlockRows>) {
    let facts = EquationFacts::collect(model);
    (
        aggregate_rows(model, &facts),
        heterogeneous_rows(model, &facts),
    )
}

fn aggregate_rows(model: &Model, facts: &EquationFacts) -> Vec<EquationRow> {
    let mut rows = Vec::new();
    for eq in &model.equations {
        if crate::engine::control::cancelled() {
            break;
        }
        if !is_counted(eq) {
            continue;
        }
        rows.push(equation_row(model, eq, rows.len(), None, facts));
    }
    rows
}

pub(crate) fn heterogeneous_equations(model: &Model) -> Vec<HeterogeneousEquationBlockRows> {
    if model.heterogeneous_models.is_empty() {
        return Vec::new();
    }
    heterogeneous_rows(model, &EquationFacts::collect(model))
}

fn heterogeneous_rows(model: &Model, facts: &EquationFacts) -> Vec<HeterogeneousEquationBlockRows> {
    let mut next_index = HashMap::new();
    model
        .heterogeneous_models
        .iter()
        .enumerate()
        .map(|(block_index, block)| {
            let index = next_index.entry(block.dimension).or_insert(0usize);
            let mut rows = Vec::new();
            for eq in &block.equations {
                if crate::engine::control::cancelled() {
                    break;
                }
                if is_counted(eq) {
                    rows.push(equation_row(
                        model,
                        eq,
                        *index,
                        Some(block.dimension),
                        facts,
                    ));
                    *index += 1;
                }
            }
            HeterogeneousEquationBlockRows {
                block_index,
                dimension: model.name(block.dimension).to_string(),
                equations: rows,
            }
        })
        .collect()
}

fn equation_row(
    model: &Model,
    eq: &Equation,
    index: usize,
    heterogeneous_dimension: Option<crate::engine::language::intern::Name>,
    facts: &EquationFacts,
) -> EquationRow {
    let timing = &facts.timing;
    let idents = model
        .ident_refs(eq)
        .into_iter()
        .map(|reference| {
            let name = model.name(reference.name).to_string();
            let model_local = facts.local_sites.contains(&(
                reference.name,
                reference.span,
                heterogeneous_dimension,
            ));
            let kind = timing
                .symbols
                .kind(reference.name)
                .or_else(|| model.final_kind_or_written_if_excluded(reference.name));
            let class = identifier_kind_class(kind, model_local);
            let timing_class = if class == IdentClass::Endogenous {
                let classes = if heterogeneous_dimension.is_some() {
                    &timing.all
                } else {
                    &timing.aggregate
                };
                classes.get(&name).map(|info| info.class)
            } else {
                None
            };
            EquationIdent {
                name,
                timing: reference.timing,
                dynare_timing: if heterogeneous_dimension.is_some() {
                    reference.timing
                } else {
                    timing.aggregate_dynare_offset(reference.name, reference.timing)
                },
                class,
                timing_class,
            }
        })
        .collect();
    EquationRow {
        index,
        name: eq.name.clone(),
        text: eq.text.clone(),
        span: eq.span,
        static_tag: eq.static_tag,
        dynamic_tag: eq.dynamic_tag,
        idents,
        tags: eq.tag_map.clone(),
        complementarity: eq.complementarity.clone(),
    }
}

pub fn count_gap(model: &Model) -> CountGap {
    count_gap_with_rows(model, &equations(model))
}

/// Reuse the aggregate rows already owned by an equation query.
pub(crate) fn count_gap_with_rows(model: &Model, rows: &[EquationRow]) -> CountGap {
    let n_equations = collapsed_counted_rows(rows);
    let referenced = equation_refs(model);
    // The official transform-stage count compares the aggregate tree with the
    // plain-endogenous symbol count (`endo_nbr()` collects `endogenous` only,
    // not `heterogeneousEndogenous`); the per-dimension counts are a separate
    // refusal. Heterogeneous declarations therefore stay out of both sides, so
    // opposing tree gaps cannot cancel. Names count by final type after
    // `change_type`, like `endo_nbr()`.
    let final_endogenous = model.final_endogenous();
    let n_endogenous = final_endogenous.len();
    CountGap {
        n_endogenous,
        n_equations,
        delta: n_equations as i32 - n_endogenous as i32,
        unreferenced_endogenous: final_endogenous
            .iter()
            .filter(|d| !referenced.contains(&d.name))
            .map(|d| model.name(d.name).to_string())
            .collect(),
        expected_delta: planner_expected_delta(model),
    }
}

pub fn explain_equation(row: &EquationRow) -> String {
    let title = if row.name.is_empty() {
        "unnamed"
    } else {
        row.name.as_str()
    };
    let flags = match (row.static_tag, row.dynamic_tag) {
        (false, false) => "(none)".to_string(),
        (true, false) => "static".to_string(),
        (false, true) => "dynamic".to_string(),
        (true, true) => "static, dynamic".to_string(),
    };
    let mut out = format!(
        "### {title}\n\nindex: {}\nflags: {flags}\n\nDynare offsets apply only the predetermined-variable convention conversion.\n\n",
        row.index
    );
    for id in &row.idents {
        if crate::engine::control::cancelled() {
            break;
        }
        out.push_str(&format!(
            "- `{}`: {}, written offset {}, Dynare offset {}",
            id.name,
            id.class.as_str(),
            id.timing,
            id.dynare_timing
        ));
        if let Some(tc) = id.timing_class {
            out.push_str(", ");
            out.push_str(tc.label());
        }
        out.push('\n');
    }
    out.pop();
    out
}

fn is_counted(eq: &Equation) -> bool {
    !eq.is_local && !eq.static_tag
}

/// Reuse accepted equation rows when a caller already owns their analysis.
pub(crate) fn collapsed_counted_rows(rows: &[EquationRow]) -> usize {
    let mut n = 0;
    let mut seen = HashSet::new();
    for row in rows {
        if crate::engine::control::cancelled() {
            break;
        }
        let occbin = !row.name.is_empty()
            && (row.tags.contains_key("bind") || row.tags.contains_key("relax"));
        if occbin {
            if seen.insert(row.name.as_str()) {
                n += 1;
            }
        } else {
            n += 1;
        }
    }
    n
}

pub(crate) fn identifier_class(
    model: &Model,
    name: crate::engine::language::intern::Name,
    model_local: bool,
) -> IdentClass {
    if model_local {
        return IdentClass::ModelLocal;
    }
    identifier_kind_class(
        model
            .final_symbol_kind(name)
            .or_else(|| model.final_kind_or_written_if_excluded(name)),
        model_local,
    )
}

fn identifier_kind_class(kind: Option<&str>, model_local: bool) -> IdentClass {
    if model_local {
        return IdentClass::ModelLocal;
    }
    match kind {
        Some("var") => IdentClass::Endogenous,
        Some("varexo_det") => IdentClass::VarexoDet,
        Some("varexo") => IdentClass::Varexo,
        Some("parameters") => IdentClass::Parameter,
        _ => IdentClass::Undeclared,
    }
}

fn planner_expected_delta(model: &Model) -> Option<i32> {
    let has_planner = model.policy_commands.iter().any(|c| c.is_planner());
    if has_planner && !model.instruments.is_empty() {
        Some(-(model.instruments.len() as i32))
    } else {
        None
    }
}

fn equation_refs(model: &Model) -> HashSet<crate::engine::language::intern::Name> {
    let mut referenced = HashSet::new();
    for eq in model.equations.iter().chain(
        model
            .heterogeneous_models
            .iter()
            .flat_map(|block| block.equations.iter()),
    ) {
        for r in model.ident_refs(eq) {
            if crate::engine::control::cancelled() {
                break;
            }
            referenced.insert(r.name);
        }
    }
    referenced
}
