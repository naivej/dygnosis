//! Shared timing facts after the predetermined-variable convention conversion.
//! The expression tree and diagnostic readers retain their written offsets.

use std::collections::{HashMap, HashSet};

use crate::intern::Name;
use crate::model::{Equation, Model};

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
    /// The displayed uses received the predetermined-variable conversion.
    /// A no-use fallback has no converted occurrence.
    pub predetermined_conversion: bool,
}

pub(crate) struct TimingAnalysis {
    marked: HashSet<Name>,
    pub aggregate: HashMap<String, TimingInfo>,
    pub all: HashMap<String, TimingInfo>,
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
        };
        let mut aggregate_offsets = HashMap::new();
        for equation in &model.equations {
            analysis.record_aggregate(model, equation, &mut aggregate_offsets);
        }
        let declarations = model.final_decls(&["var"]);
        let dimensions: HashMap<Name, Name> = declarations
            .iter()
            .filter_map(|declaration| {
                model
                    .final_heterogeneity(declaration)
                    .map(|dimension| (declaration.name, dimension))
            })
            .collect();
        let mut heterogeneous_offsets: HashMap<Name, HashSet<i32>> = HashMap::new();
        for block in &model.heterogeneous_models {
            for equation in &block.equations {
                if !equation.static_tag {
                    for reference in model.ident_refs(equation) {
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
            let name = model.name(declaration.name).to_string();
            if model.final_heterogeneity(declaration).is_none() {
                let mut timing = classify_offsets(aggregate_offsets.get(&declaration.name));
                timing.predetermined_conversion = analysis.marked.contains(&declaration.name)
                    && aggregate_offsets.contains_key(&declaration.name);
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
        predetermined_conversion: false,
    }
}

/// Shared display classes: ordinary names use their aggregate model; dimension
/// names use only blocks of their own dimension.
pub fn classify_variable_timing(model: &Model) -> HashMap<String, TimingInfo> {
    TimingAnalysis::new(model).all
}

pub(crate) fn classify_aggregate_variable_timing(model: &Model) -> HashMap<String, TimingInfo> {
    TimingAnalysis::new(model).aggregate
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
    let timing = classify_aggregate_variable_timing(model);
    let mut predetermined = 0;
    let mut forward_looking = 0;
    let mut static_vars = 0;
    let mut max_lead = 0;
    let mut max_lag = 0;
    for info in timing.values() {
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
            if o > max_lead {
                max_lead = o;
            }
            if o < max_lag {
                max_lag = o;
            }
        }
    }
    StructureSummary {
        endogenous: model.final_endogenous().len(),
        predetermined,
        forward_looking,
        static_vars,
        varexo: model
            .final_decls(&["varexo", "varexo_det"])
            .into_iter()
            .filter(|decl| model.final_heterogeneity(decl).is_none())
            .count(),
        max_lead,
        max_lag,
    }
}
