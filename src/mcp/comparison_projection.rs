//! Select and page retained comparison facts without changing shared model meaning.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value, json};

use super::result::{Detail, Pages, ResultOptions, ToolResult};

pub(super) struct Query<'a> {
    pub output: &'a ResultOptions,
    pub families: &'a [String],
    pub kinds: &'a [String],
}

impl Query<'_> {
    fn wants(&self, section: &str) -> bool {
        if self.output.sections.is_empty() {
            matches!(section, "summary" | "semantic" | "navigation")
        } else {
            self.output.sections.iter().any(|value| value == section)
        }
    }

    fn filtered(&self) -> bool {
        !self.families.is_empty() || !self.kinds.is_empty()
    }

    fn matches(&self, row: &Value) -> bool {
        (self.families.is_empty() || self.families.iter().any(|family| row["family"] == *family))
            && (self.kinds.is_empty() || self.kinds.iter().any(|kind| row["change"] == *kind))
    }
}

// A retained fact can be returned in both semantic and structural sections.
// Each copy needs its own valid local navigation target.
type LocalPointers = BTreeMap<String, Vec<String>>;

#[derive(Default)]
struct Owners {
    names: BTreeMap<String, BTreeSet<String>>,
    equations: BTreeMap<(String, Option<String>, u64), BTreeSet<String>>,
    selected: BTreeSet<String>,
    references: BTreeMap<String, BTreeSet<String>>,
}

impl Owners {
    fn new(rows: &[Value]) -> Self {
        let mut result = Self::default();
        for row in rows {
            let Some(pointer) = row["pointer"].as_str() else {
                continue;
            };
            result.selected.insert(pointer.to_owned());
            if matches!(row["family"].as_str(), Some("symbols" | "parameters")) {
                result
                    .names
                    .entry(row["name"].as_str().unwrap_or_default().to_owned())
                    .or_default()
                    .insert(pointer.to_owned());
            }
            if row["family"] == "equations" {
                for side in ["before", "after"] {
                    if let Some(index) = row[side]["equation_index"].as_u64() {
                        result
                            .equations
                            .entry((
                                side.to_owned(),
                                row[side]["scope"]["dimension"].as_str().map(str::to_owned),
                                index,
                            ))
                            .or_default()
                            .insert(pointer.to_owned());
                    }
                }
            }
            for reference in row["references"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
            {
                result
                    .references
                    .entry(reference.to_owned())
                    .or_default()
                    .insert(pointer.to_owned());
            }
        }
        result
    }

    fn contains_structural(&self, path: &str, index: usize, value: &Value) -> bool {
        if self.selected.contains(&format!("{path}/{index}")) {
            return true;
        }
        if let Some(name) = value.as_str() {
            return self.names.contains_key(name);
        }
        if path.contains("/unmatched_same_name/") {
            let side = if path.ends_with("/removed") {
                "before"
            } else {
                "after"
            };
            if let Some(index) = value["index"].as_u64() {
                return self.equations.contains_key(&(
                    side.to_owned(),
                    value["dimension"].as_str().map(str::to_owned),
                    index,
                ));
            }
        }
        false
    }
}

fn array(value: &mut Value) -> Vec<Value> {
    value
        .take()
        .as_array_mut()
        .map(std::mem::take)
        .unwrap_or_default()
}

fn map_pointer(pointers: &mut LocalPointers, old: &str, new: String) {
    pointers.entry(old.to_owned()).or_default().push(new);
}

fn rebase_pointer(value: &mut Value, pointers: &LocalPointers) {
    let Some(old) = value.as_str() else { return };
    *value = pointers
        .get(old)
        .and_then(|targets| targets.first())
        .map_or(Value::Null, |target| json!(target));
}

fn repair_context(value: &mut Value, pointers: &LocalPointers) {
    match value {
        Value::Object(object) => {
            if let Some(context) = object.get_mut("context").and_then(Value::as_object_mut)
                && let Some(pointer) = context.get_mut("pointer")
            {
                rebase_pointer(pointer, pointers);
            }
            for child in object.values_mut() {
                repair_context(child, pointers);
            }
        }
        Value::Array(rows) => {
            for row in rows {
                repair_context(row, pointers);
            }
        }
        _ => {}
    }
}

fn rebase_equation(value: &mut Value, pointers: &LocalPointers) {
    if value["equation_pointer"].is_string() {
        value["snapshot_equation_pointer"] = value["equation_pointer"].clone();
        rebase_pointer(&mut value["equation_pointer"], pointers);
    }
}

// Container indexes stay stable even when all their leaf entries are omitted.
// This order is part of the MCP projection contract, not JSON object ordering.
fn structural_paths(data: &Value) -> Vec<String> {
    let mut paths = [
        "added_endogenous",
        "removed_endogenous",
        "common_endogenous",
        "added_exogenous",
        "removed_exogenous",
        "common_exogenous",
        "added_parameters",
        "removed_parameters",
        "common_parameters",
        "changed_parameter_values",
        "symbols_changed",
        "added_equations",
        "removed_equations",
        "changed_equations",
    ]
    .into_iter()
    .map(|path| format!("/{path}"))
    .collect::<Vec<_>>();
    fn unmatched(paths: &mut Vec<String>, data: &Value, prefix: &str) {
        let path = format!("{prefix}/unmatched_same_name");
        for index in 0..data
            .pointer(&path)
            .and_then(Value::as_array)
            .map_or(0, Vec::len)
        {
            paths.push(format!("{path}/{index}/removed"));
            paths.push(format!("{path}/{index}/added"));
        }
    }
    unmatched(&mut paths, data, "");
    for index in 0..data["heterogeneous_equations"]
        .as_array()
        .map_or(0, Vec::len)
    {
        let prefix = format!("/heterogeneous_equations/{index}");
        for kind in ["added", "removed", "changed"] {
            paths.push(format!("{prefix}/{kind}"));
        }
        unmatched(&mut paths, data, &prefix);
    }
    paths.push("/shock_setup_changes".into());
    paths
}

fn source_context(nav: &[Value], owners: &Owners) -> [BTreeSet<Option<String>>; 2] {
    let mut files: [BTreeSet<Option<String>>; 2] = Default::default();
    for row in nav {
        if !row["id"]
            .as_str()
            .is_some_and(|id| owners.selected.contains(id))
        {
            continue;
        }
        for (index, side) in ["before", "after"].iter().enumerate() {
            for location in row[side]["written_locations"]
                .as_array()
                .into_iter()
                .flatten()
            {
                files[index].insert(
                    location["file_key"]
                        .as_str()
                        .or_else(|| location["file"].as_str())
                        .map(str::to_owned),
                );
            }
        }
    }
    files
}

fn filtered_markdown(rows: &[Value]) -> String {
    let mut text = String::from("# Model changes\n");
    for row in rows {
        let name = row["name"].as_str().unwrap_or_default().replace('`', "\\`");
        let family = row["family"].as_str().unwrap_or("model");
        let change = row["change"].as_str().unwrap_or("changed");
        text.push_str(&format!("\n- {family}: {change} `{name}`"));
        for field in row["fields"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|field| field["changed"] == true)
        {
            text.push_str(&format!(
                "\n  - {}: {} → {}",
                field["label"].as_str().unwrap_or("Value"),
                field["before"]["value"],
                field["after"]["value"]
            ));
        }
    }
    text.push('\n');
    text
}

pub(super) fn project(
    data: &mut Value,
    query: Query<'_>,
    arguments: &Value,
    revision: &str,
    result: &mut ToolResult,
) -> Result<(), String> {
    let allowed = [
        "summary",
        "structural",
        "semantic",
        "references",
        "source",
        "source_text",
        "navigation",
        "markdown",
    ];
    if query
        .output
        .sections
        .iter()
        .any(|section| !allowed.contains(&section.as_str()))
    {
        return Err("Unsupported comparison section".into());
    }
    if query.wants("navigation")
        && !["semantic", "structural", "references"]
            .into_iter()
            .any(|section| query.wants(section))
    {
        result.cause("refused", "NAVIGATION_OWNER_REQUIRED", "Comparison navigation needs returned facts. Select semantic, structural or references with navigation.");
        *data = Value::Null;
        return Ok(());
    }
    let mut pages = Pages::new(
        "v3:dynare_compare_models",
        revision,
        arguments,
        query.output,
    )?;
    pages.share_budget();
    let semantic_complete = data["semantic"]["availability"] == "complete";
    let source_complete = data["source_changes"]["availability"] == "complete";
    let semantic_availability = data["semantic"]["availability"].clone();
    let source_availability = data["source_changes"]["availability"].clone();
    let source_selection_complete = source_complete && (!query.filtered() || semantic_complete);
    let mut rows = array(&mut data["semantic"]["rows"]);
    rows.retain(|row| query.matches(row));
    let owners = Owners::new(&rows);
    let available_rows = rows.len();
    let navigation = array(&mut data["navigation"]["rows"]);
    let source_files = source_context(&navigation, &owners);
    if query.filtered() {
        if query.wants("markdown") {
            data["markdown"] = json!(filtered_markdown(&rows));
        }
        if query.wants("source_text") {
            for (index, side) in ["before", "after"].iter().enumerate() {
                let root = data["navigation"][side]["root_file"]
                    .as_str()
                    .or_else(|| data["navigation"][side]["root_uri"].as_str())
                    .map(str::to_owned);
                if let Some(files) = data["sources"][side].as_object_mut() {
                    files.retain(|file, _| {
                        source_files[index].contains(&Some(file.clone()))
                            || (source_files[index].contains(&None)
                                && root.as_ref().is_none_or(|root| root == file))
                    });
                }
            }
        }
    }
    let nav_index: BTreeMap<_, _> = navigation
        .iter()
        .filter_map(|row| Some((row["id"].as_str()?, row)))
        .collect();
    let mut pointers = LocalPointers::new();
    if query.wants("semantic") {
        pages.collection_with_coverage("semantic.rows", &mut rows, semantic_complete);
        for (index, row) in rows.iter_mut().enumerate() {
            let old = row["pointer"].as_str().unwrap_or_default().to_owned();
            let new = format!("/semantic/rows/{index}");
            if let Some(nav) = nav_index.get(old.as_str()) {
                for side in ["before", "after"] {
                    if row[side].is_object() {
                        row[side]["written_locations"] = nav[side]["written_locations"]
                            .as_array()
                            .map_or(json!([]), |locations| json!(locations));
                    }
                }
            }
            row["snapshot_pointer"] = json!(old);
            row["pointer"] = json!(new);
            map_pointer(&mut pointers, &old, new);
            if query.output.detail == Detail::Compact {
                strip_optional_detail(row);
            }
        }
    } else {
        rows.clear();
    }
    if query.wants("structural") {
        for path in structural_paths(data) {
            let Some(value) = data.pointer_mut(&path) else {
                continue;
            };
            let mut facts: Vec<_> = array(value)
                .into_iter()
                .enumerate()
                .filter(|(index, row)| {
                    !query.filtered() || owners.contains_structural(&path, *index, row)
                })
                .collect();
            let range = pages.projection_range(
                &path[1..].replace('/', "."),
                facts.len(),
                !query.filtered() || semantic_complete,
            );
            let mut output = Vec::with_capacity(range.len());
            for (index, (old_index, fact)) in facts.drain(range).enumerate() {
                map_pointer(
                    &mut pointers,
                    &format!("{path}/{old_index}"),
                    format!("{path}/{index}"),
                );
                output.push(fact);
            }
            *value = json!(output);
        }
    }
    if query.filtered()
        && !semantic_complete
        && (query.wants("structural") || query.wants("source"))
    {
        result.cause("partial", "COMPARISON_FILTER_INCOMPLETE", "Comparison filters use the retained semantic changes. Some semantic detail is unavailable; structural rows and source files without a retained matching change were omitted.");
    }
    let mut references = array(&mut data["semantic"]["references"]);
    references.retain(|reference| {
        reference["pointer"]
            .as_str()
            .is_some_and(|id| owners.references.contains_key(id))
    });
    if query.wants("references") {
        pages.collection_with_coverage("semantic.references", &mut references, semantic_complete);
        for (index, reference) in references.iter_mut().enumerate() {
            let old = reference["pointer"].as_str().unwrap_or_default().to_owned();
            reference["owners"] = json!(owners.references.get(&old));
            reference["snapshot_pointer"] = json!(old);
            let new = format!("/semantic/references/{index}");
            reference["pointer"] = json!(new);
            map_pointer(&mut pointers, &old, new);
        }
    } else {
        references.clear();
    }
    let returned_references: BTreeMap<_, _> = references
        .iter()
        .filter_map(|reference| {
            Some((
                reference["snapshot_pointer"].as_str()?,
                reference["pointer"].clone(),
            ))
        })
        .collect();
    for row in &mut rows {
        let ids = row["references"].as_array().cloned().unwrap_or_default();
        let local: Vec<_> = ids
            .iter()
            .filter_map(Value::as_str)
            .filter_map(|id| returned_references.get(id).cloned())
            .collect();
        row["reference_counts"] = json!({"available_matches":ids.len(),"total_matches":semantic_complete.then_some(ids.len()),"returned_count":local.len()});
        row["references"] = json!(local);
    }
    for reference in &mut references {
        rebase_equation(reference, &pointers);
    }
    data["semantic"]["rows"] = json!(rows);
    data["semantic"]["references"] = json!(references);
    let mut available_files = 0;
    if let Some(value) = data.pointer_mut("/source_changes/files") {
        let mut files = array(value);
        files.retain(|file| {
            !query.filtered()
                || ["before", "after"].iter().enumerate().any(|(index, side)| {
                    let key = file[side]["file_key"].as_str().map(str::to_owned);
                    key.as_ref()
                        .is_some_and(|_| source_files[index].contains(&key))
                        || (file["correspondence"] == "selected_roots"
                            && source_files[index].contains(&None))
                })
        });
        available_files = files.len();
        if query.wants("source") {
            pages.collection_with_coverage(
                "source_changes.files",
                &mut files,
                source_selection_complete,
            );
            for (index, file) in files.iter_mut().enumerate() {
                let old = file["pointer"].as_str().unwrap_or_default().to_owned();
                file["snapshot_pointer"] = json!(old);
                let new = format!("/source_changes/files/{index}");
                file["pointer"] = json!(new);
                map_pointer(&mut pointers, &old, new);
            }
        } else {
            files.clear();
        }
        *value = json!(files);
    }
    let mut nav_rows = Vec::new();
    for original in navigation {
        let Some(old) = original["id"].as_str() else {
            continue;
        };
        if let Some(targets) = pointers.get(old) {
            for target in targets {
                let mut row = original.clone();
                row["snapshot_pointer"] = json!(old);
                row["id"] = json!(target);
                rebase_equation(&mut row, &pointers);
                nav_rows.push(row);
            }
        }
    }
    data["navigation"]["rows"] = json!(nav_rows);
    repair_context(data, &pointers);
    let object = data.as_object_mut().expect("comparison object");
    for (section, field) in [
        ("source", "source_changes"),
        ("source_text", "sources"),
        ("navigation", "navigation"),
        ("markdown", "markdown"),
    ] {
        if !query.wants(section) {
            object.remove(field);
        }
    }
    if !query.wants("semantic") && !query.wants("references") {
        object.remove("semantic");
    }
    if !query.wants("structural") {
        let keep = [
            "semantic",
            "source_changes",
            "sources",
            "navigation",
            "coverage",
            "comparison_versions",
            "markdown",
        ];
        object.retain(|key, _| keep.contains(&key.as_str()));
    }
    if query.wants("summary") {
        object.insert("summary".into(), json!({"semantic_rows":semantic_complete.then_some(available_rows),"available_semantic_rows":available_rows,"source_files":source_selection_complete.then_some(available_files),"available_source_files":available_files,"semantic_availability":semantic_availability,"source_availability":source_availability}));
    }
    result.selection = pages.finish(query.output);
    Ok(())
}

fn strip_optional_detail(value: &mut Value) {
    match value {
        Value::Object(object) => {
            object.remove("runs");
            object.remove("statement_text");
            for child in object.values_mut() {
                strip_optional_detail(child);
            }
        }
        Value::Array(values) => {
            for child in values {
                strip_optional_detail(child);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp::{canonical, source::RequestControl};

    fn call(arguments: Value) -> Value {
        canonical::dispatch(
            "dynare_compare_models",
            arguments,
            RequestControl::default(),
        )
        .unwrap()
        .structured_content
        .unwrap()
    }

    fn inputs() -> Value {
        let before = "var y;parameters p q;p=1;q=2;model;[name='eq'] y=p+q;end;";
        let after = "var y;parameters p q;p=2;q=3;model;[name='eq'] y=p+q+1;end;";
        json!({"comparison":{"kind":"sources","before":{"kind":"text","text":before},"after":{"kind":"text","text":after}}})
    }

    fn check_pointers(data: &Value) {
        for row in data["semantic"]["rows"].as_array().into_iter().flatten() {
            assert!(data.pointer(row["pointer"].as_str().unwrap()).is_some());
            for reference in row["references"].as_array().into_iter().flatten() {
                assert!(data.pointer(reference.as_str().unwrap()).is_some());
            }
        }
        for row in data["navigation"]["rows"].as_array().into_iter().flatten() {
            assert!(
                data.pointer(row["id"].as_str().unwrap()).is_some(),
                "navigation target {row}"
            );
        }
        for row in data["semantic"]["references"]
            .as_array()
            .into_iter()
            .flatten()
            .chain(data["navigation"]["rows"].as_array().into_iter().flatten())
        {
            if let Some(pointer) = row["equation_pointer"].as_str() {
                assert!(data.pointer(pointer).is_some(), "equation target {row}");
            }
        }
    }

    #[test]
    fn canonical_comparison_pages_every_requested_fact_and_keeps_owners() {
        // Guards structural budget bypass, duplicate traversal and dangling pointers.
        let mut args = inputs();
        args["sections"] = json!([
            "summary",
            "semantic",
            "structural",
            "references",
            "source",
            "navigation"
        ]);
        args["page_size"] = json!(1);
        let mut identities = BTreeSet::new();
        let mut names = BTreeSet::new();
        let mut counts = BTreeMap::new();
        for page in 0..100 {
            let result = call(args.clone());
            let data = &result["data"];
            check_pointers(data);
            let collections = result["selection"]["collections"].as_object().unwrap();
            assert!(
                collections
                    .values()
                    .map(|count| count["returned_count"].as_u64().unwrap())
                    .sum::<u64>()
                    <= 1
            );
            for (key, count) in collections {
                assert_eq!(
                    counts
                        .entry(key.clone())
                        .or_insert_with(|| count["available_matches"].clone()),
                    &count["available_matches"]
                );
            }
            for row in data["semantic"]["rows"].as_array().unwrap() {
                assert!(identities.insert(row["snapshot_pointer"].clone().to_string()));
                names.insert(row["name"].as_str().unwrap().to_owned());
            }
            for reference in data["semantic"]["references"].as_array().unwrap() {
                assert!(!reference["owners"].as_array().unwrap().is_empty());
                for owner in reference["owners"].as_array().unwrap() {
                    assert!(owner.as_str().unwrap().starts_with('/'));
                }
            }
            if result["selection"]["continuation"].is_null() {
                break;
            }
            assert!(page < 99, "finite traversal");
            args["continuation"] = result["selection"]["continuation"].clone();
            args["expected_revision_a"] = result["inputs"][0]["revision"].clone();
            args["expected_revision_b"] = result["inputs"][1]["revision"].clone();
        }
        assert_eq!(
            names,
            BTreeSet::from(["p".to_owned(), "q".to_owned(), "eq".to_owned()])
        );
        assert_eq!(counts["changed_parameter_values"], 2);
        assert_eq!(counts["changed_equations"], 1);
        assert_eq!(counts["common_endogenous"], 1);
        assert_eq!(counts["common_parameters"], 2);
        assert_eq!(counts["source_changes.files"], 1);
    }

    #[test]
    fn default_context_does_not_expand_high_fanout_or_select_more_sections_with_full_detail() {
        // One calibration row must not serialize every usage under a one-row page.
        let equations = (0..300)
            .map(|index| format!("[name='e{index}'] y=p;\n"))
            .collect::<String>();
        let before = format!("var y;parameters p;p=1;model;{equations}end;");
        let after = before.replacen("p=1", "p=2", 1);
        let result = call(
            json!({"comparison":{"kind":"sources","before":{"kind":"text","text":before},"after":{"kind":"text","text":after}},"detail":"full","page_size":1}),
        );
        let data = &result["data"];
        let row = &data["semantic"]["rows"][0];
        assert_eq!(row["name"], "p");
        assert_eq!(row["reference_counts"]["available_matches"], 600);
        assert_eq!(row["reference_counts"]["returned_count"], 0);
        assert_eq!(row["references"], json!([]));
        assert_eq!(data["semantic"]["references"], json!([]));
        assert!(data.get("common_parameters").is_none());
        assert!(data.get("source_changes").is_none());
        assert!(data.get("sources").is_none());
        assert_eq!(data["navigation"]["rows"].as_array().unwrap().len(), 1);
        assert!(
            !row["before"]["written_locations"]
                .as_array()
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn filters_select_matching_structural_context_and_reference_owners() {
        // Structural common names and references must follow the chosen changes.
        let mut args = inputs();
        args["families"] = json!(["parameters"]);
        args["kinds"] = json!(["changed"]);
        args["sections"] = json!([
            "summary",
            "structural",
            "references",
            "navigation",
            "source"
        ]);
        args["page_size"] = json!(1000);
        let result = call(args);
        let data = &result["data"];
        assert_eq!(data["summary"]["available_semantic_rows"], 2);
        assert_eq!(data["common_endogenous"], json!([]));
        assert_eq!(data["common_parameters"], json!(["p", "q"]));
        assert_eq!(data["changed_equations"], json!([]));
        assert_eq!(
            data["changed_parameter_values"].as_array().unwrap().len(),
            2
        );
        for reference in data["semantic"]["references"].as_array().unwrap() {
            assert!(matches!(reference["symbol"].as_str(), Some("p" | "q")));
            assert!(reference["owners"].as_array().unwrap().iter().all(|owner| {
                owner
                    .as_str()
                    .unwrap()
                    .starts_with("/changed_parameter_values/")
            }));
        }
        check_pointers(data);
    }

    #[test]
    fn navigation_requires_a_selected_fact_section() {
        // An empty navigation-only result must not claim the model has no locations.
        let mut args = inputs();
        args["sections"] = json!(["navigation"]);
        let result = call(args);
        assert_eq!(result["outcome"]["status"], "refused");
        assert!(
            result["outcome"]["reasons"]
                .as_array()
                .unwrap()
                .iter()
                .any(|reason| reason["code"] == "NAVIGATION_OWNER_REQUIRED")
        );
        assert!(result["data"].is_null());
    }

    #[test]
    fn references_keep_all_selected_owners_and_duplicate_section_navigation() {
        // A usage can support both a label change and a calibration change.
        let source = |label, value| json!({"kind":"text","text":format!("var y;parameters p (long_name='{label}');p={value};model;[name='eq'] y=p;end;")});
        let mut args = json!({"comparison":{"kind":"sources","before":source("Old",1),"after":source("New",2)},"sections":["semantic","structural","references","navigation"],"page_size":1000});
        let result = call(args.clone());
        let data = &result["data"];
        check_pointers(data);
        let references = data["semantic"]["references"].as_array().unwrap();
        assert!(!references.is_empty());
        assert!(
            references
                .iter()
                .all(|reference| reference["owners"].as_array().unwrap().len() == 2)
        );
        let parameter = data["semantic"]["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["family"] == "parameters")
            .unwrap();
        for target in [
            parameter["pointer"].as_str().unwrap(),
            parameter["snapshot_pointer"].as_str().unwrap(),
        ] {
            assert!(
                data["navigation"]["rows"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|row| row["id"] == target)
            );
        }
        args["families"] = json!(["parameters"]);
        let filtered = call(args);
        assert!(
            filtered["data"]["semantic"]["references"]
                .as_array()
                .unwrap()
                .iter()
                .all(|reference| reference["owners"].as_array().unwrap().len() == 1)
        );
    }

    #[test]
    fn changed_symbol_kind_selects_both_structural_declaration_sides() {
        // A kind transition has one semantic owner and two declaration-list entries.
        let before = "var y z;parameters p;p=1;model;[name='y'] y=0;[name='z'] z=p;end;";
        let after = "var z;parameters y p;y=0;p=1;model;[name='z'] z=p;end;";
        let result = call(
            json!({"comparison":{"kind":"sources","before":{"kind":"text","text":before},"after":{"kind":"text","text":after}},"families":["symbols"],"kinds":["changed"],"sections":["structural","navigation"],"page_size":1000}),
        );
        let data = &result["data"];
        assert_eq!(data["removed_endogenous"], json!(["y"]));
        assert_eq!(data["added_parameters"], json!(["y"]));
        assert_eq!(data["common_endogenous"], json!([]));
        assert_eq!(data["removed_equations"], json!([]));
        check_pointers(data);
    }

    #[test]
    fn source_and_text_filters_follow_direct_change_files() {
        // A calibration filter must not include a separate file's symbol-label edit.
        let source = |p, label| json!({"kind":"map","root":"main.mod","files":{"main.mod":"var y;\n@#include \"p.inc\"\n@#include \"q.inc\"\nmodel;y=p;end;","p.inc":format!("parameters p;p={p};"),"q.inc":format!("parameters q (long_name='{label}');q=0;")}});
        let result = call(
            json!({"comparison":{"kind":"sources","before":source(1,"Old"),"after":source(2,"New")},"families":["parameters"],"sections":["summary","source","source_text","markdown"],"page_size":1000}),
        );
        let data = &result["data"];
        assert_eq!(data["summary"]["available_semantic_rows"], 1);
        assert_eq!(data["source_changes"]["files"].as_array().unwrap().len(), 1);
        assert_eq!(
            data["source_changes"]["files"][0]["before"]["file_key"],
            "p.inc"
        );
        assert_eq!(
            data["sources"]["before"]
                .as_object()
                .unwrap()
                .keys()
                .collect::<Vec<_>>(),
            vec!["p.inc"]
        );
        let markdown = data["markdown"].as_str().unwrap();
        assert!(markdown.contains("`p`"));
        assert!(!markdown.contains("`q`"));
    }

    #[test]
    fn partial_semantic_membership_cannot_supply_exact_filtered_source_totals() {
        // Source capture can be complete while filtered change membership is limited.
        let mut data = json!({"semantic":{"availability":"limit_exceeded","rows":[{"pointer":"/semantic/rows/0","family":"parameters","change":"changed","name":"p","references":[]}],"references":[]},"source_changes":{"availability":"complete","files":[{"pointer":"/source_changes/files/0","correspondence":"proven_file_identity","before":{"file_key":"p.inc"}}]},"navigation":{"rows":[{"id":"/semantic/rows/0","before":{"written_locations":[{"file":"p.inc"}]}}]}});
        let output = ResultOptions {
            sections: vec!["summary".into(), "source".into()],
            page_size: Some(1),
            ..Default::default()
        };
        let mut result = ToolResult::new(Value::Null);
        project(
            &mut data,
            Query {
                output: &output,
                families: &["parameters".into()],
                kinds: &[],
            },
            &json!({}),
            "revision",
            &mut result,
        )
        .unwrap();
        assert_eq!(data["summary"]["semantic_availability"], "limit_exceeded");
        assert_eq!(data["summary"]["source_availability"], "complete");
        assert!(data["summary"]["source_files"].is_null());
        assert!(
            result.selection.unwrap()["collections"]["source_changes.files"]["total_matches"]
                .is_null()
        );
        assert_eq!(result.outcome.status, "partial");
    }
}
