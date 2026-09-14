//! Structural compare of two parsed models. No numeric steady state, no solver.

use std::collections::{HashMap, HashSet};

use serde::Serialize;
use serde_json::Value;

use crate::equations::{equations, EquationRow};
use crate::model::{Assignment, Decl, Model};
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
