//! Counted model equations, per-use idents, and the W013 count gap.

use std::collections::{BTreeMap, HashSet};

use crate::model::{Complementarity, Equation, Model};
use crate::model_info::{classify_variable_timing, TimingClass};
use crate::span::Span;

/// One counted model equation (`!is_local && !static_tag`). `index` is 0-based
/// among those rows and is the identity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EquationRow {
    pub index: usize,
    pub name: String,
    pub text: String,
    pub lhs: String,
    pub rhs: String,
    pub span: Span,
    pub static_tag: bool,
    pub dynamic_tag: bool,
    pub idents: Vec<EquationIdent>,
    pub tags: BTreeMap<String, String>,
    pub complementarity: Option<Complementarity>,
}

/// One identifier use in an equation (lhs then rhs; ident nodes only).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EquationIdent {
    pub name: String,
    pub timing: i32,
    pub class: IdentClass,
    pub timing_class: Option<TimingClass>,
}

/// Declaration class, first hit in this order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IdentClass {
    Endogenous,
    Varexo,
    VarexoDet,
    Parameter,
    Undeclared,
}

impl IdentClass {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Endogenous => "endogenous",
            Self::Varexo => "varexo",
            Self::VarexoDet => "varexo_det",
            Self::Parameter => "parameter",
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
    let timing = classify_variable_timing(model);
    let mut rows = Vec::new();
    for eq in &model.equations {
        if !is_counted(eq) {
            continue;
        }
        let index = rows.len();
        let idents = model
            .ident_refs(eq)
            .into_iter()
            .map(|r| {
                let name = model.name(r.name).to_string();
                let class = ident_class(model, r.name);
                let timing_class = if class == IdentClass::Endogenous {
                    timing.get(&name).map(|info| info.class)
                } else {
                    None
                };
                EquationIdent {
                    name,
                    timing: r.timing,
                    class,
                    timing_class,
                }
            })
            .collect();
        rows.push(EquationRow {
            index,
            name: eq.name.clone(),
            text: eq.text.clone(),
            lhs: eq.lhs.clone(),
            rhs: eq.rhs.clone(),
            span: eq.span,
            static_tag: eq.static_tag,
            dynamic_tag: eq.dynamic_tag,
            idents,
            tags: eq.tag_map.clone(),
            complementarity: eq.complementarity.clone(),
        });
    }
    rows
}

pub fn count_gap(model: &Model) -> CountGap {
    let n_equations = collapsed_equation_count(model);
    // The official transform-stage count compares the aggregate tree with the
    // plain-endogenous symbol count (`endo_nbr()` collects `endogenous` only,
    // not `heterogeneousEndogenous`); the per-dimension counts are a separate
    // refusal. Heterogeneous declarations therefore stay out of both sides, so
    // opposing tree gaps cannot cancel.
    let n_endogenous = model
        .endogenous
        .iter()
        .filter(|d| d.heterogeneity.is_none())
        .count();
    CountGap {
        n_endogenous,
        n_equations,
        delta: n_equations as i32 - n_endogenous as i32,
        unreferenced_endogenous: unreferenced_endogenous(model),
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
        "### {title}\n\nindex: {}\nflags: {flags}\nlhs: `{}`\nrhs: `{}`\n\n",
        row.index, row.lhs, row.rhs
    );
    for id in &row.idents {
        out.push_str(&format!(
            "- `{}`: {}, offset {}",
            id.name,
            id.class.as_str(),
            id.timing
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

fn ident_class(model: &Model, name: crate::intern::Name) -> IdentClass {
    if model.endogenous.iter().any(|d| d.name == name) {
        IdentClass::Endogenous
    } else if model.deterministic_exogenous.iter().any(|d| d.name == name) {
        // `parse` clones varexo_det into `exogenous`; classify det first or
        // every det name would look like varexo.
        IdentClass::VarexoDet
    } else if model.exogenous.iter().any(|d| d.name == name) {
        IdentClass::Varexo
    } else if model.parameters.iter().any(|d| d.name == name) {
        IdentClass::Parameter
    } else {
        IdentClass::Undeclared
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

fn unreferenced_endogenous(model: &Model) -> Vec<String> {
    let mut referenced = HashSet::new();
    for eq in &model.equations {
        for r in model.ident_refs(eq) {
            referenced.insert(r.name);
        }
    }
    model
        .endogenous
        .iter()
        .filter(|d| d.heterogeneity.is_none())
        .filter(|d| !referenced.contains(&d.name))
        .map(|d| model.name(d.name).to_string())
        .collect()
}
