//! Counted model equations, per-use idents, and the W013 count gap.

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::model::{Complementarity, Equation, Model};
use crate::span::Span;
use crate::timing::{TimingAnalysis, TimingClass};

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
    let timing = TimingAnalysis::new(model);
    let locals = crate::model_locals::ModelLocals::collect(model);
    let mut rows = Vec::new();
    for eq in &model.equations {
        if !is_counted(eq) {
            continue;
        }
        rows.push(equation_row(model, eq, rows.len(), &timing, None, &locals));
    }
    rows
}

pub(crate) fn heterogeneous_equations(model: &Model) -> Vec<HeterogeneousEquationBlockRows> {
    let timing = TimingAnalysis::new(model);
    let locals = crate::model_locals::ModelLocals::collect(model);
    let mut next_index = HashMap::new();
    model
        .heterogeneous_models
        .iter()
        .enumerate()
        .map(|(block_index, block)| {
            let index = next_index.entry(block.dimension).or_insert(0usize);
            let mut rows = Vec::new();
            for eq in &block.equations {
                if is_counted(eq) {
                    rows.push(equation_row(
                        model,
                        eq,
                        *index,
                        &timing,
                        Some(block.dimension),
                        &locals,
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
    timing: &TimingAnalysis,
    heterogeneous_dimension: Option<crate::intern::Name>,
    locals: &crate::model_locals::ModelLocals,
) -> EquationRow {
    let idents = model
        .ident_refs(eq)
        .into_iter()
        .map(|reference| {
            let name = model.name(reference.name).to_string();
            let model_local = locals.uses.iter().any(|usage| {
                usage.name == reference.name
                    && usage.span == reference.span
                    && usage.dimension == heterogeneous_dimension
            });
            let class = identifier_class(model, reference.name, model_local);
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
    let n_equations = collapsed_equation_count(model);
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

fn collapsed_equation_count(model: &Model) -> usize {
    let mut n = 0;
    let mut seen = HashSet::new();
    for row in equations(model) {
        let occbin = !row.name.is_empty()
            && (row.tags.contains_key("bind") || row.tags.contains_key("relax"));
        if occbin {
            if seen.insert(row.name) {
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
    name: crate::intern::Name,
    model_local: bool,
) -> IdentClass {
    if model_local {
        return IdentClass::ModelLocal;
    }
    match model
        .final_symbol_kind(name)
        .or_else(|| model.final_kind_or_written_if_excluded(name))
    {
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

fn equation_refs(model: &Model) -> HashSet<crate::intern::Name> {
    let mut referenced = HashSet::new();
    for eq in model.equations.iter().chain(
        model
            .heterogeneous_models
            .iter()
            .flat_map(|block| block.equations.iter()),
    ) {
        for r in model.ident_refs(eq) {
            referenced.insert(r.name);
        }
    }
    referenced
}
