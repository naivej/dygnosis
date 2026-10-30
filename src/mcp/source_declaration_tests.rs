//! MCP regressions for declaration facts selected from an inactive source use.

use super::{canonical, source::RequestControl};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

fn call(tool: &str, args: Value) -> Value {
    canonical::dispatch(tool, args, RequestControl::default())
        .unwrap()
        .structured_content
        .unwrap()
}

fn include_model(include: &str) -> String {
    format!(
        "@#if 1\nvar mu_cngdp (long_name='Active consumption');\nmodel;\nmu_cngdp=0;\nend;\n@#else\n@#include \"{include}\"\nmodel;\ny=mu_cngdp(1);\nend;\n@#endif\n"
    )
}

fn target_query(source: Value, file: Option<&str>, line: u32, column: u32) -> Value {
    let mut symbol = file.map_or_else(
        || json!({"line":line,"column":column}),
        |file| json!({"file":file,"line":line,"column":column}),
    );
    symbol["kind"] = json!("target");
    call(
        "dynare_symbol_info",
        json!({
            "source": source,
            "view": "source",
            "symbol": symbol
        }),
    )
}

fn assert_source_only_candidate(result: &Value, expected_file_suffix: &str, expected_name: &str) {
    let rows = result["data"]["candidates"]
        .as_array()
        .expect("candidate rows");
    assert_eq!(rows.len(), 1, "{result}");
    let row = &rows[0];
    assert_eq!(row["name"], expected_name, "{result}");
    assert_eq!(row["kind"], "declaration", "{result}");
    assert_eq!(row["verified_binding"], false, "{result}");
    assert_eq!(row["label"], "source candidate", "{result}");
    assert!(
        row["file"]
            .as_str()
            .is_some_and(|file| file.ends_with(expected_file_suffix)),
        "{result}"
    );
    assert_eq!(row["written_kind"], "var", "{result}");
    assert_eq!(row["long_name"], "Dormant consumption", "{result}");
    assert_eq!(row["tex_name"], "C", "{result}");
    assert!(row["range"].is_object(), "{result}");
    assert!(row["selection_range"].is_object(), "{result}");
    assert!(row["source_revision"].is_string(), "{result}");
    assert!(row["source_id"].is_string(), "{result}");
    for forbidden in [
        "values",
        "assignment_values",
        "timing",
        "binding_id",
        "references",
    ] {
        assert!(
            row.get(forbidden).is_none(),
            "unexpected {forbidden}: {result}"
        );
    }
}

#[test]
fn inactive_use_selects_only_the_literal_include_declaration_facts() {
    // Guards inactive x(1) lookup returning its written declaration without active sibling metadata or semantic facts.
    let root = "Models/./main.mod";
    let child = "Models/./nested/../nested/parameters_initial.inc";
    let root_text = include_model("nested/../nested/parameters_initial.inc");
    let child_text = "var mu_cngdp $C$ (long_name='Dormant consumption');\n";
    let source = json!({
        "kind": "map",
        "root": root,
        "files": {(root): root_text, (child): child_text}
    });

    let effective_before = call(
        "dynare_model_info",
        json!({"source": source, "sections": ["summary", "analysis"]}),
    );
    let hovered = target_query(source.clone(), Some(root), 9, 5);
    assert_source_only_candidate(&hovered, "parameters_initial.inc", "mu_cngdp");
    assert_eq!(
        hovered["data"]["candidates"][0]["file"], child,
        "map source identity must preserve the exact supplied key: {hovered}"
    );
    let effective_after = call(
        "dynare_model_info",
        json!({"source": source, "sections": ["summary", "analysis"]}),
    );
    assert_eq!(effective_before, effective_after);
}

#[test]
fn inactive_same_file_text_use_has_one_candidate_and_no_synthetic_file_key() {
    // Guards Text source metadata staying fileless and preserving declaration metadata at an inactive use.
    let text = "@#if 1\nvar mu_cngdp (long_name='Active consumption');\nmodel;\nmu_cngdp=0;\nend;\n@#else\nvar mu_cngdp $C$ (long_name='Dormant consumption');\nmodel;\ny=mu_cngdp(1);\nend;\n@#endif\n";
    let result = target_query(json!({"kind":"text","text":text}), None, 9, 5);
    let rows = result["data"]["candidates"]
        .as_array()
        .expect("candidate rows");
    assert_eq!(
        rows.len(),
        1,
        "the selected declaration was duplicated: {result}"
    );
    let row = &rows[0];
    assert_eq!(row["name"], "mu_cngdp", "{result}");
    assert_eq!(
        row["file"],
        Value::Null,
        "synthetic Text path leaked: {result}"
    );
    assert_eq!(row["long_name"], "Dormant consumption", "{result}");
    assert_eq!(row["tex_name"], "C", "{result}");
    assert_eq!(row["verified_binding"], false, "{result}");
}

#[test]
fn typed_name_and_target_selectors_return_written_sites_and_local_expression_only() {
    // Guards typed name selectors and target use lookup returning written sites without model facts.
    let text = "@#if 0\nvar y; parameters beta; beta=.9;\npac_model(model_name=pm,discount=beta,auxiliary_model_name=vm);\ntrend_component_model(model_name=vm,eqtags=['Y'],targets=['Y']);\nexternal_function(name='ext',nargs=1,first_deriv_provided='deriv');\nmodel;\n#scale=beta;\ny=scale+ext(y(-1));\nend;\n@#endif\n";
    let source = json!({"kind":"text","text":text});

    let by_name = call(
        "dynare_symbol_info",
        json!({
            "source": source,
            "view": "source",
            "symbol": {"kind":"name","name":"vm"}
        }),
    );
    let rows = by_name["data"]["candidates"]
        .as_array()
        .expect("name selector candidates");
    assert_eq!(rows.len(), 1, "duplicate typed declaration: {by_name}");
    assert_eq!(rows[0]["name"], "vm", "{by_name}");
    assert_eq!(
        rows[0]["written_kind"], "trend_component_model",
        "{by_name}"
    );
    assert_eq!(rows[0]["verified_binding"], false, "{by_name}");

    let auxiliary_column = text.lines().nth(2).unwrap().find("vm").unwrap() as u32 + 1;
    let auxiliary = target_query(source.clone(), None, 3, auxiliary_column);
    let candidate = auxiliary["data"]["candidates"]
        .as_array()
        .and_then(|rows| rows.first())
        .expect("forward auxiliary model declaration");
    assert_eq!(candidate["name"], "vm", "{auxiliary}");
    assert_eq!(
        candidate["written_kind"], "trend_component_model",
        "{auxiliary}"
    );
    assert_eq!(candidate["verified_binding"], false, "{auxiliary}");

    let local_use = target_query(source, None, 8, 3);
    let candidate = local_use["data"]["candidates"]
        .as_array()
        .and_then(|rows| rows.first())
        .expect("model-local use declaration");
    assert_eq!(candidate["name"], "scale", "{local_use}");
    assert_eq!(
        candidate["written_kind"], "model_local_variable",
        "{local_use}"
    );
    assert_eq!(candidate["expression"], "beta", "{local_use}");
    assert_eq!(candidate["verified_binding"], false, "{local_use}");
    for forbidden in [
        "values",
        "assignment_values",
        "timing",
        "binding_id",
        "references",
    ] {
        assert!(
            candidate.get(forbidden).is_none(),
            "unexpected {forbidden}: {local_use}"
        );
    }
}

#[test]
fn replaced_subsample_head_does_not_select_an_old_label() {
    // Guards stale labels after a declaration or a copy in a compatible child branch.
    for (text, line, cause) in [
        (
            "parameters a;\n@#if 0\na.subsamples(short=2000Q1:2001Q1);\na.subsamples(long=2002Q1:2003Q1);\na.short.options(init=.4);\n@#endif\n",
            5,
            "Multiple written subsample declarations",
        ),
        (
            "parameters a b;\n@#if 0\na.subsamples(short=2000Q1:2001Q1);\nb.subsamples(long=2002Q1:2003Q1);\n@#if FLAG\na.subsamples=b.subsamples;\n@#endif\na.short.options(init=.4);\n@#endif\n",
            8,
            "Written subsample copies",
        ),
    ] {
        let result = target_query(json!({"kind":"text","text":text}), None, line, 3);
        assert_eq!(
            result["data"]["candidates"].as_array().map(Vec::len),
            Some(0),
            "{result}"
        );
        assert!(
            reason_code(&result, "SOURCE_DECLARATION_UNAVAILABLE"),
            "{result}"
        );
        assert!(result.to_string().contains(cause), "{result}");
    }
}

struct DiskFiles(PathBuf);

impl DiskFiles {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let directory = std::env::temp_dir().join(format!(
            "dygnosis-mcp-source-declaration-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&directory).unwrap();
        Self(directory)
    }

    fn path(&self, name: &str) -> String {
        self.0.join(name).to_string_lossy().into_owned()
    }

    fn root(&self) -> String {
        self.path("root.mod")
    }

    fn write(&self, name: &str, text: &str) {
        let path = self.0.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
}

impl Drop for DiskFiles {
    fn drop(&mut self) {
        let temp = std::env::temp_dir().canonicalize().unwrap();
        let path = self.0.canonicalize().unwrap();
        assert_eq!(path.parent(), Some(temp.as_path()));
        std::fs::remove_dir_all(path).unwrap();
    }
}

fn disk_source(root: String, include_dirs: Vec<String>) -> Value {
    json!({"kind":"disk","root":root,"include_dirs":include_dirs})
}

fn reason_code(result: &Value, code: &str) -> bool {
    result["outcome"]["reasons"]
        .as_array()
        .is_some_and(|reasons| reasons.iter().any(|reason| reason["code"] == code))
}

#[test]
fn dormant_include_edit_invalidates_same_length_source_revision() {
    // Guards a stale expected_revision being accepted after same-length metadata changes in an unopened include.
    let files = DiskFiles::new();
    files.write("root.mod", &include_model("nested/parameters_initial.inc"));
    files.write(
        "nested/parameters_initial.inc",
        "var mu_cngdp $C$ (long_name='Dormant consumption');\n",
    );
    let source = disk_source(files.root(), vec![]);
    let first = target_query(source.clone(), None, 9, 5);
    assert_source_only_candidate(&first, "parameters_initial.inc", "mu_cngdp");
    let revision = first["inputs"][0]["revision"].as_str().unwrap();

    files.write(
        "nested/parameters_initial.inc",
        "var mu_cngdp $C$ (long_name='Dormant consumptioN');\n",
    );
    let stale = call(
        "dynare_symbol_info",
        json!({
            "source": source,
            "view": "source",
            "symbol": {"kind":"target","line":9,"column":5},
            "expected_revision": revision
        }),
    );
    assert!(reason_code(&stale, "INPUT_CHANGED"), "{stale}");
    let refreshed = target_query(disk_source(files.root(), vec![]), None, 9, 5);
    let row = &refreshed["data"]["candidates"][0];
    assert_eq!(row["long_name"], "Dormant consumptioN", "{refreshed}");
    assert_ne!(
        row["source_revision"], first["data"]["candidates"][0]["source_revision"],
        "{refreshed}"
    );
}

#[test]
fn dormant_child_edit_during_source_capture_returns_no_candidate() {
    // Guards a child edited after candidate acquisition from being paired with the old checked revision.
    use super::source::{self, CaptureBoundary};

    let files = DiskFiles::new();
    files.write("root.mod", &include_model("nested/parameters_initial.inc"));
    files.write(
        "nested/parameters_initial.inc",
        "var mu_cngdp $C$ (long_name='Dormant consumption');\n",
    );
    let source = disk_source(files.root(), vec![]);
    let changed_path = files.0.join("nested/parameters_initial.inc");
    let result = source::with_capture_hook(
        CaptureBoundary::AnalysisMetadata,
        move || {
            std::fs::write(
                changed_path,
                "var mu_cngdp $C$ (long_name='Changed consumption');\n",
            )
            .unwrap();
        },
        || {
            call(
                "dynare_symbol_info",
                json!({
                    "source": source,
                    "view": "source",
                    "symbol": {"kind":"target","line":9,"column":5}
                }),
            )
        },
    );
    assert!(reason_code(&result, "INPUT_CHANGED"), "{result}");
    assert!(
        result["data"]["candidates"]
            .as_array()
            .is_none_or(|rows| rows.is_empty()),
        "a candidate escaped from the changed source snapshot: {result}"
    );
}

#[test]
fn newly_present_preferred_dormant_include_invalidates_old_revision_and_wins_lookup() {
    // Guards a missing higher-priority include path escaping the revision and retaining the lower-priority declaration.
    let files = DiskFiles::new();
    files.write("root.mod", &include_model("parameters_initial.inc"));
    files.write(
        "search/parameters_initial.inc",
        "var mu_cngdp $C$ (long_name='Dormant consumption');\n",
    );
    let source = disk_source(files.root(), vec![files.path("search")]);
    let first = target_query(source.clone(), None, 9, 5);
    assert_source_only_candidate(&first, "parameters_initial.inc", "mu_cngdp");
    let revision = first["inputs"][0]["revision"].as_str().unwrap();
    assert!(
        first["data"]["candidates"][0]["file"]
            .as_str()
            .unwrap()
            .contains("search"),
        "{first}"
    );

    files.write(
        "parameters_initial.inc",
        "var mu_cngdp $C$ (long_name='Preferred consumption');\n",
    );
    let stale = call(
        "dynare_symbol_info",
        json!({
            "source": source,
            "view": "source",
        "symbol": {"kind":"target","line":9,"column":5},
            "expected_revision": revision
        }),
    );
    assert!(reason_code(&stale, "INPUT_CHANGED"), "{stale}");

    let refreshed = target_query(
        disk_source(files.root(), vec![files.path("search")]),
        None,
        9,
        5,
    );
    let row = &refreshed["data"]["candidates"][0];
    assert_eq!(row["long_name"], "Preferred consumption", "{refreshed}");
    assert!(
        row["file"].as_str().is_some_and(
            |file| file.ends_with("parameters_initial.inc") && !file.contains("search")
        ),
        "{refreshed}"
    );
}
