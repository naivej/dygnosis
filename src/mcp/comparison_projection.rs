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

type LocalPointers = BTreeMap<String, String>;

#[derive(Default)]
struct Owners {
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
}

fn array(value: &mut Value) -> Vec<Value> {
    value
        .take()
        .as_array_mut()
        .map(std::mem::take)
        .unwrap_or_default()
}

fn map_pointer(pointers: &mut LocalPointers, old: &str, new: String) {
    pointers.insert(old.to_owned(), new);
}

fn rebase_pointer(value: &mut Value, pointers: &LocalPointers) {
    let Some(old) = value.as_str() else { return };
    *value = pointers
        .get(old)
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
        return Err("Unsupported comparison section. Use semantic for model changes.".into());
    }
    if query.wants("navigation")
        && !["semantic", "references"]
            .into_iter()
            .any(|section| query.wants(section))
    {
        result.cause("refused", "NAVIGATION_OWNER_REQUIRED", "Comparison navigation needs returned facts. Select semantic or references with navigation.");
        *data = Value::Null;
        return Ok(());
    }
    let mut pages = Pages::new(
        "v4:dynare_compare_models",
        revision,
        arguments,
        query.output,
    )?;
    pages.share_budget();
    let semantic_complete = data["summary"]["semantic_rows"].is_u64();
    let reference_complete = data["summary"]["reference_membership_complete"] == true;
    let source_complete = data["summary"]["source_files"].is_u64();
    let source_selection_complete = source_complete && (!query.filtered() || semantic_complete);
    let mut rows = array(&mut data["semantic"]["rows"]);
    rows.retain(|row| query.matches(row));
    let owners = Owners::new(&rows);
    let available_rows = rows.len();
    let navigation = array(&mut data["navigation"]["rows"]);
    let source_files = source_context(&navigation, &owners);
    if query.wants("markdown") {
        data["markdown"] = json!(filtered_markdown(&rows));
    }
    if query.filtered() {
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
    let mut references = array(&mut data["semantic"]["references"]);
    references.retain(|reference| {
        reference["pointer"]
            .as_str()
            .is_some_and(|id| owners.references.contains_key(id))
    });
    if query.wants("references") {
        pages.collection_with_coverage("semantic.references", &mut references, reference_complete);
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
        row["reference_counts"] = json!({"available_matches":ids.len(),"total_matches":reference_complete.then_some(ids.len()),"returned_count":local.len()});
        row["references"] = json!(local);
    }
    for reference in &mut references {
        rebase_equation(reference, &pointers);
    }
    data["semantic"]["rows"] = json!(rows);
    data["semantic"]["references"] = json!(references);
    let mut available_files = 0;
    let mut unpaired_files = 0;
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
        unpaired_files = files
            .iter()
            .filter(|file| file["correspondence"] == "unpaired")
            .count();
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
        if let Some(target) = pointers.get(old) {
            let old = old.to_owned();
            let mut row = original;
            row["snapshot_pointer"] = json!(old);
            row["id"] = json!(target);
            rebase_equation(&mut row, &pointers);
            nav_rows.push(row);
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
    if query.wants("summary") {
        let summary = object.get_mut("summary").expect("comparison summary");
        summary["semantic_rows"] = json!(semantic_complete.then_some(available_rows));
        summary["available_semantic_rows"] = json!(available_rows);
        summary["source_files"] = json!(source_selection_complete.then_some(available_files));
        summary["available_source_files"] = json!(available_files);
        summary["paired_file_changes"] = json!(available_files - unpaired_files);
        summary["unpaired_files"] = json!(unpaired_files);
    } else {
        object.remove("summary");
    }
    if query.filtered()
        && !semantic_complete
        && (query.wants("source") || query.wants("source_text"))
    {
        result.cause_for("partial", "COMPARISON_FILTER_INCOMPLETE",
            "The filter used the available model changes. Other matching file changes could not be determined.",
            json!({"effect":"model_rows"}));
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

    #[test]
    fn configuration_switch_reports_executed_includes_without_a_written_macro_edit() {
        let root = "var y;\n@#ifndef POLICY\n@#define POLICY = 0\n@#endif\nmodel;\n@#if POLICY\n@#include \"policy.inc\"\n@#else\n@#include \"baseline.inc\"\n@#endif\nend;";
        let source = json!({"kind":"map","root":"main.mod","files":{
            "main.mod":root, "baseline.inc":"[name='Demand'] y=1;", "policy.inc":"[name='Demand'] y=2;"
        }});
        let result = call(json!({
            "comparison":{"kind":"sources","before":source,"after":source},
            "analysis_b":{"defines":[{"name":"POLICY","expression":"1"}]},
            "sections":["summary","semantic","source","navigation"], "detail":"full"
        }));
        let data = &result["data"];
        assert_eq!(data["summary"]["root_text"], "matching", "{result}");
        assert_eq!(
            data["summary"]["executed_includes"],
            json!({"before":["baseline.inc"],"after":["policy.inc"]})
        );
        let rows = data["semantic"]["rows"].as_array().unwrap();
        assert!(
            rows.iter()
                .any(|row| row["family"] == "equations" && row["name"] == "Demand")
        );
        assert!(rows.iter().all(|row| row["family"] != "macro_context"));
        assert!(data.get("changed_equations").is_none());
        check_pointers(data);
    }

    #[test]
    fn changed_macro_definitions_count_by_name_and_keep_their_written_lines() {
        let before = "// macro settings\n@#ifndef POLICY\n@#define POLICY = 0\n@#endif\n@#define FEEDBACK_SCALE = 1\nvar y; model; y=0; end;\n";
        let after = before
            .replace("POLICY = 0", "POLICY = 1")
            .replace("FEEDBACK_SCALE = 1", "FEEDBACK_SCALE = 2");
        let result = call(json!({
            "comparison":{"kind":"sources","before":{"kind":"text","text":before},"after":{"kind":"text","text":after}},
            "sections":["summary","semantic","navigation"],"detail":"full"
        }));
        let data = &result["data"];
        let rows = data["semantic"]["rows"].as_array().unwrap();
        assert_eq!(data["summary"]["semantic_rows"], 2, "{result}");
        assert_eq!(rows.len(), 2);
        for (row, (name, line, old, new)) in rows.iter().zip([
            ("POLICY", 3, "@#define POLICY = 0", "@#define POLICY = 1"),
            (
                "FEEDBACK_SCALE",
                5,
                "@#define FEEDBACK_SCALE = 1",
                "@#define FEEDBACK_SCALE = 2",
            ),
        ]) {
            assert_eq!(row["family"], "macro_context");
            assert_eq!(row["name"], name);
            assert_eq!(row["change"], "changed");
            assert_eq!(row["before"]["written_locations"][0]["line"], line);
            assert_eq!(row["after"]["written_locations"][0]["line"], line);
            let field = &row["fields"][0];
            assert_eq!(field["before"]["value"]["value"][0]["value"], old);
            assert_eq!(field["after"]["value"]["value"][0]["value"], new);
        }
        check_pointers(data);
    }

    #[test]
    fn inserted_macro_definition_keeps_only_the_edit_and_its_source_line() {
        let before = "@#define H = 1\n@#define H = 3\nvar y; model; y=@{H}; end;";
        let after = before.replace("@#define H = 3", "@#define H = 2\n@#define H = 3");
        let result = call(json!({
            "comparison":{"kind":"sources","before":{"kind":"text","text":before},"after":{"kind":"text","text":after}},
            "sections":["summary","semantic","navigation"],"detail":"full"
        }));
        let data = &result["data"];
        let rows = data["semantic"]["rows"].as_array().unwrap();
        assert_eq!(rows.len(), 1, "{result}");
        let row = &rows[0];
        assert_eq!(row["family"], "macro_context");
        assert_eq!(row["name"], "H");
        assert_eq!(row["change"], "changed");
        assert!(
            row["before"]["written_locations"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        assert_eq!(row["after"]["written_locations"][0]["line"], 2);
        let field = &row["fields"][0];
        assert_eq!(field["before"]["value"]["value"], json!([]));
        assert_eq!(
            field["after"]["value"]["value"],
            json!([{"kind":"text","value":"@#define H = 2"}])
        );
        check_pointers(data);
    }

    #[test]
    fn an_unchanged_macro_definition_stays_plain_when_it_moves_between_edits() {
        let before = "@#define H = 1\n@#define H = 2\n@#define H = 3\nvar y; model; y=@{H}; end;";
        let after = before
            .replace("@#define H = 1", "@#define H = 10\n@#define H = 11")
            .replace("@#define H = 3", "@#define H = 30");
        let result = call(json!({
            "comparison":{"kind":"sources","before":{"kind":"text","text":before},"after":{"kind":"text","text":after}},
            "sections":["semantic"],"detail":"full"
        }));
        let row = result["data"]["semantic"]["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["family"] == "macro_context")
            .unwrap();
        let expression = &row["expressions"][0];
        for side in ["before", "after"] {
            let unchanged: String = expression[side]["runs"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|run| run["role"] == "unchanged")
                .map(|run| run["text"].as_str().unwrap())
                .collect();
            assert_eq!(unchanged.trim(), "@#define H = 2", "{expression}");
        }
    }

    #[test]
    fn repeated_include_retains_one_written_edit_with_both_exact_targets() {
        let root =
            "var y;\n@#for j in 1:2\n@#include \"shared.inc\"\n@#endfor\nmodel; y=@{V}; end;";
        let side = |value| {
            json!({"kind":"map","root":"main.mod","files":{
                "main.mod":root, "shared.inc":format!("@#define V = {value}\n")
            }})
        };
        let result =
            call(json!({"comparison":{"kind":"sources","before":side(1),"after":side(2)}}));
        let data = &result["data"];
        let rows: Vec<_> = data["semantic"]["rows"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|row| row["family"] == "macro_context")
            .collect();
        assert_eq!(rows.len(), 1, "{result}");
        assert_eq!(rows[0]["name"], "V");
        assert_eq!(
            data["summary"]["executed_includes"],
            json!({"before":["shared.inc"],"after":["shared.inc"]})
        );
        for side in ["before", "after"] {
            assert_eq!(rows[0][side]["written_locations"][0]["file"], "shared.inc");
        }
        check_pointers(data);
    }

    #[test]
    fn identical_capture_reports_zero_changes_without_a_partial_outcome() {
        let mut args = inputs();
        args["comparison"]["after"] = args["comparison"]["before"].clone();
        let result = call(args);
        assert_eq!(result["outcome"]["status"], "ok", "{result}");
        assert_eq!(result["data"]["summary"]["semantic_rows"], 0);
        assert_eq!(result["data"]["summary"]["source_files"], 0);
        assert_eq!(result["data"]["summary"]["root_text"], "matching");
    }

    #[test]
    fn a_macro_directive_comment_is_a_captured_text_edit_only() {
        let before = "@#define SCALE = 1  \nvar y; model; [name='Law'] y=@{SCALE}; end;";
        let after = before.replace("1  \n", "1  // Added comment\n");
        let result = call(json!({
            "comparison":{"kind":"sources","before":{"kind":"text","text":before},"after":{"kind":"text","text":after}}
        }));
        let summary = &result["data"]["summary"];
        assert_eq!(summary["semantic_rows"], 0, "{result}");
        assert_eq!(summary["root_text"], "changed");
        assert_eq!(summary["source_files"], 1);
        assert_eq!(result["outcome"]["status"], "ok");
    }

    #[test]
    fn captured_statement_highlight_omission_reports_the_affected_family() {
        let model = |value| {
            let sum = vec![value; 160].join("+");
            format!("var y x; model; y=1;x=1;end; initval; y={sum}; x={sum}; end;")
        };
        let result = call(json!({
            "comparison":{"kind":"sources","before":{"kind":"text","text":model("1")},"after":{"kind":"text","text":model("2")}},
            "detail":"full"
        }));
        let summary = &result["data"]["summary"];
        assert_eq!(summary["semantic_rows"], 2, "{summary}");
        let issue = summary["issues"]
            .as_array()
            .unwrap()
            .iter()
            .find(|issue| issue["code"] == "written_statement_highlight_limit")
            .expect("omitted statement highlighting is explained");
        assert_eq!(issue["effect"], "highlights");
        assert_eq!(issue["family"], "steady_state");
        assert_eq!(issue["omitted"], 2);
        assert_eq!(result["outcome"]["status"], "partial");
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
        args["sections"] = json!(["summary", "semantic", "references", "source", "navigation"]);
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
        assert_eq!(counts["semantic.rows"], 3);
        assert_eq!(counts["source_changes.files"], 1);
    }

    #[test]
    fn dependent_parent_survives_a_page_without_the_parent_row() {
        // The parent identity is a name, not a local pointer into another page.
        let mut args = json!({"comparison":{"kind":"sources",
            "before":{"kind":"text","text":"parameters p q; p=1; q=p;"},
            "after":{"kind":"text","text":"parameters p q; p=2; q=p;"}},
            "families":["parameters"], "page_size":1});
        let first = call(args.clone());
        assert_eq!(first["data"]["semantic"]["rows"][0]["name"], "p");
        args["continuation"] = first["selection"]["continuation"].clone();
        args["expected_revision_a"] = first["inputs"][0]["revision"].clone();
        args["expected_revision_b"] = first["inputs"][1]["revision"].clone();
        let second = call(args);
        let row = &second["data"]["semantic"]["rows"][0];
        assert_eq!(row["name"], "q");
        let parent = row["fields"]
            .as_array()
            .unwrap()
            .iter()
            .find(|field| field["name"] == "dependent_on")
            .unwrap();
        assert_eq!(parent["before"]["value"]["value"], "p");
        assert_eq!(parent["after"], parent["before"]);
        assert_eq!(parent["changed"], false);
        check_pointers(&second["data"]);
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
    fn filters_select_matching_model_changes_and_reference_owners() {
        // Only references owned by the selected calibration rows should survive.
        let mut args = inputs();
        args["families"] = json!(["parameters"]);
        args["kinds"] = json!(["changed"]);
        args["sections"] = json!(["summary", "semantic", "references", "navigation", "source"]);
        args["page_size"] = json!(1000);
        let result = call(args);
        let data = &result["data"];
        assert_eq!(data["summary"]["available_semantic_rows"], 2);
        assert_eq!(data["semantic"]["rows"].as_array().unwrap().len(), 2);
        assert!(
            data["semantic"]["rows"]
                .as_array()
                .unwrap()
                .iter()
                .all(|row| row["family"] == "parameters")
        );
        assert!(data.get("common_parameters").is_none());
        for reference in data["semantic"]["references"].as_array().unwrap() {
            assert!(matches!(reference["symbol"].as_str(), Some("p" | "q")));
            assert!(
                reference["owners"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|owner| { owner.as_str().unwrap().starts_with("/semantic/rows/") })
            );
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
    fn references_keep_all_selected_owners_with_one_navigation_per_owner() {
        // A usage can support both a label change and a calibration change.
        let source = |label, value| json!({"kind":"text","text":format!("var y;parameters p (long_name='{label}');p={value};model;[name='eq'] y=p;end;")});
        let mut args = json!({"comparison":{"kind":"sources","before":source("Old",1),"after":source("New",2)},"sections":["semantic","references","navigation"],"page_size":1000});
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
        let targets = data["navigation"]["rows"].as_array().unwrap();
        assert_eq!(
            targets
                .iter()
                .filter(|row| row["id"] == parameter["pointer"])
                .count(),
            1
        );
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
    fn changed_symbol_kind_retains_both_declaration_targets() {
        // A kind transition keeps both written declarations on one owner.
        let before = "var y z;parameters p;p=1;model;[name='y'] y=0;[name='z'] z=p;end;";
        let after = "var z;parameters y p;y=0;p=1;model;[name='z'] z=p;end;";
        let result = call(
            json!({"comparison":{"kind":"sources","before":{"kind":"text","text":before},"after":{"kind":"text","text":after}},"families":["symbols"],"kinds":["changed"],"sections":["semantic","navigation"],"page_size":1000}),
        );
        let data = &result["data"];
        assert_eq!(data["semantic"]["rows"].as_array().unwrap().len(), 1);
        assert_eq!(data["semantic"]["rows"][0]["name"], "y");
        let targets = &data["navigation"]["rows"][0];
        assert!(targets["before"]["written_locations"][0]["line"].is_u64());
        assert!(targets["after"]["written_locations"][0]["line"].is_u64());
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
        assert_eq!(data["summary"]["paired_file_changes"], 1);
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
        let mut data = json!({"summary":{"semantic_rows":null,"source_files":1,"semantic_availability":"limit_exceeded","source_availability":"complete"},"semantic":{"availability":"limit_exceeded","rows":[{"pointer":"/semantic/rows/0","family":"parameters","change":"changed","name":"p","references":[]}],"references":[]},"source_changes":{"availability":"complete","files":[{"pointer":"/source_changes/files/0","correspondence":"proven_file_identity","before":{"file_key":"p.inc"}}]},"navigation":{"rows":[{"id":"/semantic/rows/0","before":{"written_locations":[{"file":"p.inc"}]}}]}});
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
