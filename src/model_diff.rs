//! Structural compare of two parsed models. No numeric steady state, no solver.

use std::collections::{HashMap, HashSet};

use serde::Serialize;
use serde_json::Value;

use crate::model::{Assignment, Decl, Equation, Model};
use crate::model_info::assigned_number;

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
    pub old_text: String,
    pub new_text: String,
    pub line_old: u32,
    pub line_new: u32,
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
    pub added_equations: Vec<String>,
    pub removed_equations: Vec<String>,
    pub common_equations: Vec<String>,
    pub changed_equations: Vec<EquationChange>,
}

impl ModelDiff {
    pub fn to_json(&self) -> Value {
        serde_json::to_value(self).unwrap_or(Value::Null)
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

        if !self.changed_equations.is_empty() {
            lines.push(String::new());
            lines.push("## Changed equations".into());
            for e in &self.changed_equations {
                lines.push(format!(
                    "- L{} -> L{}: `{}` -> `{}`",
                    e.line_old, e.line_new, e.old_text, e.new_text
                ));
            }
        }

        if !self.added_equations.is_empty() {
            lines.push(String::new());
            lines.push("## Added equations".into());
            for eq in &self.added_equations {
                lines.push(format!("- `{eq}`"));
            }
        }

        if !self.removed_equations.is_empty() {
            lines.push(String::new());
            lines.push("## Removed equations".into());
            for eq in &self.removed_equations {
                lines.push(format!("- `{eq}`"));
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
    let end_a = names(model_a, &model_a.endogenous);
    let end_b = names(model_b, &model_b.endogenous);
    let exo_a = names(model_a, &model_a.exogenous);
    let exo_b = names(model_b, &model_b.exogenous);
    let par_a = names(model_a, &model_a.parameters);
    let par_b = names(model_b, &model_b.parameters);

    let common_params: HashSet<String> = par_a.intersection(&par_b).cloned().collect();
    let changed_parameter_values = changed_params(model_a, model_b, &common_params);

    let (added_eq, removed_eq, common_eq, changed_eq) = diff_equations(model_a, model_b);

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
        common_equations: common_eq,
        changed_equations: changed_eq,
    }
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

fn eq_line(model: &Model, eq: &Equation) -> u32 {
    crate::span::LineIndex::new(&model.source)
        .position(&model.source, eq.span.start)
        .line
        + 1
}

fn diff_equations(
    a: &Model,
    b: &Model,
) -> (Vec<String>, Vec<String>, Vec<String>, Vec<EquationChange>) {
    let mut norm_a: HashMap<String, Vec<&Equation>> = HashMap::new();
    for eq in &a.equations {
        let key = normalize_equation(&eq.text);
        if !key.is_empty() {
            norm_a.entry(key).or_default().push(eq);
        }
    }
    let mut norm_b: HashMap<String, Vec<&Equation>> = HashMap::new();
    for eq in &b.equations {
        let key = normalize_equation(&eq.text);
        if !key.is_empty() {
            norm_b.entry(key).or_default().push(eq);
        }
    }

    let keys_a: HashSet<String> = norm_a.keys().cloned().collect();
    let keys_b: HashSet<String> = norm_b.keys().cloned().collect();
    let common: Vec<String> = sorted_intersect(&keys_a, &keys_b);

    let mut leftover_removed = Vec::new();
    let mut leftover_added = Vec::new();
    for key in keys_a.union(&keys_b) {
        let na = norm_a.get(key).map(|v| v.len()).unwrap_or(0);
        let nb = norm_b.get(key).map(|v| v.len()).unwrap_or(0);
        let shared = na.min(nb);
        if let Some(list) = norm_a.get(key) {
            leftover_removed.extend(list.iter().skip(shared).copied());
        }
        if let Some(list) = norm_b.get(key) {
            leftover_added.extend(list.iter().skip(shared).copied());
        }
    }

    let (changed, leftover_removed, leftover_added) =
        pair_changed(a, b, leftover_removed, leftover_added);

    let added: Vec<String> = leftover_added
        .into_iter()
        .map(|e| normalize_equation(&e.text))
        .collect();
    let mut added = added;
    added.sort();
    let mut removed: Vec<String> = leftover_removed
        .into_iter()
        .map(|e| normalize_equation(&e.text))
        .collect();
    removed.sort();

    (added, removed, common, changed)
}

fn pair_changed<'a>(
    model_a: &Model,
    model_b: &Model,
    removed: Vec<&'a Equation>,
    added: Vec<&'a Equation>,
) -> (Vec<EquationChange>, Vec<&'a Equation>, Vec<&'a Equation>) {
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
            old_text: normalize_equation(&removed[i].text),
            new_text: normalize_equation(&added[j].text),
            line_old: eq_line(model_a, removed[i]),
            line_new: eq_line(model_b, added[j]),
        });
    }
    changes.sort_by_key(|c| (c.line_old, c.line_new));
    let leftover_removed: Vec<&Equation> = removed
        .into_iter()
        .enumerate()
        .filter(|(i, _)| !used_r.contains(i))
        .map(|(_, e)| e)
        .collect();
    let leftover_added: Vec<&Equation> = added
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
}
