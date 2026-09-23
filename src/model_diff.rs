//! Structural compare of two parsed models. No numeric steady state, no solver.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use serde::Serialize;
use serde_json::Value;

use crate::equations::{equations, EquationRow};
use crate::lexer::{tokenize, TokenKind};
use crate::model::{
    Assignment, Decl, EndvalInstruction, Model, PathBlock, PathTarget, PeriodPoint, PeriodRange,
    ShockBlock, ShockBlockKind, ShockKind, ShockOperation, ShockOptions, ShockStmt,
};
use crate::model_info::assigned_number;
use crate::parser::normalize_newlines;
use crate::span::{LineIndex, Span};

const VALUE_TOL: f64 = 1e-12;
const EQ_CHANGE_RATIO: f64 = 0.4;

/// A parameter present in both models whose calibration differs.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ParameterChange {
    pub name: String,
    pub old_value: Option<f64>,
    pub new_value: Option<f64>,
    pub old_raw: String,
    pub new_raw: String,
}

/// Near-match pairing of one removed and one added equation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct EquationChange {
    pub index_old: usize,
    pub index_new: usize,
    pub text_old: String,
    pub text_new: String,
}

/// One counted equation in an add/remove list. `index` is the equation-object identity.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct IndexedEquation {
    pub index: usize,
    pub text: String,
}

/// A verified location in the source file the caller supplied.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SourceLocation {
    pub line: u32,
    pub column: u32,
    pub end_line: u32,
    pub end_column: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct WrittenPeriod {
    pub kind: String,
    pub text: String,
}

/// One side of a written shock instruction. Absent fields do not apply to that form.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ShockSetting {
    pub block: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub written_target: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group_explicit: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group_location: Option<SourceLocation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub related_setup: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub related_target: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub references: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub measure: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub periods: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub values: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub learnt_in: Option<WrittenPeriod>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub relative_to_initval: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub released_exogenous: Option<String>,
    pub overwrite: bool,
    #[serde(skip_serializing_if = "is_written_context_status")]
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<SourceLocation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub origin_uri: Option<String>,
}

/// An added, removed, or clearly paired change to one written shock instruction.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ShockSetupChange {
    pub form: String,
    pub role: String,
    pub target: String,
    pub change: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before: Option<ShockSetting>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after: Option<ShockSetting>,
}

/// The original text and optional file identity used to verify compare locations.
#[derive(Clone, Copy, Debug)]
pub struct CompareSource<'a> {
    pub text: &'a str,
    pub origin_uri: Option<&'a str>,
}

/// Structural diff. JSON has no computed steady-state keys.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ModelDiff {
    pub added_endogenous: Vec<String>,
    pub removed_endogenous: Vec<String>,
    pub common_endogenous: Vec<String>,
    pub added_exogenous: Vec<String>,
    pub removed_exogenous: Vec<String>,
    pub common_exogenous: Vec<String>,
    pub added_parameters: Vec<String>,
    pub removed_parameters: Vec<String>,
    pub common_parameters: Vec<String>,
    pub changed_parameter_values: Vec<ParameterChange>,
    pub added_equations: Vec<IndexedEquation>,
    pub removed_equations: Vec<IndexedEquation>,
    pub changed_equations: Vec<EquationChange>,
    pub shock_setup_changes: Vec<ShockSetupChange>,
}

impl ModelDiff {
    pub fn to_json(&self) -> Value {
        let mut v = serde_json::to_value(self).unwrap_or(Value::Null);
        if let Some(obj) = v.as_object_mut() {
            obj.insert("markdown".to_string(), Value::String(self.to_markdown()));
        }
        v
    }

    pub fn to_markdown(&self) -> String {
        let mut lines = vec!["# Model diff".to_string()];

        fn section(lines: &mut Vec<String>, title: &str, items: &[String]) {
            if items.is_empty() {
                return;
            }
            lines.push(String::new());
            lines.push(format!("## {title}"));
            for item in items {
                lines.push(format!("- {item}"));
            }
        }

        section(&mut lines, "Added endogenous", &self.added_endogenous);
        section(&mut lines, "Removed endogenous", &self.removed_endogenous);
        section(&mut lines, "Added exogenous", &self.added_exogenous);
        section(&mut lines, "Removed exogenous", &self.removed_exogenous);
        section(&mut lines, "Added parameters", &self.added_parameters);
        section(&mut lines, "Removed parameters", &self.removed_parameters);

        if !self.changed_parameter_values.is_empty() {
            lines.push(String::new());
            lines.push("## Changed parameter values".into());
            for p in &self.changed_parameter_values {
                if let (Some(old), Some(new)) = (p.old_value, p.new_value) {
                    lines.push(format!("- `{}`: {} -> {}", p.name, old, new));
                } else {
                    lines.push(format!(
                        "- `{}`: `{}` -> `{}`",
                        p.name, p.old_raw, p.new_raw
                    ));
                }
            }
        }

        let setup_changes: Vec<&ShockSetupChange> = self
            .shock_setup_changes
            .iter()
            .filter(|change| !is_shock_analysis_change(change))
            .collect();
        if !setup_changes.is_empty() {
            lines.push(String::new());
            lines.push("## Shock setup changes".into());
            for change in setup_changes {
                lines.push(format!("- {}", format_shock_change(change)));
            }
        }
        let analysis_changes: Vec<&ShockSetupChange> = self
            .shock_setup_changes
            .iter()
            .filter(|change| is_shock_analysis_change(change))
            .collect();
        if !analysis_changes.is_empty() {
            lines.push(String::new());
            lines.push("## Shock analysis setup".into());
            for change in analysis_changes {
                lines.push(format!("- {}", format_shock_change(change)));
            }
        }

        if !self.changed_equations.is_empty() {
            lines.push(String::new());
            lines.push("## Changed equations".into());
            for e in &self.changed_equations {
                lines.push(format!(
                    "- [{} -> {}]: `{}` -> `{}`",
                    e.index_old, e.index_new, e.text_old, e.text_new
                ));
            }
        }

        if !self.added_equations.is_empty() {
            lines.push(String::new());
            lines.push("## Added equations".into());
            for eq in &self.added_equations {
                lines.push(format!("- [{}] `{}`", eq.index, eq.text));
            }
        }

        if !self.removed_equations.is_empty() {
            lines.push(String::new());
            lines.push("## Removed equations".into());
            for eq in &self.removed_equations {
                lines.push(format!("- [{}] `{}`", eq.index, eq.text));
            }
        }

        if lines.len() == 1 {
            lines.push(String::new());
            lines.push("_No structural or calibration changes detected._".into());
        }
        lines.push(String::new());
        lines.join("\n")
    }
}

/// Compare `model_a` (before) with `model_b` (after).
pub fn compare_models(model_a: &Model, model_b: &Model) -> ModelDiff {
    compare_models_with_sources(model_a, model_b, None, None)
}

/// Compare two parsed models, attaching row locations only when each source text
/// is exactly the text parsed for that model. The original compare entry point
/// remains safe for models built from spliced includes.
pub fn compare_models_with_sources(
    model_a: &Model,
    model_b: &Model,
    source_a: Option<CompareSource<'_>>,
    source_b: Option<CompareSource<'_>>,
) -> ModelDiff {
    let end_a = names(model_a, &model_a.endogenous);
    let end_b = names(model_b, &model_b.endogenous);
    let exo_a = names(model_a, &model_a.exogenous);
    let exo_b = names(model_b, &model_b.exogenous);
    let par_a = names(model_a, &model_a.parameters);
    let par_b = names(model_b, &model_b.parameters);

    let common_params: HashSet<String> = par_a.intersection(&par_b).cloned().collect();
    let changed_parameter_values = changed_params(model_a, model_b, &common_params);

    let (added_eq, removed_eq, changed_eq) = diff_equations(model_a, model_b);

    ModelDiff {
        added_endogenous: sorted_diff(&end_b, &end_a),
        removed_endogenous: sorted_diff(&end_a, &end_b),
        common_endogenous: sorted_intersect(&end_a, &end_b),
        added_exogenous: sorted_diff(&exo_b, &exo_a),
        removed_exogenous: sorted_diff(&exo_a, &exo_b),
        common_exogenous: sorted_intersect(&exo_a, &exo_b),
        added_parameters: sorted_diff(&par_b, &par_a),
        removed_parameters: sorted_diff(&par_a, &par_b),
        common_parameters: sorted_intersect(&par_a, &par_b),
        changed_parameter_values,
        added_equations: added_eq,
        removed_equations: removed_eq,
        changed_equations: changed_eq,
        shock_setup_changes: diff_shock_setup(model_a, model_b, source_a, source_b),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum LearningKey {
    Integer(i32),
    Date(DateKey),
}

/// Dynare's `dates` class compares frequency and period count. Keep an opaque
/// fallback for any spelling we cannot evaluate safely.
#[derive(Clone, Debug, PartialEq, Eq)]
enum DateKey {
    Period { frequency: u8, index: i64 },
    Written(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum ShockBucket {
    Stochastic,
    Skew(String),
    Deterministic,
    Surprise,
    Heteroskedastic,
    Learnt(LearningKey),
    Path(LearningKey),
    Other,
}

struct ShockInstruction {
    form: &'static str,
    role: &'static str,
    target: String,
    setting: ShockSetting,
    bucket: ShockBucket,
    span: Span,
}

impl ShockInstruction {
    fn same_written_setting(&self, other: &Self) -> bool {
        self.form == other.form
            && self.role == other.role
            && self.target == other.target
            && self.setting.block == other.setting.block
            && self.setting.group == other.setting.group
            && self.setting.group_explicit == other.setting.group_explicit
            && self.setting.measure == other.setting.measure
            && self.setting.operation == other.setting.operation
            && self.setting.periods == other.setting.periods
            && self.setting.values == other.setting.values
            && self.setting.learnt_in == other.setting.learnt_in
            && self.setting.relative_to_initval == other.setting.relative_to_initval
            && self.setting.released_exogenous == other.setting.released_exogenous
            && self.setting.overwrite == other.setting.overwrite
            && self.setting.status == other.setting.status
    }

    fn pairing_key(&self) -> (&str, &str, &str, Option<&str>) {
        (
            self.form,
            self.role,
            &self.target,
            self.setting.group.as_deref(),
        )
    }
}

struct VerifiedSource<'a> {
    text: &'a str,
    index: LineIndex,
    origin_uri: Option<String>,
}

impl<'a> VerifiedSource<'a> {
    fn new(model: &'a Model, supplied: Option<CompareSource<'_>>) -> Option<Self> {
        let supplied = supplied?;
        if normalize_newlines(supplied.text) != model.source {
            return None;
        }
        Some(Self {
            text: &model.source,
            index: LineIndex::new(&model.source),
            origin_uri: supplied.origin_uri.map(str::to_string),
        })
    }

    fn location(&self, span: Span) -> Option<SourceLocation> {
        self.text.get(span.start as usize..span.end as usize)?;
        let start = self.index.position(self.text, span.start);
        let end = self.index.position(self.text, span.end);
        Some(SourceLocation {
            line: start.line + 1,
            column: start.character + 1,
            end_line: end.line + 1,
            end_column: end.character + 1,
        })
    }
}

#[derive(Default)]
struct BaselineUse {
    setups: BTreeSet<&'static str>,
    targets: BTreeSet<String>,
}

#[derive(Default)]
struct ContextRelevance {
    baselines: BTreeMap<(String, &'static str), BaselineUse>,
    timed: bool,
    timing_targets: BTreeSet<String>,
    paths: bool,
    heteroskedastic: bool,
    heteroskedastic_targets: BTreeSet<String>,
}

impl ContextRelevance {
    fn for_pair(before: &Model, after: &Model) -> Self {
        let mut relevance = Self::default();
        relevance.add_model(before);
        relevance.add_model(after);
        relevance
    }

    fn add_model(&mut self, model: &Model) {
        let regular_endval_names: HashSet<_> = model
            .endval_instructions
            .iter()
            .filter(|block| is_default_learning(block.learnt_in.as_ref()))
            .flat_map(|block| {
                block
                    .entries
                    .iter()
                    .map(|row| model.name(row.name).to_string())
            })
            .collect();
        for block in &model.shock_blocks {
            if !block.scheduled.is_empty() {
                self.timed = true;
            }
            for row in &block.scheduled {
                let target = model.name(row.name).to_string();
                self.timing_targets.insert(target.clone());
                if block.kind == ShockBlockKind::Multiplicative {
                    let baseline = if block.options.relative_to_initval
                        || !regular_endval_names.contains(&target)
                    {
                        "initval"
                    } else {
                        "endval"
                    };
                    let use_record = self
                        .baselines
                        .entry((target.clone(), baseline))
                        .or_default();
                    use_record.setups.insert("mshocks");
                    use_record.targets.insert(target);
                } else if block.kind == ShockBlockKind::Heteroskedastic {
                    self.heteroskedastic = true;
                    self.heteroskedastic_targets.insert(target);
                }
            }
            if block.kind == ShockBlockKind::Heteroskedastic {
                self.heteroskedastic = true;
            }
        }
        self.paths |= !model.shock_paths.is_empty();
        for block in &model.shock_paths {
            for stanza in &block.stanzas {
                self.timed = true;
                let path_name = path_target(model, &stanza.target).1;
                self.timing_targets.insert(path_name.clone());
                for value in &stanza.values {
                    for reference in &value.path_refs {
                        if matches!(reference.namespace.as_deref(), Some("initval" | "init")) {
                            let name = model.name(reference.name).to_string();
                            let use_record = self.baselines.entry((name, "initval")).or_default();
                            use_record.setups.insert("shock_paths");
                            use_record.targets.insert(path_name.clone());
                        }
                    }
                }
            }
        }
        for block in &model.controlled_paths {
            for stanza in &block.stanzas {
                self.timed = true;
                self.timing_targets
                    .insert(path_target(model, &stanza.target).1);
            }
        }
        for block in &model.endval_instructions {
            for row in &block.entries {
                self.timed = true;
                self.timing_targets.insert(model.name(row.name).to_string());
            }
        }
    }
}

fn diff_shock_setup(
    before: &Model,
    after: &Model,
    source_before: Option<CompareSource<'_>>,
    source_after: Option<CompareSource<'_>>,
) -> Vec<ShockSetupChange> {
    let before_source = VerifiedSource::new(before, source_before);
    let after_source = VerifiedSource::new(after, source_after);
    let relevance = ContextRelevance::for_pair(before, after);
    let old = shock_instructions(before, before_source.as_ref(), &relevance);
    let new = shock_instructions(after, after_source.as_ref(), &relevance);
    let mut used_old = vec![false; old.len()];
    let mut used_new = vec![false; new.len()];

    // Cancel unchanged duplicates one at a time. Source positions are never
    // part of identity; moving a block without changing it is not a shock diff.
    for (i, item) in old.iter().enumerate() {
        if let Some(j) = new
            .iter()
            .enumerate()
            .find(|(j, candidate)| !used_new[*j] && item.same_written_setting(candidate))
            .map(|(j, _)| j)
        {
            used_old[i] = true;
            used_new[j] = true;
        }
    }

    let mut pending: Vec<(u8, u32, usize, ShockSetupChange)> = Vec::new();
    for (i, item) in old.iter().enumerate() {
        if used_old[i] {
            continue;
        }
        let matches_old = old
            .iter()
            .enumerate()
            .filter(|(k, candidate)| !used_old[*k] && candidate.pairing_key() == item.pairing_key())
            .count();
        let matches_new: Vec<usize> = new
            .iter()
            .enumerate()
            .filter(|(k, candidate)| !used_new[*k] && candidate.pairing_key() == item.pairing_key())
            .map(|(k, _)| k)
            .collect();
        if matches_old == 1 && matches_new.len() == 1 {
            let j = matches_new[0];
            used_old[i] = true;
            used_new[j] = true;
            pending.push((
                0,
                new[j].span.start,
                j,
                ShockSetupChange {
                    form: item.form.to_string(),
                    role: item.role.to_string(),
                    target: item.target.clone(),
                    change: "changed".into(),
                    before: Some(item.setting.clone()),
                    after: Some(new[j].setting.clone()),
                },
            ));
        }
    }

    for (i, item) in old.iter().enumerate() {
        if !used_old[i] {
            pending.push((
                1,
                item.span.start,
                i,
                ShockSetupChange {
                    form: item.form.to_string(),
                    role: item.role.to_string(),
                    target: item.target.clone(),
                    change: "removed".into(),
                    before: Some(item.setting.clone()),
                    after: None,
                },
            ));
        }
    }
    for (j, item) in new.iter().enumerate() {
        if !used_new[j] {
            pending.push((
                0,
                item.span.start,
                j,
                ShockSetupChange {
                    form: item.form.to_string(),
                    role: item.role.to_string(),
                    target: item.target.clone(),
                    change: "added".into(),
                    before: None,
                    after: Some(item.setting.clone()),
                },
            ));
        }
    }
    pending.sort_by_key(|(side, offset, index, _)| (*side, *offset, *index));
    pending
        .into_iter()
        .map(|(_, _, _, change)| change)
        .collect()
}

fn source_text(model: &Model, span: Span) -> Option<&str> {
    model.source.get(span.start as usize..span.end as usize)
}

fn written_period(model: &Model, point: &PeriodPoint, span: Option<Span>) -> WrittenPeriod {
    let (kind, fallback) = match point {
        PeriodPoint::Integer(value) => ("integer", value.to_string()),
        PeriodPoint::Date(date) => ("date", date.text.clone()),
        PeriodPoint::End => ("end", "end".into()),
    };
    let text = span
        .and_then(|span| source_text(model, span))
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
        .unwrap_or(fallback);
    WrittenPeriod {
        kind: kind.into(),
        text,
    }
}

fn learning_key(point: Option<&PeriodPoint>) -> LearningKey {
    match point {
        Some(PeriodPoint::Integer(value)) => LearningKey::Integer(*value),
        Some(PeriodPoint::Date(date)) => LearningKey::Date(date_replacement_key(&date.text)),
        _ => LearningKey::Integer(1),
    }
}

/// The 7.2 grammar admits DATE followed by zero or more `+ INT_NUMBER`s.
/// Match the installed `dates` class's frequency and period count without
/// changing the written value shown in compare output.
fn date_replacement_key(text: &str) -> DateKey {
    let compact: String = text.chars().filter(|ch| !ch.is_whitespace()).collect();
    let fallback = DateKey::Written(compact.to_ascii_lowercase());
    (|| {
        let mut terms = compact.split('+');
        let base = terms.next()?;
        let unit_at = base.bytes().position(|byte| byte.is_ascii_alphabetic())?;
        let year = base[..unit_at].parse::<i64>().ok()?;
        let suffix = base[unit_at..].to_ascii_lowercase();
        let (frequency, subperiod): (u8, i64) = match suffix.as_str() {
            "y" | "a" => (1, 0),
            "s1" | "h1" => (2, 1),
            "s2" | "h2" => (2, 2),
            "q1" => (4, 1),
            "q2" => (4, 2),
            "q3" => (4, 3),
            "q4" => (4, 4),
            _ => {
                let month = suffix.strip_prefix('m')?.parse::<u8>().ok()?;
                if !(1..=12).contains(&month) || suffix != format!("m{month}") {
                    return None;
                }
                (12, i64::from(month))
            }
        };
        let mut index = year
            .checked_mul(i64::from(frequency))?
            .checked_add(subperiod)?;
        for term in terms {
            if term.is_empty() || !term.bytes().all(|byte| byte.is_ascii_digit()) {
                return None;
            }
            index = index.checked_add(term.parse::<i64>().ok()?)?;
        }
        Some(DateKey::Period { frequency, index })
    })()
    .unwrap_or(fallback)
}

fn is_default_learning(point: Option<&PeriodPoint>) -> bool {
    matches!(learning_key(point), LearningKey::Integer(1))
}

fn period_ranges(model: &Model, ranges: &[PeriodRange]) -> Vec<String> {
    ranges
        .iter()
        .map(|range| {
            source_text(model, range.span)
                .map(str::trim)
                .map(str::to_string)
                .unwrap_or_else(|| {
                    let first = written_period(model, &range.first, None).text;
                    match &range.last {
                        Some(last) => format!("{first}:{}", written_period(model, last, None).text),
                        None => first,
                    }
                })
        })
        .collect()
}

fn base_shock_setting(
    block: &str,
    options: Option<&ShockOptions>,
    learnt_in: Option<(&PeriodPoint, Option<Span>)>,
    model: &Model,
    span: Span,
    source: Option<&VerifiedSource<'_>>,
) -> ShockSetting {
    let learnt_in =
        learnt_in.map(|(point, written_span)| written_period(model, point, written_span));
    ShockSetting {
        block: block.into(),
        domain: None,
        written_target: None,
        group: None,
        group_explicit: None,
        group_location: None,
        related_setup: None,
        related_target: None,
        references: None,
        measure: None,
        operation: None,
        periods: None,
        values: None,
        learnt_in,
        relative_to_initval: None,
        released_exogenous: None,
        overwrite: options.is_some_and(|options| options.overwrite),
        status: "active".into(),
        location: source.and_then(|source| source.location(span)),
        origin_uri: source.and_then(|source| source.origin_uri.clone()),
    }
}

fn operation_name(operation: ShockOperation) -> &'static str {
    match operation {
        ShockOperation::Values => "values",
        ShockOperation::Add => "add",
        ShockOperation::Multiply => "multiply",
        ShockOperation::Scales => "scales",
    }
}

enum ShockBlockRef<'a> {
    Shocks(&'a ShockBlock),
    Path(&'a PathBlock),
    Controlled(&'a PathBlock),
    Endval(&'a EndvalInstruction),
}

fn shock_instructions(
    model: &Model,
    source: Option<&VerifiedSource<'_>>,
    relevance: &ContextRelevance,
) -> Vec<ShockInstruction> {
    let mut blocks = Vec::new();
    for block in &model.shock_blocks {
        if block.kind != ShockBlockKind::Heterogeneous {
            blocks.push((block.span.start, ShockBlockRef::Shocks(block)));
        }
    }
    for block in &model.shock_paths {
        blocks.push((block.span.start, ShockBlockRef::Path(block)));
    }
    for block in &model.controlled_paths {
        blocks.push((block.span.start, ShockBlockRef::Controlled(block)));
    }
    for block in &model.endval_instructions {
        blocks.push((block.span.start, ShockBlockRef::Endval(block)));
    }
    blocks.sort_by_key(|(start, _)| *start);

    let mut out = Vec::new();
    for (_, block) in blocks {
        match block {
            ShockBlockRef::Shocks(block) => append_shock_block(&mut out, model, block, source),
            ShockBlockRef::Path(block) => append_path_block(&mut out, model, block, source),
            ShockBlockRef::Controlled(block) => {
                append_controlled_block(&mut out, model, block, source)
            }
            ShockBlockRef::Endval(block) => append_endval_block(&mut out, model, block, source),
        }
    }
    append_context_instructions(&mut out, model, source, relevance);
    append_analysis_instructions(&mut out, model, source);
    out.sort_by_key(|item| item.span.start);
    out
}

fn mark_superseded(out: &mut [ShockInstruction], mut applies: impl FnMut(&ShockBucket) -> bool) {
    for item in out {
        if applies(&item.bucket) {
            item.setting.status = "superseded".into();
        }
    }
}

fn block_learning(options: &ShockOptions) -> Option<(&PeriodPoint, Option<Span>)> {
    options
        .learnt_in
        .as_ref()
        .map(|point| (point, options.learnt_in_span))
}

fn append_shock_block(
    out: &mut Vec<ShockInstruction>,
    model: &Model,
    block: &ShockBlock,
    source: Option<&VerifiedSource<'_>>,
) {
    let opts = &block.options;
    let block_name = match block.kind {
        ShockBlockKind::Multiplicative => "mshocks",
        ShockBlockKind::Surprise => "shocks(surprise)",
        ShockBlockKind::Heteroskedastic => "heteroskedastic_shocks",
        _ => "shocks",
    };
    let learnt = opts.learnt_in.as_ref();
    let learnt_key = learning_key(learnt);
    let default_learning = is_default_learning(learnt);
    let bucket = match block.kind {
        ShockBlockKind::Surprise => ShockBucket::Surprise,
        ShockBlockKind::Heteroskedastic => ShockBucket::Heteroskedastic,
        ShockBlockKind::Multiplicative | ShockBlockKind::LearntIn if !default_learning => {
            ShockBucket::Learnt(learnt_key.clone())
        }
        _ => ShockBucket::Deterministic,
    };

    if opts.overwrite {
        match &bucket {
            ShockBucket::Surprise => {
                mark_superseded(out, |existing| *existing == ShockBucket::Surprise)
            }
            ShockBucket::Heteroskedastic => {
                mark_superseded(out, |existing| *existing == ShockBucket::Heteroskedastic)
            }
            ShockBucket::Learnt(key) => mark_superseded(out, |existing| {
                existing == &ShockBucket::Learnt(key.clone())
            }),
            _ if block.kind == ShockBlockKind::Multiplicative => {
                mark_superseded(out, |existing| *existing == ShockBucket::Deterministic)
            }
            _ => mark_superseded(out, |existing| {
                matches!(
                    existing,
                    ShockBucket::Deterministic | ShockBucket::Stochastic
                )
            }),
        }
    }

    let mut written = Vec::new();
    if block.kind == ShockBlockKind::Regular {
        for stmt in &block.stochastic {
            written.push(stochastic_instruction(model, stmt, opts, source));
        }
    }
    for row in &block.scheduled {
        let form = match block.kind {
            ShockBlockKind::Surprise => "surprise_shock",
            ShockBlockKind::Heteroskedastic => "heteroskedastic_shock",
            _ => "scheduled_shock",
        };
        let mut setting = base_shock_setting(
            block_name,
            Some(opts),
            block_learning(opts),
            model,
            row.span,
            source,
        );
        setting.operation = Some(operation_name(row.operation).into());
        setting.periods = Some(period_ranges(model, &row.periods));
        setting.values = Some(row.values.iter().map(|value| value.text.clone()).collect());
        if block.kind == ShockBlockKind::Multiplicative {
            setting.relative_to_initval = Some(opts.relative_to_initval);
        }
        written.push(ShockInstruction {
            form,
            role: "scheduled",
            target: model.name(row.name).into(),
            setting,
            bucket: bucket.clone(),
            span: row.span,
        });
    }
    written.sort_by_key(|item| item.span.start);
    if written.is_empty() && opts.overwrite {
        let target = match &bucket {
            ShockBucket::Surprise => "surprise shocks",
            ShockBucket::Heteroskedastic => "heteroskedastic shocks",
            ShockBucket::Learnt(_) => "learnt shocks",
            _ if block.kind == ShockBlockKind::Multiplicative => "deterministic shocks",
            _ => "deterministic schedules and variance/covariance settings",
        };
        out.push(ShockInstruction {
            form: "shock_reset",
            role: "reset",
            target: target.into(),
            setting: base_shock_setting(
                block_name,
                Some(opts),
                block_learning(opts),
                model,
                block.span,
                source,
            ),
            bucket: ShockBucket::Other,
            span: block.span,
        });
    }
    for item in written {
        if let ShockBucket::Skew(triple) = &item.bucket {
            mark_superseded(out, |existing| {
                existing == &ShockBucket::Skew(triple.clone())
            });
        }
        out.push(item);
    }
}

fn stochastic_instruction(
    model: &Model,
    stmt: &ShockStmt,
    opts: &ShockOptions,
    source: Option<&VerifiedSource<'_>>,
) -> ShockInstruction {
    let (role, measure, names): (&str, &str, Vec<_>) = match &stmt.kind {
        ShockKind::Var(name) => ("size", "variance", vec![*name]),
        ShockKind::Stderr(name) => ("size", "stderr", vec![*name]),
        ShockKind::Cov(names) => ("pair", "covariance", names.clone()),
        ShockKind::Corr { a, b } => ("pair", "correlation", vec![*a, *b]),
        ShockKind::Skew(names) if names.len() == 1 => ("skew", "skewness", names.clone()),
        ShockKind::Skew(names) => ("skew", "co_skewness", names.clone()),
    };
    let mut source_names: Vec<String> = names.iter().map(|name| model.name(*name).into()).collect();
    let written_target = source_names.join(",");
    source_names.sort();
    let mut setting = base_shock_setting("shocks", Some(opts), None, model, stmt.span, source);
    setting.measure = Some(measure.into());
    setting.values = stochastic_rhs_text(model, stmt).map(|value| vec![value]);
    setting.domain = Some(
        if names
            .iter()
            .all(|name| model.varobs.iter().any(|observed| observed.name == *name))
        {
            "measurement_error"
        } else {
            "exogenous"
        }
        .into(),
    );
    if names.len() > 1 || role == "skew" {
        setting.written_target = Some(written_target);
    }
    let target = source_names.join(",");
    // The 7.2 driver stores `skew e` as the tensor triple (e,e,e).
    let canonical_target = if role == "skew" && names.len() == 1 {
        format!("{target},{target},{target}")
    } else {
        target.clone()
    };
    ShockInstruction {
        form: "stochastic_shock",
        role,
        target: canonical_target.clone(),
        setting,
        bucket: if role == "skew" {
            ShockBucket::Skew(canonical_target)
        } else {
            ShockBucket::Stochastic
        },
        span: stmt.span,
    }
}

fn stochastic_rhs_text(model: &Model, stmt: &ShockStmt) -> Option<String> {
    let raw = source_text(model, stmt.span)?;
    let expr_start = stmt
        .rhs_expr
        .map(|expr| {
            model
                .exprs
                .get(expr)
                .span
                .start
                .saturating_sub(stmt.span.start) as usize
        })
        .unwrap_or(raw.len());
    let tokens = tokenize(raw);
    let delimiter = tokens.iter().rev().find(|token| {
        token.span.end as usize <= expr_start
            && match stmt.kind {
                ShockKind::Stderr(_) => {
                    token.kind == TokenKind::Ident && token.text(raw).eq_ignore_ascii_case("stderr")
                }
                _ => token.kind == TokenKind::Eq,
            }
    })?;
    let value_start = delimiter.span.end as usize;
    let value_end = tokens
        .iter()
        .rev()
        .find(|token| token.kind == TokenKind::Semi)
        .map(|token| token.span.start as usize)
        .unwrap_or(raw.len());
    let value = raw.get(value_start..value_end)?.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

fn append_path_block(
    out: &mut Vec<ShockInstruction>,
    model: &Model,
    block: &PathBlock,
    source: Option<&VerifiedSource<'_>>,
) {
    let key = learning_key(block.options.learnt_in.as_ref());
    let bucket = ShockBucket::Path(key.clone());
    if block.options.overwrite {
        mark_superseded(out, |existing| existing == &ShockBucket::Path(key.clone()));
    }
    if block.stanzas.is_empty() && block.options.overwrite {
        out.push(ShockInstruction {
            form: "shock_reset",
            role: "reset",
            target: "shock paths".into(),
            setting: base_shock_setting(
                "shock_paths",
                Some(&block.options),
                block_learning(&block.options),
                model,
                block.span,
                source,
            ),
            bucket: ShockBucket::Other,
            span: block.span,
        });
    }
    for row in &block.stanzas {
        let (role, target, released) = path_target(model, &row.target);
        let mut setting = base_shock_setting(
            "shock_paths",
            Some(&block.options),
            block_learning(&block.options),
            model,
            row.span,
            source,
        );
        setting.operation = Some("values".into());
        setting.periods = Some(period_ranges(model, &row.periods));
        setting.values = Some(row.values.iter().map(|value| value.text.clone()).collect());
        setting.released_exogenous = released;
        out.push(ShockInstruction {
            form: "shock_path",
            role,
            target,
            setting,
            bucket: bucket.clone(),
            span: row.span,
        });
    }
}

fn path_target(model: &Model, target: &PathTarget) -> (&'static str, String, Option<String>) {
    match target {
        PathTarget::Exogenous { name, .. } => ("exogenous", model.name(*name).into(), None),
        PathTarget::Controlled {
            exogenize,
            endogenize,
            ..
        } => (
            "controlled",
            model.name(*exogenize).into(),
            Some(model.name(*endogenize).into()),
        ),
    }
}

fn append_controlled_block(
    out: &mut Vec<ShockInstruction>,
    model: &Model,
    block: &PathBlock,
    source: Option<&VerifiedSource<'_>>,
) {
    for row in &block.stanzas {
        let (role, target, released) = path_target(model, &row.target);
        let mut setting = base_shock_setting(
            "perfect_foresight_controlled_paths",
            Some(&block.options),
            block_learning(&block.options),
            model,
            row.span,
            source,
        );
        setting.operation = Some("values".into());
        setting.periods = Some(period_ranges(model, &row.periods));
        setting.values = Some(row.values.iter().map(|value| value.text.clone()).collect());
        setting.released_exogenous = released;
        out.push(ShockInstruction {
            form: "controlled_path",
            role,
            target,
            setting,
            bucket: ShockBucket::Other,
            span: row.span,
        });
    }
}

fn append_endval_block(
    out: &mut Vec<ShockInstruction>,
    model: &Model,
    block: &EndvalInstruction,
    source: Option<&VerifiedSource<'_>>,
) {
    for row in &block.entries {
        let mut setting = base_shock_setting(
            "endval",
            None,
            block
                .learnt_in
                .as_ref()
                .map(|point| (point, block.learnt_in_span)),
            model,
            row.span,
            source,
        );
        setting.operation = Some(operation_name(row.operation).into());
        setting.values = Some(vec![row.value.text.clone()]);
        out.push(ShockInstruction {
            form: "endval",
            role: "terminal",
            target: model.name(row.name).into(),
            setting,
            bucket: ShockBucket::Other,
            span: row.span,
        });
    }
}

struct ContextSpec<'a> {
    role: &'static str,
    target: String,
    block: &'a str,
    related_setup: &'a str,
    related_target: Option<String>,
    measure: Option<&'a str>,
    values: Option<Vec<String>>,
    span: Span,
}

fn context_instruction(
    model: &Model,
    source: Option<&VerifiedSource<'_>>,
    spec: ContextSpec<'_>,
) -> ShockInstruction {
    let mut setting = base_shock_setting(spec.block, None, None, model, spec.span, source);
    setting.related_setup = Some(spec.related_setup.into());
    setting.related_target = spec.related_target;
    setting.measure = spec.measure.map(str::to_string);
    setting.values = spec.values;
    setting.status = "written".into();
    ShockInstruction {
        form: "shock_context",
        role: spec.role,
        target: spec.target,
        setting,
        bucket: ShockBucket::Other,
        span: spec.span,
    }
}

fn append_context_instructions(
    out: &mut Vec<ShockInstruction>,
    model: &Model,
    source: Option<&VerifiedSource<'_>>,
    relevance: &ContextRelevance,
) {
    append_baseline_context(out, model, source, relevance);
    if relevance.timed {
        let related_target = if relevance.timing_targets.len() == 1 {
            relevance.timing_targets.iter().next().cloned()
        } else {
            None
        };
        for statement in &model.set_time {
            out.push(context_instruction(
                model,
                source,
                ContextSpec {
                    role: "time",
                    target: "set_time".into(),
                    block: "set_time",
                    related_setup: "shock timing",
                    related_target: related_target.clone(),
                    measure: None,
                    values: Some(vec![statement.value.text.clone()]),
                    span: statement.value.span,
                },
            ));
        }
        for option in &model.date_options {
            if !matches!(
                option.command.as_str(),
                "perfect_foresight_setup" | "perfect_foresight_with_expectation_errors_setup"
            ) || !matches!(
                option.name.as_str(),
                "first_simulation_period" | "last_simulation_period"
            ) {
                continue;
            }
            out.push(context_instruction(
                model,
                source,
                ContextSpec {
                    role: "time",
                    target: format!("{}.{}", option.command, option.name),
                    block: &option.command,
                    related_setup: "shock timing",
                    related_target: related_target.clone(),
                    measure: Some(&option.name),
                    values: Some(vec![option.value.text.clone()]),
                    span: option.span,
                },
            ));
        }
    }
    if relevance.paths {
        for declaration in &model.databases {
            for (name, span) in &declaration.names {
                let namespace = model.name(*name);
                let mut item = context_instruction(
                    model,
                    source,
                    ContextSpec {
                        role: "database",
                        target: namespace.into(),
                        block: "database",
                        related_setup: "shock_paths",
                        related_target: None,
                        measure: None,
                        values: None,
                        span: *span,
                    },
                );
                let mut references = Vec::new();
                let mut referring_targets = HashSet::new();
                for block in &model.shock_paths {
                    for stanza in &block.stanzas {
                        for value in &stanza.values {
                            if value
                                .path_refs
                                .iter()
                                .any(|reference| reference.namespace.as_deref() == Some(namespace))
                            {
                                referring_targets.insert(path_target(model, &stanza.target).1);
                                if !references.contains(&value.text) {
                                    references.push(value.text.clone());
                                }
                            }
                        }
                    }
                }
                item.setting.related_target = only_target(referring_targets);
                if !references.is_empty() {
                    item.setting.references = Some(references);
                }
                out.push(item);
            }
        }
    }
    if relevance.heteroskedastic {
        let heteroskedastic_target = if relevance.heteroskedastic_targets.len() == 1 {
            relevance.heteroskedastic_targets.iter().next().cloned()
        } else {
            None
        };
        for statement in &model.estimation_statements {
            for option in &statement.data_options {
                let target = if option.name.eq_ignore_ascii_case("first_obs") {
                    "estimation.first_obs"
                } else {
                    "estimation.source"
                };
                out.push(context_instruction(
                    model,
                    source,
                    ContextSpec {
                        role: "data_source",
                        target: target.into(),
                        block: "estimation",
                        related_setup: "heteroskedastic_shocks",
                        related_target: heteroskedastic_target.clone(),
                        measure: Some(&option.name),
                        values: Some(vec![written_option_value(model, option)]),
                        span: option_value_span(option),
                    },
                ));
            }
        }
        for statement in &model.data_statements {
            for option in &statement.options {
                if !matches!(
                    option.name.to_ascii_lowercase().as_str(),
                    "file" | "series" | "first_obs"
                ) {
                    continue;
                }
                let target = if option.name.eq_ignore_ascii_case("first_obs") {
                    "data.first_obs"
                } else {
                    "data.source"
                };
                out.push(context_instruction(
                    model,
                    source,
                    ContextSpec {
                        role: "data_source",
                        target: target.into(),
                        block: "data",
                        related_setup: "heteroskedastic_shocks",
                        related_target: heteroskedastic_target.clone(),
                        measure: Some(&option.name),
                        values: Some(vec![written_option_value(model, option)]),
                        span: option_value_span(option),
                    },
                ));
            }
        }
    }
}

fn only_target(targets: HashSet<String>) -> Option<String> {
    if targets.len() == 1 {
        targets.into_iter().next()
    } else {
        None
    }
}

fn option_value_span(option: &crate::model::FamilyOption) -> Span {
    Span {
        start: option.span.start,
        end: option.value_span.end,
    }
}

fn written_option_value(model: &Model, option: &crate::model::FamilyOption) -> String {
    source_text(model, option.value_span)
        .map(str::trim)
        .map(str::to_string)
        .unwrap_or_else(|| option.value_text.clone())
}

fn written_assignment_value(model: &Model, assignment: &Assignment) -> String {
    source_text(model, assignment.span)
        .and_then(|text| text.split_once('=').map(|(_, value)| value))
        .map(|value| value.trim().trim_end_matches(';').trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| assignment.expression.clone())
}

fn append_baseline_context(
    out: &mut Vec<ShockInstruction>,
    model: &Model,
    source: Option<&VerifiedSource<'_>>,
    relevance: &ContextRelevance,
) {
    for ((target, baseline), usage) in &relevance.baselines {
        let related_setup = usage
            .setups
            .iter()
            .copied()
            .collect::<Vec<_>>()
            .join(" and ");
        let related_target = if usage.targets.len() == 1 {
            usage.targets.iter().next().cloned()
        } else {
            None
        };
        if *baseline == "initval" {
            for assignment in &model.initval {
                if model.name(assignment.name) == target {
                    out.push(context_instruction(
                        model,
                        source,
                        ContextSpec {
                            role: "baseline",
                            target: target.clone(),
                            block: "initval",
                            related_setup: &related_setup,
                            related_target: related_target.clone(),
                            measure: None,
                            values: Some(vec![written_assignment_value(model, assignment)]),
                            span: assignment.span,
                        },
                    ));
                }
            }
        } else {
            for block in &model.endval_instructions {
                if !is_default_learning(block.learnt_in.as_ref()) {
                    continue;
                }
                for row in &block.entries {
                    if model.name(row.name) == target {
                        out.push(context_instruction(
                            model,
                            source,
                            ContextSpec {
                                role: "baseline",
                                target: target.clone(),
                                block: "endval",
                                related_setup: &related_setup,
                                related_target: related_target.clone(),
                                measure: Some(operation_name(row.operation)),
                                values: Some(vec![row.value.text.clone()]),
                                span: row.span,
                            },
                        ));
                    }
                }
            }
        }
    }
}

fn append_analysis_instructions(
    out: &mut Vec<ShockInstruction>,
    model: &Model,
    source: Option<&VerifiedSource<'_>>,
) {
    for block in &model.shock_group_blocks {
        for row in &model.shock_groups[block.row_start..block.row_end] {
            let mut setting =
                base_shock_setting("shock_groups", None, None, model, row.span, source);
            setting.group = Some(block.group.clone());
            setting.group_explicit = Some(block.group_span.is_some());
            setting.status = "written".into();
            setting.group_location = block
                .group_span
                .or(Some(block.span))
                .and_then(|span| source.and_then(|source| source.location(span)));
            setting.values = Some(
                row.members
                    .iter()
                    .map(|(name, _)| model.name(*name).to_string())
                    .collect(),
            );
            out.push(ShockInstruction {
                form: "shock_group",
                role: "membership",
                target: row.label.clone(),
                setting,
                bucket: ShockBucket::Other,
                span: row.span,
            });
        }
    }
    for block in &model.init2shocks_blocks {
        for row in &block.rows {
            let mut setting =
                base_shock_setting("init2shocks", None, None, model, row.span, source);
            setting.group = Some(block.group.clone());
            setting.group_explicit = Some(block.group_span.is_some());
            setting.status = "written".into();
            setting.group_location = block
                .group_span
                .or(Some(block.span))
                .and_then(|span| source.and_then(|source| source.location(span)));
            setting.values = Some(vec![model.name(row.exo).into()]);
            out.push(ShockInstruction {
                form: "init2shocks",
                role: "attribution",
                target: model.name(row.endo).into(),
                setting,
                bucket: ShockBucket::Other,
                span: row.span,
            });
        }
    }
}

fn is_shock_analysis_change(change: &ShockSetupChange) -> bool {
    matches!(change.form.as_str(), "shock_group" | "init2shocks")
}

fn is_written_context_status(status: &String) -> bool {
    status == "written"
}

fn format_shock_change(change: &ShockSetupChange) -> String {
    if change.form == "shock_context" {
        return format_context_change(change);
    }
    if is_shock_analysis_change(change) {
        return format_analysis_change(change);
    }
    let setting = change.after.as_ref().or(change.before.as_ref());
    let measurement_error =
        setting.and_then(|setting| setting.domain.as_deref()) == Some("measurement_error");
    let multiplicative = setting.is_some_and(|setting| setting.block == "mshocks");
    let label = match (change.form.as_str(), change.role.as_str()) {
        ("stochastic_shock", _) if measurement_error => "measurement error",
        ("stochastic_shock", _) => "stochastic shock",
        ("scheduled_shock", _) if multiplicative => "multiplicative shock",
        ("scheduled_shock", _) => "temporary shock",
        ("surprise_shock", _) => "surprise shock",
        ("heteroskedastic_shock", _) => "heteroskedastic shock",
        ("shock_path", "controlled") | ("controlled_path", _) => "controlled path",
        ("shock_path", _) => "exogenous path",
        ("endval", _) => "terminal value",
        ("shock_reset", _) => "overwrite reset",
        _ => "shock setup",
    };
    let shown_target = setting
        .and_then(|setting| setting.written_target.as_deref())
        .unwrap_or(&change.target);
    let prefix = format!("{} — {label}", markdown_escape(shown_target));
    let detail = match (&change.before, &change.after) {
        (None, Some(after)) => format!("added: {}", setting_summary(after)),
        (Some(before), None) => format!("removed: {}", setting_summary(before)),
        (Some(before), Some(after)) => {
            let fields = changed_setting_fields(before, after);
            if fields.is_empty() {
                "changed".into()
            } else {
                fields.join("; ")
            }
        }
        (None, None) => "changed".into(),
    };
    format!("{prefix}: {detail}{}", change_location(change))
}

fn change_location(change: &ShockSetupChange) -> String {
    match (&change.before, &change.after) {
        (Some(before), Some(after)) => match (&before.location, &after.location) {
            (Some(a), Some(b)) => format!(" (before line {}; after line {})", a.line, b.line),
            (Some(a), None) => format!(" (before line {})", a.line),
            (None, Some(b)) => format!(" (after line {})", b.line),
            _ => String::new(),
        },
        (Some(before), None) => before
            .location
            .as_ref()
            .map(|loc| format!(" (before line {})", loc.line))
            .unwrap_or_default(),
        (None, Some(after)) => after
            .location
            .as_ref()
            .map(|loc| format!(" (after line {})", loc.line))
            .unwrap_or_default(),
        _ => String::new(),
    }
}

fn format_context_change(change: &ShockSetupChange) -> String {
    let setting = change.after.as_ref().or(change.before.as_ref());
    let related = setting.and_then(|setting| setting.related_setup.as_deref());
    let affected = setting
        .and_then(|setting| setting.related_target.as_deref())
        .filter(|target| *target != change.target)
        .map(|target| format!(" for {}", markdown_escape(target)))
        .unwrap_or_default();
    let title = match change.role.as_str() {
        "baseline" => format!(
            "{} — written baseline for {}{}",
            markdown_escape(&change.target),
            markdown_escape(related.unwrap_or("shock setup")),
            affected
        ),
        "time" => format!(
            "Shock timing — {}{}",
            markdown_escape(&change.target),
            affected
        ),
        "database" => format!(
            "Shock path database — {}{}",
            markdown_escape(&change.target),
            affected
        ),
        "data_source" => format!(
            "Heteroskedastic data — {}{}",
            markdown_escape(&change.target),
            affected
        ),
        _ => format!("Shock context — {}", markdown_escape(&change.target)),
    };
    let detail = match (&change.before, &change.after) {
        (None, Some(after)) => format!("added: {}", context_setting_summary(after)),
        (Some(before), None) => format!("removed: {}", context_setting_summary(before)),
        (Some(before), Some(after)) => {
            let fields = changed_setting_fields(before, after);
            if fields.is_empty() {
                "changed".into()
            } else {
                fields.join("; ")
            }
        }
        (None, None) => "changed".into(),
    };
    format!("{title}: {detail}{}", change_location(change))
}

fn context_setting_summary(setting: &ShockSetting) -> String {
    let mut parts = vec![markdown_escape(&setting.block)];
    if let Some(measure) = &setting.measure {
        parts.push(markdown_escape(measure));
    }
    if let Some(values) = &setting.values {
        parts.push(markdown_list(values));
    }
    if let Some(references) = &setting.references {
        parts.push(format!("referenced by {}", markdown_list(references)));
    }
    parts.join(" ")
}

fn format_analysis_change(change: &ShockSetupChange) -> String {
    let setting = change.after.as_ref().or(change.before.as_ref());
    let group = setting
        .and_then(|setting| setting.group.as_deref())
        .unwrap_or("default");
    let label = if change.form == "shock_group" {
        "shock group"
    } else {
        "initial-condition attribution"
    };
    let prefix = format!(
        "{} / {} — {label}",
        markdown_escape(group),
        markdown_escape(&change.target)
    );
    let value_label = if change.form == "shock_group" {
        "members"
    } else {
        "attributed to"
    };
    let detail = match (&change.before, &change.after) {
        (None, Some(after)) => format!(
            "added: {value_label} {}",
            markdown_optional_list(after.values.as_deref())
        ),
        (Some(before), None) => format!(
            "removed: {value_label} {}",
            markdown_optional_list(before.values.as_deref())
        ),
        (Some(before), Some(after)) => {
            let mut fields = Vec::new();
            if before.group_explicit != after.group_explicit {
                fields.push(format!(
                    "block name {} → {}",
                    analysis_group_option(before),
                    analysis_group_option(after)
                ));
            }
            if before.values != after.values {
                fields.push(format!(
                    "{value_label} {} → {}",
                    markdown_optional_list(before.values.as_deref()),
                    markdown_optional_list(after.values.as_deref())
                ));
            }
            fields.join("; ")
        }
        (None, None) => "changed".into(),
    };
    format!("{prefix}: {detail}{}", change_location(change))
}

fn analysis_group_option(setting: &ShockSetting) -> String {
    if setting.group_explicit == Some(true) {
        format!(
            "name={}",
            markdown_escape(setting.group.as_deref().unwrap_or("default"))
        )
    } else {
        "bare default".into()
    }
}

fn setting_summary(setting: &ShockSetting) -> String {
    let mut parts = vec![markdown_escape(&setting.block)];
    if let Some(domain) = &setting.domain {
        if domain == "measurement_error" {
            parts.push("measurement error".into());
        }
    }
    if let Some(measure) = &setting.measure {
        parts.push(markdown_escape(measure));
    }
    if let Some(operation) = &setting.operation {
        parts.push(markdown_escape(operation));
    }
    if let Some(periods) = &setting.periods {
        parts.push(format!("periods {}", markdown_list(periods)));
    }
    if let Some(values) = &setting.values {
        parts.push(format!("value {}", markdown_list(values)));
    }
    if let Some(learning) = &setting.learnt_in {
        parts.push(format!("learnt in {}", markdown_escape(&learning.text)));
    }
    if let Some(released) = &setting.released_exogenous {
        parts.push(format!("released exogenous {}", markdown_escape(released)));
    }
    if let Some(baseline) = setting.relative_to_initval {
        parts.push(if baseline {
            "baseline option relative_to_initval".into()
        } else {
            "baseline option default".into()
        });
    }
    if setting.overwrite {
        parts.push("overwrite".into());
    }
    if setting.status == "superseded" {
        parts.push(status_label(setting).into());
    }
    parts.join("; ")
}

fn changed_setting_fields(before: &ShockSetting, after: &ShockSetting) -> Vec<String> {
    let mut fields = Vec::new();
    if before.block != after.block {
        fields.push(format!(
            "form {} → {}",
            markdown_escape(&before.block),
            markdown_escape(&after.block)
        ));
    }
    if before.domain != after.domain {
        fields.push(format!(
            "domain {} → {}",
            markdown_optional(before.domain.as_deref()),
            markdown_optional(after.domain.as_deref())
        ));
    }
    if before.measure != after.measure {
        fields.push(format!(
            "measure {} → {}",
            markdown_optional(before.measure.as_deref()),
            markdown_optional(after.measure.as_deref())
        ));
    }
    if before.operation != after.operation {
        fields.push(format!(
            "operation {} → {}",
            markdown_optional(before.operation.as_deref()),
            markdown_optional(after.operation.as_deref())
        ));
    }
    if before.periods != after.periods {
        fields.push(format!(
            "periods {} → {}",
            markdown_optional_list(before.periods.as_deref()),
            markdown_optional_list(after.periods.as_deref())
        ));
    }
    if before.values != after.values {
        fields.push(format!(
            "value {} → {}",
            markdown_optional_list(before.values.as_deref()),
            markdown_optional_list(after.values.as_deref())
        ));
    }
    if before.learnt_in != after.learnt_in {
        fields.push(format!(
            "learnt in {} → {}",
            markdown_optional(before.learnt_in.as_ref().map(|p| p.text.as_str())),
            markdown_optional(after.learnt_in.as_ref().map(|p| p.text.as_str()))
        ));
    }
    if before.released_exogenous != after.released_exogenous {
        fields.push(format!(
            "released exogenous {} → {}",
            markdown_optional(before.released_exogenous.as_deref()),
            markdown_optional(after.released_exogenous.as_deref())
        ));
    }
    if before.relative_to_initval != after.relative_to_initval {
        fields.push(format!(
            "multiplicative baseline option {} → {}",
            baseline_option(before.relative_to_initval),
            baseline_option(after.relative_to_initval)
        ));
    }
    if before.overwrite != after.overwrite {
        fields.push(format!(
            "overwrite {} → {}",
            before.overwrite, after.overwrite
        ));
    }
    if before.status != after.status {
        fields.push(format!(
            "status {} → {}",
            status_label(before),
            status_label(after)
        ));
    }
    fields
}

fn status_label(setting: &ShockSetting) -> &'static str {
    if setting.status != "superseded" {
        "active"
    } else if matches!(setting.measure.as_deref(), Some("skewness" | "co_skewness")) {
        "superseded by later skew row"
    } else {
        "superseded by overwrite"
    }
}

fn markdown_optional(value: Option<&str>) -> String {
    value.map(markdown_escape).unwrap_or_else(|| "none".into())
}

fn baseline_option(value: Option<bool>) -> &'static str {
    match value {
        Some(true) => "relative_to_initval",
        Some(false) => "default",
        None => "not applicable",
    }
}

fn markdown_optional_list(values: Option<&[String]>) -> String {
    values.map(markdown_list).unwrap_or_else(|| "none".into())
}

fn markdown_list(values: &[String]) -> String {
    values
        .iter()
        .map(|value| markdown_escape(value))
        .collect::<Vec<_>>()
        .join(", ")
}

fn markdown_escape(value: &str) -> String {
    let mut out = String::new();
    for ch in value.chars() {
        match ch {
            '\n' | '\r' => out.push(' '),
            '\\' | '`' | '*' | '_' | '[' | ']' | '<' | '>' | '|' => {
                out.push('\\');
                out.push(ch);
            }
            _ => out.push(ch),
        }
    }
    out
}

fn names(model: &Model, decls: &[Decl]) -> HashSet<String> {
    decls
        .iter()
        .map(|d| model.name(d.name).to_string())
        .collect()
}

fn sorted_diff(have: &HashSet<String>, against: &HashSet<String>) -> Vec<String> {
    let mut v: Vec<String> = have.difference(against).cloned().collect();
    v.sort();
    v
}

fn sorted_intersect(a: &HashSet<String>, b: &HashSet<String>) -> Vec<String> {
    let mut v: Vec<String> = a.intersection(b).cloned().collect();
    v.sort();
    v
}

fn last_assignment<'a>(model: &'a Model, name: &str) -> Option<&'a Assignment> {
    model
        .param_assignments
        .iter()
        .rfind(|a| model.name(a.name) == name)
}

fn normalize_expr(raw: &str) -> String {
    collapse_ws(raw).trim_end_matches(';').trim().to_string()
}

fn collapse_ws(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn changed_params(a: &Model, b: &Model, common: &HashSet<String>) -> Vec<ParameterChange> {
    let mut out = Vec::new();
    let mut names: Vec<&String> = common.iter().collect();
    names.sort();
    for name in names {
        let old_raw = last_assignment(a, name)
            .map(|asg| asg.expression.clone())
            .unwrap_or_default();
        let new_raw = last_assignment(b, name)
            .map(|asg| asg.expression.clone())
            .unwrap_or_default();
        let old_value = assigned_number(a, name);
        let new_value = assigned_number(b, name);
        let num_changed = match (old_value, new_value) {
            (Some(x), Some(y)) => (x - y).abs() > VALUE_TOL,
            (None, None) => false,
            _ => true,
        };
        let expr_changed = normalize_expr(&old_raw) != normalize_expr(&new_raw);
        if num_changed || expr_changed {
            out.push(ParameterChange {
                name: name.clone(),
                old_value,
                new_value,
                old_raw,
                new_raw,
            });
        }
    }
    out
}

fn normalize_equation(text: &str) -> String {
    let mut s = String::new();
    for line in text.lines() {
        let cut = line.find("//").map(|i| &line[..i]).unwrap_or(line);
        if !s.is_empty() {
            s.push(' ');
        }
        s.push_str(cut);
    }
    let s = collapse_ws(&s);
    s.trim_end_matches(';').trim().to_string()
}

fn diff_equations(
    a: &Model,
    b: &Model,
) -> (
    Vec<IndexedEquation>,
    Vec<IndexedEquation>,
    Vec<EquationChange>,
) {
    let mut norm_a: HashMap<String, Vec<EquationRow>> = HashMap::new();
    for row in equations(a) {
        let key = normalize_equation(&row.text);
        if !key.is_empty() {
            norm_a.entry(key).or_default().push(row);
        }
    }
    let mut norm_b: HashMap<String, Vec<EquationRow>> = HashMap::new();
    for row in equations(b) {
        let key = normalize_equation(&row.text);
        if !key.is_empty() {
            norm_b.entry(key).or_default().push(row);
        }
    }

    let keys_a: HashSet<String> = norm_a.keys().cloned().collect();
    let keys_b: HashSet<String> = norm_b.keys().cloned().collect();

    let mut leftover_removed = Vec::new();
    let mut leftover_added = Vec::new();
    for key in keys_a.union(&keys_b) {
        let na = norm_a.get(key).map(|v| v.len()).unwrap_or(0);
        let nb = norm_b.get(key).map(|v| v.len()).unwrap_or(0);
        let shared = na.min(nb);
        if let Some(list) = norm_a.get(key) {
            leftover_removed.extend(list.iter().skip(shared).cloned());
        }
        if let Some(list) = norm_b.get(key) {
            leftover_added.extend(list.iter().skip(shared).cloned());
        }
    }

    let (changed, leftover_removed, leftover_added) =
        pair_changed(leftover_removed, leftover_added);

    let mut added: Vec<IndexedEquation> = leftover_added
        .into_iter()
        .map(|e| IndexedEquation {
            index: e.index,
            text: e.text,
        })
        .collect();
    added.sort_by_key(|e| e.index);
    let mut removed: Vec<IndexedEquation> = leftover_removed
        .into_iter()
        .map(|e| IndexedEquation {
            index: e.index,
            text: e.text,
        })
        .collect();
    removed.sort_by_key(|e| e.index);

    (added, removed, changed)
}

fn pair_changed(
    removed: Vec<EquationRow>,
    added: Vec<EquationRow>,
) -> (Vec<EquationChange>, Vec<EquationRow>, Vec<EquationRow>) {
    if removed.is_empty() || added.is_empty() {
        return (Vec::new(), removed, added);
    }
    let mut candidates = Vec::new();
    for (i, r) in removed.iter().enumerate() {
        let rt = normalize_equation(&r.text);
        for (j, add) in added.iter().enumerate() {
            let at = normalize_equation(&add.text);
            let max_len = rt.len().max(at.len());
            if max_len == 0 {
                continue;
            }
            let d = levenshtein(&rt, &at);
            let ratio = d as f64 / max_len as f64;
            if ratio < EQ_CHANGE_RATIO {
                candidates.push((ratio, i, j));
            }
        }
    }
    candidates.sort_by(|x, y| x.0.partial_cmp(&y.0).unwrap_or(std::cmp::Ordering::Equal));
    let mut used_r = HashSet::new();
    let mut used_a = HashSet::new();
    let mut changes = Vec::new();
    for (_, i, j) in candidates {
        if used_r.contains(&i) || used_a.contains(&j) {
            continue;
        }
        used_r.insert(i);
        used_a.insert(j);
        changes.push(EquationChange {
            index_old: removed[i].index,
            index_new: added[j].index,
            text_old: removed[i].text.clone(),
            text_new: added[j].text.clone(),
        });
    }
    changes.sort_by_key(|c| (c.index_old, c.index_new));
    let leftover_removed: Vec<EquationRow> = removed
        .into_iter()
        .enumerate()
        .filter(|(i, _)| !used_r.contains(i))
        .map(|(_, e)| e)
        .collect();
    let leftover_added: Vec<EquationRow> = added
        .into_iter()
        .enumerate()
        .filter(|(i, _)| !used_a.contains(i))
        .map(|(_, e)| e)
        .collect();
    (changes, leftover_removed, leftover_added)
}

fn levenshtein(a: &str, b: &str) -> usize {
    if a == b {
        return 0;
    }
    if a.is_empty() {
        return b.len();
    }
    if b.is_empty() {
        return a.len();
    }
    let b_chars: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b_chars.len()).collect();
    let mut curr = vec![0; b_chars.len() + 1];
    for (i, ca) in a.chars().enumerate() {
        curr[0] = i + 1;
        for (j, &cb) in b_chars.iter().enumerate() {
            let cost = if ca == cb { 0 } else { 1 };
            curr[j + 1] = (prev[j + 1] + 1).min(curr[j] + 1).min(prev[j] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[b_chars.len()]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse;

    const MODEL_A: &str = r#"
var c k;
varexo e;
parameters betta alpha delta;
betta = 0.99;
alpha = 0.33;
delta = 0.025;
model;
c = betta*c(+1);
k = (1-delta)*k(-1) + e;
end;
"#;

    const MODEL_B: &str = r#"
var c n;
varexo u;
parameters betta rho;
betta = 0.99;
rho = 0.9;
model;
c = betta*c(+1);
n = rho*n(-1) + u;
end;
"#;

    fn json_has_ss_key(v: &Value) -> bool {
        fn walk(v: &Value) -> bool {
            match v {
                Value::Object(map) => map.keys().any(|k| {
                    let lower = k.to_ascii_lowercase();
                    lower.contains("steady_state")
                        || lower.contains("steadystate")
                        || lower.contains("computed")
                        || walk(&map[k])
                }),
                Value::Array(items) => items.iter().any(walk),
                _ => false,
            }
        }
        walk(v)
    }

    #[test]
    fn same_model_has_common_names_no_adds() {
        let model = parse(MODEL_A);
        let diff = compare_models(&model, &model);
        assert!(!diff.common_endogenous.is_empty());
        assert!(!diff.common_parameters.is_empty());
        assert!(diff.added_endogenous.is_empty());
        assert!(diff.removed_endogenous.is_empty());
        assert!(diff.changed_parameter_values.is_empty());
        assert!(!json_has_ss_key(&diff.to_json()));
    }

    #[test]
    fn different_models_have_added_or_removed() {
        let a = parse(MODEL_A);
        let b = parse(MODEL_B);
        let diff = compare_models(&a, &b);
        let json = diff.to_json();
        assert!(json.get("added_endogenous").is_some());
        assert!(json.get("removed_endogenous").is_some());
        assert!(json.get("added_parameters").is_some());
        assert!(json.get("removed_parameters").is_some());
        assert!(json.get("added_equations").is_some());
        assert!(json.get("removed_equations").is_some());
        assert!(
            !diff.added_endogenous.is_empty()
                || !diff.removed_endogenous.is_empty()
                || !diff.added_equations.is_empty()
                || !diff.removed_equations.is_empty(),
            "expected declaration or equation differences"
        );
        assert!(!json_has_ss_key(&json));
        assert!(json.get("changed_steady_state_values").is_none());
    }

    #[test]
    fn parameter_value_change_is_reported() {
        let a = parse(MODEL_A);
        let mutated = MODEL_A.replacen("0.99", "0.95", 1);
        let b = parse(&mutated);
        let diff = compare_models(&a, &b);
        assert!(
            diff.changed_parameter_values
                .iter()
                .any(|p| p.name == "betta"),
            "expected betta in {:?}",
            diff.changed_parameter_values
        );
        assert!(!json_has_ss_key(&diff.to_json()));
        assert!(diff.to_markdown().contains("betta"));
    }

    #[test]
    fn compare_json_has_indexed_equations_no_commons() {
        let a = parse(MODEL_A);
        let b = parse(MODEL_B);
        let json = compare_models(&a, &b).to_json();
        assert!(json.get("common_equations").is_none());
        assert!(json.get("markdown").and_then(|v| v.as_str()).is_some());
        for key in ["added_equations", "removed_equations"] {
            let rows = json[key].as_array().expect(key);
            for row in rows {
                assert!(
                    row.get("index").and_then(|v| v.as_u64()).is_some(),
                    "{key} {row}"
                );
                assert!(
                    row.get("text").and_then(|v| v.as_str()).is_some(),
                    "{key} {row}"
                );
            }
        }
        let blob = json.to_string();
        assert!(
            !blob.contains("c = betta*c(+1)") && !blob.contains("c=betta*c(+1)"),
            "common Euler must be absent from compare JSON: {blob}"
        );
        assert!(json["changed_equations"]
            .as_array()
            .expect("changed")
            .is_empty());
        let added = json["added_equations"].as_array().expect("added");
        let removed = json["removed_equations"].as_array().expect("removed");
        assert_eq!(added.len(), 1);
        assert_eq!(added[0]["index"], 1);
        assert_eq!(added[0]["text"], "n = rho*n(-1)+u");
        assert_eq!(removed.len(), 1);
        assert_eq!(removed[0]["index"], 1);
        assert_eq!(removed[0]["text"], "k =(1-delta)*k(-1)+e");
        let md = json["markdown"].as_str().expect("markdown");
        assert!(md.contains("[1]"), "markdown must show indexes: {md}");
    }

    #[test]
    fn compare_skips_locals_and_static() {
        let base = "\
var y x;
model;
# helper = 1;
[static] x = 0;
y = 1;
end;
";
        let edited = base.replace("y = 1;", "y = 2;");
        let json = compare_models(&parse(base), &parse(&edited)).to_json();
        let blob = json.to_string();
        assert!(!blob.contains("helper"), "locals must be omitted: {blob}");
        assert!(!blob.contains("x = 0"), "[static] must be omitted: {blob}");
        let changed = json["changed_equations"].as_array().expect("changed");
        assert_eq!(changed.len(), 1, "{json}");
        assert_eq!(changed[0]["index_old"], 0);
        assert_eq!(changed[0]["index_new"], 0);
        assert_eq!(changed[0]["text_old"].as_str().expect("old"), "y = 1");
        assert_eq!(changed[0]["text_new"].as_str().expect("new"), "y = 2");
        let md = json["markdown"].as_str().expect("markdown");
        assert!(md.contains("[0 -> 0]"), "changed indexes in markdown: {md}");
    }

    #[test]
    fn compare_near_match_keeps_index_zero() {
        let a = "var y;\nmodel;\ny = 0.5*y(-1);\nend;\n";
        let b = "var y;\nmodel;\ny = 0.6*y(-1);\nend;\n";
        let json = compare_models(&parse(a), &parse(b)).to_json();
        assert!(json.get("common_equations").is_none());
        let changed = json["changed_equations"].as_array().expect("changed");
        assert_eq!(changed.len(), 1, "{json}");
        assert_eq!(changed[0]["index_old"], 0);
        assert_eq!(changed[0]["index_new"], 0);
        assert_eq!(json["added_equations"].as_array().unwrap().len(), 0);
        assert_eq!(json["removed_equations"].as_array().unwrap().len(), 0);
    }
}
