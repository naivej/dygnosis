//! Shared timing facts after the predetermined-variable convention conversion.
//! The expression tree and diagnostic readers retain their written offsets.

use std::collections::{HashMap, HashSet};

use crate::engine::language::intern::Name;
use crate::engine::language::model::{Equation, FinalSymbolTypes, Model};

/// Combined dynamic lead/lag class. This is not a solver classification.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimingClass {
    Static,
    Predetermined,
    ForwardLooking,
    Mixed,
}

impl TimingClass {
    pub fn label(self) -> &'static str {
        match self {
            Self::Static => "static",
            Self::Predetermined => "predetermined",
            Self::ForwardLooking => "forward-looking",
            Self::Mixed => "mixed",
        }
    }
}

/// Dynamic offsets after convention conversion, before other transformations.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TimingInfo {
    pub class: TimingClass,
    pub offsets: Vec<i32>,
    /// A valid predetermined_variables declaration selects the convention.
    /// The declaration remains active even when there is no dynamic use.
    pub predetermined_convention: bool,
}

pub(crate) struct TimingAnalysis {
    marked: HashSet<Name>,
    pub aggregate: HashMap<String, TimingInfo>,
    pub all: HashMap<String, TimingInfo>,
    pub symbols: FinalSymbolTypes,
}

impl TimingAnalysis {
    pub fn new(model: &Model) -> Self {
        // A valid later mark restores the convention after a successful type
        // change cleared it. Captured contexts, unlike spans, order macro copies.
        let marked = model
            .predetermined
            .iter()
            .filter(|decl| {
                model.symbol_kind_in_context(decl.name, decl.symbol_type_context) == Some("var")
                    && !model.heterogeneous_in_context(decl.name, decl.symbol_type_context)
                    && !model.symbol_type_events[decl.symbol_type_context.index()..]
                        .iter()
                        .any(|event| event.name == decl.name && event.kind.as_str() != "var")
            })
            .map(|decl| decl.name)
            .collect();
        let mut analysis = Self {
            marked,
            aggregate: HashMap::new(),
            all: HashMap::new(),
            symbols: FinalSymbolTypes::collect(model),
        };
        let mut aggregate_offsets = HashMap::new();
        for equation in &model.equations {
            if crate::engine::control::cancelled() {
                break;
            }
            analysis.record_aggregate(model, equation, &mut aggregate_offsets);
        }
        let declarations = analysis.symbols.declarations(model, &["var"]);
        let dimensions: HashMap<Name, Name> = declarations
            .iter()
            .filter_map(|declaration| {
                analysis
                    .symbols
                    .heterogeneity(declaration)
                    .map(|dimension| (declaration.name, dimension))
            })
            .collect();
        let mut heterogeneous_offsets: HashMap<Name, HashSet<i32>> = HashMap::new();
        for block in &model.heterogeneous_models {
            if crate::engine::control::cancelled() {
                break;
            }
            for equation in &block.equations {
                if crate::engine::control::cancelled() {
                    break;
                }
                if !equation.static_tag {
                    for reference in model.ident_refs(equation) {
                        if crate::engine::control::cancelled() {
                            break;
                        }
                        if dimensions.get(&reference.name) == Some(&block.dimension) {
                            heterogeneous_offsets
                                .entry(reference.name)
                                .or_default()
                                .insert(reference.timing);
                        }
                    }
                }
            }
        }
        for declaration in declarations {
            if crate::engine::control::cancelled() {
                break;
            }
            let name = model.name(declaration.name).to_string();
            if analysis.symbols.heterogeneity(declaration).is_none() {
                let mut timing = classify_offsets(aggregate_offsets.get(&declaration.name));
                timing.predetermined_convention = analysis.is_predetermined(declaration.name);
                analysis.aggregate.insert(name.clone(), timing.clone());
                analysis.all.insert(name, timing);
            } else {
                analysis.all.insert(
                    name,
                    classify_offsets(heterogeneous_offsets.get(&declaration.name)),
                );
            }
        }
        analysis
    }

    pub fn aggregate_dynare_offset(&self, name: Name, written_offset: i32) -> i32 {
        if self.marked.contains(&name) {
            written_offset.saturating_sub(1)
        } else {
            written_offset
        }
    }

    /// Effective convention after accepted type changes, using the same mark
    /// selection as every converted aggregate identifier offset.
    pub(crate) fn is_predetermined(&self, name: Name) -> bool {
        self.marked.contains(&name)
    }

    fn record_aggregate(
        &self,
        model: &Model,
        equation: &Equation,
        offsets: &mut HashMap<Name, HashSet<i32>>,
    ) {
        // Dynare shifts local definitions once, but does not shift or count
        // static-only replacement equations in dynamic timing.
        if equation.static_tag {
            return;
        }
        for reference in model.ident_refs(equation) {
            if crate::engine::control::cancelled() {
                break;
            }
            let dynare_offset = self.aggregate_dynare_offset(reference.name, reference.timing);
            offsets
                .entry(reference.name)
                .or_default()
                .insert(dynare_offset);
        }
    }
}

fn classify_offsets(offsets: Option<&HashSet<i32>>) -> TimingInfo {
    let mut offsets: Vec<i32> = offsets.into_iter().flatten().copied().collect();
    offsets.sort_unstable();
    let has_lead = offsets.iter().any(|&dynare_offset| dynare_offset > 0);
    let has_lag = offsets.iter().any(|&dynare_offset| dynare_offset < 0);
    let class = match (has_lead, has_lag) {
        (true, true) => TimingClass::Mixed,
        (true, false) => TimingClass::ForwardLooking,
        (false, true) => TimingClass::Predetermined,
        (false, false) => TimingClass::Static,
    };
    if offsets.is_empty() {
        offsets.push(0);
    }
    TimingInfo {
        class,
        offsets,
        predetermined_convention: false,
    }
}

/// Shared display classes: ordinary names use their aggregate model; dimension
/// names use only blocks of their own dimension.
pub fn classify_variable_timing(model: &Model) -> HashMap<String, TimingInfo> {
    TimingAnalysis::new(model).all
}

/// Aggregate endogenous, timing-class, and varexo counts. No BK; no Python labels.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StructureSummary {
    pub endogenous: usize,
    pub predetermined: usize,
    pub forward_looking: usize,
    pub static_vars: usize,
    pub varexo: usize,
    pub max_lead: i32,
    pub max_lag: i32,
}

/// Predetermined / forward-looking / static counts for the aggregate model.
pub fn structure_summary(model: &Model) -> StructureSummary {
    let analysis = TimingAnalysis::new(model);
    let timing = &analysis.aggregate;
    let mut predetermined = 0;
    let mut forward_looking = 0;
    let mut static_vars = 0;
    let mut max_lead = 0;
    let mut max_lag = 0;
    for info in timing.values() {
        if crate::engine::control::cancelled() {
            break;
        }
        match info.class {
            TimingClass::Static => static_vars += 1,
            TimingClass::Predetermined => predetermined += 1,
            TimingClass::ForwardLooking => forward_looking += 1,
            TimingClass::Mixed => {
                predetermined += 1;
                forward_looking += 1;
            }
        }
        for &o in &info.offsets {
            if crate::engine::control::cancelled() {
                break;
            }
            if o > max_lead {
                max_lead = o;
            }
            if o < max_lag {
                max_lag = o;
            }
        }
    }
    StructureSummary {
        endogenous: analysis
            .symbols
            .declarations(model, &["var"])
            .into_iter()
            .filter(|decl| analysis.symbols.heterogeneity(decl).is_none())
            .count(),
        predetermined,
        forward_looking,
        static_vars,
        varexo: analysis
            .symbols
            .declarations(model, &["varexo", "varexo_det"])
            .into_iter()
            .filter(|decl| analysis.symbols.heterogeneity(decl).is_none())
            .count(),
        max_lead,
        max_lag,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Guards rescanning every type event for each heterogeneous declaration.
    #[test]
    fn timing_indexes_final_dimensions_in_one_event_pass() {
        use crate::engine::language::model::FINAL_TYPE_EVENT_VISITS;
        let names: Vec<_> = (0..128).map(|i| format!("x{i}")).collect();
        let equations: String = names
            .iter()
            .map(|name| format!("{name}={name}(-1);\n"))
            .collect();
        let model = crate::engine::language::parser::parse(&format!(
            "heterogeneity_dimension h;var(heterogeneity=h) {};model(heterogeneity=h);{equations}end;",
            names.join(" ")
        ));
        FINAL_TYPE_EVENT_VISITS.with(|count| count.set(0));
        let timing = TimingAnalysis::new(&model);
        assert!(timing.aggregate.is_empty());
        for name in &names {
            assert_eq!(timing.all[name].offsets, [-1, 0]);
        }
        assert_eq!(
            FINAL_TYPE_EVENT_VISITS.with(std::cell::Cell::get),
            model.symbol_type_events.len()
        );
    }
}
