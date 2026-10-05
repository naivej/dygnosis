use std::collections::HashMap;

use dygnosis::{compare_models_with_sources, dynare_compare_models, parse, CompareSource};
use serde_json::{json, Value};

fn row<'a>(diff: &'a Value, id: &str) -> &'a Value {
    diff["navigation"]["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == id)
        .unwrap_or_else(|| panic!("missing {id}: {diff}"))
}

fn assert_pointers(diff: &Value) {
    let mut ids = std::collections::HashSet::new();
    for row in diff["navigation"]["rows"].as_array().unwrap() {
        let id = row["id"].as_str().unwrap();
        assert!(ids.insert(id), "duplicate navigation id {id}");
        assert!(diff.pointer(id).is_some(), "unassociated row {row}");
        assert!(row.get("before").is_some() && row.get("after").is_some());
    }
}

#[test]
fn direct_assignments_and_metadata_have_exact_legacy_rows() {
    let before = "parameters p;\np=1;\np=2;\nvar y(long_name='Before');\nmodel; y=p+y(-1); end;";
    let after = "parameters p;\np=4;\nvar y(long_name='After');\nmodel; y=2*p+y(-1); end;";
    let mut diff = dynare_compare_models(
        before,
        after,
        Some("before.mod"),
        Some("after.mod"),
        None,
        None,
        None,
    );
    assert_pointers(&diff);
    assert_eq!(diff["navigation"]["schema_version"], 1);
    let parameter = row(&diff, "/changed_parameter_values/0");
    assert_eq!(parameter["before"]["written_locations"][0]["line"], 3);
    assert_eq!(parameter["after"]["written_locations"][0]["line"], 2);
    let symbol = row(&diff, "/symbols_changed/0");
    assert_eq!(symbol["before"]["written_locations"][0]["line"], 4);
    assert_eq!(symbol["after"]["written_locations"][0]["line"], 3);
    let equation = row(&diff, "/changed_equations/0");
    assert_eq!(equation["index_old"], 0);
    assert_eq!(equation["index_new"], 0);
    assert_eq!(equation["domain"], "aggregate");
    assert_eq!(
        equation["before"]["written_locations"][0]["file"],
        "before.mod"
    );
    diff.as_object_mut().unwrap().remove("navigation");
    assert_eq!(
        diff,
        compare_models_with_sources(
            &parse(before),
            &parse(after),
            Some(CompareSource {
                text: before,
                origin_uri: Some("before.mod")
            }),
            Some(CompareSource {
                text: after,
                origin_uri: Some("after.mod")
            })
        )
        .to_json()
    );
}

#[test]
fn each_workspace_maps_its_own_include_and_revision() {
    let root = "@#include \"part.inc\"\n";
    let before = "parameters p; p=1; var y(long_name='Before'); varexo e;\nmodel; y=p+y(-1)+e; end;\nshocks; var e; stderr 1; end;";
    let after = before
        .replace("p=1", "p=2")
        .replace("Before", "After")
        .replace("y=p", "y=2*p")
        .replace("stderr 1", "stderr 2");
    let files_a = HashMap::from([
        ("before/root.mod".to_owned(), root.to_owned()),
        ("before/part.inc".to_owned(), before.to_owned()),
    ]);
    let files_b = HashMap::from([
        ("after/root.mod".to_owned(), root.to_owned()),
        ("after/part.inc".to_owned(), after.clone()),
    ]);
    let compare = |files_b: &HashMap<String, String>| {
        dynare_compare_models(
            root,
            root,
            Some("before/root.mod"),
            Some("after/root.mod"),
            Some(&files_a),
            Some(files_b),
            None,
        )
    };
    let diff = compare(&files_b);
    assert_pointers(&diff);
    assert_eq!(diff["navigation"]["before"]["complete"], true);
    assert_eq!(diff["navigation"]["after"]["complete"], true);
    for id in [
        "/changed_parameter_values/0",
        "/symbols_changed/0",
        "/changed_equations/0",
        "/shock_setup_changes/0",
    ] {
        let target = row(&diff, id);
        assert_eq!(
            target["before"]["written_locations"][0]["file"], "before/part.inc",
            "{target}"
        );
        assert_eq!(
            target["after"]["written_locations"][0]["file"], "after/part.inc",
            "{target}"
        );
    }
    // Old shock fields still withhold an incorrect root location for includes.
    assert!(diff["shock_setup_changes"][0]["before"]
        .get("location")
        .is_none());
    let mut edited = files_b.clone();
    edited.insert("after/part.inc".to_owned(), after.replace("p=2", "p=3"));
    let changed = compare(&edited);
    assert_eq!(
        diff["navigation"]["before"]["revision"],
        changed["navigation"]["before"]["revision"]
    );
    assert_ne!(
        diff["navigation"]["after"]["revision"],
        changed["navigation"]["after"]["revision"]
    );
    assert_eq!(changed["changed_parameter_values"][0]["new_value"], 3.0);
}

#[test]
fn macro_copies_keep_occurrence_and_pairing_identity() {
    let before = "var y_1 y_2; varexo e;\nmodel;\n@#for i in [1,2]\n[name='same'] y_@{i}=0.9*y_@{i}(-1)+e;\n@#endfor\nend;";
    let after = before.replace("0.9", "0.8");
    let diff = dynare_compare_models(before, &after, None, None, None, None, None);
    assert_pointers(&diff);
    let rows: Vec<_> = diff["navigation"]["rows"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| {
            row["id"]
                .as_str()
                .unwrap()
                .starts_with("/removed_equations/")
        })
        .collect();
    assert_eq!(rows.len(), 2, "{diff}");
    assert_ne!(
        rows[0]["before"]["occurrence_id"],
        rows[1]["before"]["occurrence_id"]
    );
    assert_eq!(
        rows[0]["before"]["written_locations"],
        rows[1]["before"]["written_locations"]
    );
    let mut legacy = diff.clone();
    legacy.as_object_mut().unwrap().remove("navigation");
    assert_eq!(
        legacy,
        compare_models_with_sources(
            &parse(before),
            &parse(&after),
            Some(CompareSource {
                text: before,
                origin_uri: None
            }),
            Some(CompareSource {
                text: &after,
                origin_uri: None
            })
        )
        .to_json()
    );
}

#[test]
fn scopes_and_unmatched_rows_join_the_correct_output() {
    let before = "heterogeneity_dimension d; var y; var(heterogeneity=d) x;\nmodel; [name='r'] y=y(-1); end;\nmodel(heterogeneity=d); [name='r'] x=x(-1); end;";
    let after = before
        .replace("y=y(-1)", "y=0.5*y(-1)")
        .replace("x=x(-1)", "x=0.8*x(-1)");
    let diff = dynare_compare_models(before, &after, None, None, None, None, None);
    assert_pointers(&diff);
    let aggregate = row(&diff, "/changed_equations/0");
    let dimension = row(&diff, "/heterogeneous_equations/0/changed/0");
    assert_eq!(aggregate["dimension"], Value::Null);
    assert_eq!(dimension["dimension"], "d");
    assert_eq!(aggregate["index_old"], 0);
    assert_eq!(dimension["index_old"], 0);
    assert_ne!(
        aggregate["before"]["occurrence_id"],
        dimension["before"]["occurrence_id"]
    );

    let before = "var y z; model; [name='same'] y=1; [name='same'] z=2; end;";
    let after = "var y z; model; [name='same'] y=sin(z)+exp(z)+log(z); end;";
    let diff = dynare_compare_models(before, after, None, None, None, None, None);
    assert_pointers(&diff);
    for row in diff["navigation"]["rows"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["kind"] == "equation")
    {
        if row["id"].as_str().unwrap().contains("/removed/") {
            assert!(row["before"].is_object() && row["after"].is_null(), "{row}");
        }
    }
}

#[test]
fn incomplete_input_withholds_mcp_comparison_and_navigation() {
    let before = "@#include \"missing-compare.inc\"\nvar y; model; y=1; end;";
    let after = "var y; model; y=2; end;";
    let diff = dynare_compare_models(before, after, None, None, None, None, None);
    assert_eq!(
        diff,
        json!({"status": "incomplete", "message": "Model expansion is incomplete"})
    );
}

#[test]
fn unnamed_mcp_compare_does_not_resolve_absolute_host_includes_for_navigation() {
    let include = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/writing/quiet_named.mod");
    assert!(include.is_file());
    let before = format!(
        "@#include \"{}\"\nvar z; model; z=1; end;",
        include.to_string_lossy().replace('\\', "/")
    );
    let after = before.replace("z=1", "z=2");
    let diff = dynare_compare_models(&before, &after, None, None, None, None, None);
    assert_eq!(
        diff,
        json!({"status": "incomplete", "message": "Model expansion is incomplete"})
    );
    let legacy = compare_models_with_sources(
        &parse(&before),
        &parse(&after),
        Some(CompareSource {
            text: &before,
            origin_uri: None,
        }),
        Some(CompareSource {
            text: &after,
            origin_uri: None,
        }),
    )
    .to_json();
    assert_eq!(legacy["common_endogenous"], serde_json::json!(["z"]));
    assert_eq!(legacy["changed_equations"][0]["text_old"], "z = 1");
}

#[test]
fn multi_file_equation_returns_only_contributing_segments() {
    let before = "var y; model;\ny =\n@#include \"rhs.inc\"\nend;";
    let files_a = HashMap::from([
        ("root.mod".to_owned(), before.to_owned()),
        ("rhs.inc".to_owned(), "1;".to_owned()),
    ]);
    let files_b = HashMap::from([
        ("root.mod".to_owned(), before.to_owned()),
        ("rhs.inc".to_owned(), "2;".to_owned()),
    ]);
    let diff = dynare_compare_models(
        before,
        before,
        Some("root.mod"),
        Some("root.mod"),
        Some(&files_a),
        Some(&files_b),
        None,
    );
    let locations = row(&diff, "/changed_equations/0")["before"]["written_locations"]
        .as_array()
        .unwrap();
    assert_eq!(locations.len(), 2, "{diff}");
    assert_eq!(locations[0]["file"], "root.mod");
    assert_eq!(locations[0]["line"], 2);
    assert_eq!(locations[0]["end_line"], 2);
    assert_eq!(locations[1]["file"], "rhs.inc");
    assert_eq!(locations[1]["line"], 1);
}

#[test]
fn macro_last_assignment_and_unpaired_shocks_keep_real_occurrences() {
    let before = "parameters p; varexo e;\n@#for i in [1,2]\np=@{i};\nshocks; var e; stderr @{i}; end;\n@#endfor\n";
    let after = before.replace("@{i}", "@{i+1}");
    let diff = dynare_compare_models(before, &after, None, None, None, None, None);
    let parameter = row(&diff, "/changed_parameter_values/0");
    assert_eq!(parameter["before"]["written_locations"][0]["line"], 3);
    assert_eq!(parameter["before"]["occurrence_id"], "s4");
    let legacy = compare_models_with_sources(
        &parse(before),
        &parse(&after),
        Some(CompareSource {
            text: before,
            origin_uri: None,
        }),
        Some(CompareSource {
            text: &after,
            origin_uri: None,
        }),
    )
    .to_json();
    assert_eq!(
        diff["changed_parameter_values"],
        legacy["changed_parameter_values"]
    );
    assert_eq!(diff["shock_setup_changes"], legacy["shock_setup_changes"]);
    for side in ["before", "after"] {
        let targets: Vec<_> = diff["navigation"]["rows"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|row| row["kind"] == "shock" && row[side].is_object())
            .collect();
        let expected = legacy["shock_setup_changes"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|row| row[side].is_object())
            .count();
        assert_eq!(targets.len(), expected);
        let ids: std::collections::HashSet<_> = targets
            .iter()
            .map(|row| row[side]["occurrence_id"].as_str().unwrap())
            .collect();
        assert_eq!(ids.len(), targets.len());
        for target in targets {
            assert_eq!(target[side]["written_locations"][0]["line"], 4);
        }
    }
    assert_pointers(&diff);
}

#[test]
fn exact_shock_cancellation_maps_the_remaining_pair() {
    let before = "varexo e;\nshocks; var e; stderr 1; end;\nshocks; var e; stderr 2; end;";
    let after = "varexo e;\nshocks; var e; stderr 2; end;\nshocks; var e; stderr 3; end;";
    let diff = dynare_compare_models(before, after, None, None, None, None, None);
    assert_eq!(diff["shock_setup_changes"].as_array().unwrap().len(), 1);
    let shock = row(&diff, "/shock_setup_changes/0");
    assert_eq!(shock["before"]["occurrence_id"], "h0");
    assert_eq!(shock["after"]["occurrence_id"], "h1");
    assert_eq!(shock["before"]["written_locations"][0]["line"], 2);
    assert_eq!(shock["after"]["written_locations"][0]["line"], 3);
}

#[test]
fn scalar_columns_do_not_count_utf16_units() {
    let before = "var y; model; /* 😀 */ y=1; end;\r\n";
    let after = before.replace("y=1", "y=2");
    let diff = dynare_compare_models(before, &after, None, None, None, None, None);
    let column = before[..before.find("y=1").unwrap()].chars().count() as u64 + 1;
    assert_eq!(
        row(&diff, "/changed_equations/0")["before"]["written_locations"][0]["column"],
        column
    );
}

use dygnosis::server::{new_service, Backend};
use tower_lsp::lsp_types::*;
use tower_lsp::LanguageServer;

async fn open(backend: &Backend, uri: Url, text: &str) {
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri,
                language_id: "dynare".to_owned(),
                version: 1,
                text: text.to_owned(),
            },
        })
        .await;
}

async fn lsp_compare(backend: &Backend, before: &Url, after: &Url) -> Value {
    backend
        .execute_command(ExecuteCommandParams {
            command: "dynare/compareModels".to_owned(),
            arguments: vec![serde_json::json!({"uri_a": before, "uri_b": after})],
            work_done_progress_params: WorkDoneProgressParams::default(),
        })
        .await
        .unwrap()
        .unwrap()
}

#[tokio::test]
async fn lsp_uses_utf16_and_current_overlay_revisions() {
    let source = "var y; model; /* 😀 */ y=1; end;\r\n";
    let after = source.replace("y=1", "y=2");
    let before_uri = Url::parse("untitled:before.mod").unwrap();
    let after_uri = Url::parse("untitled:after.mod").unwrap();
    let (service, _socket) = new_service();
    let backend = service.inner();
    open(backend, before_uri.clone(), source).await;
    open(backend, after_uri.clone(), &after).await;
    let diff = lsp_compare(backend, &before_uri, &after_uri).await;
    let location = &row(&diff, "/changed_equations/0")["before"]["written_locations"][0];
    assert_eq!(location["uri"], before_uri.as_str());
    assert_eq!(
        location["range"]["start"]["character"],
        source[..source.find("y=1").unwrap()].encode_utf16().count()
    );
    assert_eq!(
        diff["navigation"]["before"]["revision"],
        backend.model_input_revision(&before_uri).unwrap()
    );
    assert_eq!(
        diff["navigation"]["after"]["revision"],
        backend.model_input_revision(&after_uri).unwrap()
    );
    backend
        .did_change(DidChangeTextDocumentParams {
            text_document: VersionedTextDocumentIdentifier {
                uri: after_uri.clone(),
                version: 2,
            },
            content_changes: vec![TextDocumentContentChangeEvent {
                range: None,
                range_length: None,
                text: format!("\n{after}"),
            }],
        })
        .await;
    let edited = lsp_compare(backend, &before_uri, &after_uri).await;
    assert_eq!(
        diff["navigation"]["before"]["revision"],
        edited["navigation"]["before"]["revision"]
    );
    assert_ne!(
        diff["navigation"]["after"]["revision"],
        edited["navigation"]["after"]["revision"]
    );
    assert_eq!(
        row(&edited, "/changed_equations/0")["after"]["written_locations"][0]["range"]["start"]
            ["line"],
        1
    );
    let capability = serde_json::to_value(dygnosis::server::initialize_result()).unwrap();
    assert_eq!(
        capability["capabilities"]["experimental"]["dygnosis"]["compareModels"]
            ["navigation_schema_version"],
        1
    );
}

struct TempFiles(std::path::PathBuf);

impl TempFiles {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "dygnosis-compare-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn write(&self, path: &str, text: &str) -> Url {
        let path = self.0.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, text).unwrap();
        Url::from_file_path(path).unwrap()
    }
}

impl Drop for TempFiles {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[tokio::test]
async fn lsp_each_root_keeps_its_settings_disk_dependencies_and_include_overlays() {
    let files = TempFiles::new();
    let source = "@#include \"values.inc\"\nvar y; model; y=p; end;";
    let before_uri = files.write("before.mod", source);
    let after_uri = files.write("after.mod", source);
    let before_include = files.write("a/values.inc", "parameters p; p=1;\n");
    let after_include = files.write("b/values.inc", "parameters p; p=2;\n");
    files.write("c/values.inc", "parameters p; p=5;\n");
    let (service, _socket) = new_service();
    let backend = service.inner();
    let configure = |after: &str| {
        serde_json::json!({"dynare":{"searchPathsByRoot": {
            before_uri.as_str(): [files.0.join("a")], after_uri.as_str(): [files.0.join(after)],
        }}})
    };
    backend
        .did_change_configuration(DidChangeConfigurationParams {
            settings: configure("b"),
        })
        .await;
    let diff = lsp_compare(backend, &before_uri, &after_uri).await;
    assert_pointers(&diff);
    assert_eq!(diff["changed_parameter_values"][0]["old_value"], 1.0);
    assert_eq!(diff["changed_parameter_values"][0]["new_value"], 2.0);
    let parameter = row(&diff, "/changed_parameter_values/0");
    for (side, uri) in [("before", &before_include), ("after", &after_include)] {
        assert_eq!(
            dygnosis::include_resolver::normalize_uri(
                parameter[side]["written_locations"][0]["uri"]
                    .as_str()
                    .unwrap()
            ),
            dygnosis::include_resolver::normalize_uri(uri.as_str())
        );
    }
    // No watched-file event: comparing itself must observe changed disk input.
    files.write("b/values.inc", "parameters p; p=3;\n");
    let disk_edit = lsp_compare(backend, &before_uri, &after_uri).await;
    assert_eq!(disk_edit["changed_parameter_values"][0]["new_value"], 3.0);
    assert_eq!(
        diff["navigation"]["before"]["revision"],
        disk_edit["navigation"]["before"]["revision"]
    );
    assert_ne!(
        diff["navigation"]["after"]["revision"],
        disk_edit["navigation"]["after"]["revision"]
    );
    open(backend, after_include, "parameters p; p=4;\n").await;
    let overlay = lsp_compare(backend, &before_uri, &after_uri).await;
    assert_eq!(overlay["changed_parameter_values"][0]["new_value"], 4.0);
    assert_ne!(
        disk_edit["navigation"]["after"]["revision"],
        overlay["navigation"]["after"]["revision"]
    );
    backend
        .did_change_configuration(DidChangeConfigurationParams {
            settings: configure("c"),
        })
        .await;
    let settings = lsp_compare(backend, &before_uri, &after_uri).await;
    assert_eq!(settings["changed_parameter_values"][0]["old_value"], 1.0);
    assert_eq!(settings["changed_parameter_values"][0]["new_value"], 5.0);
    assert_eq!(
        overlay["navigation"]["before"]["revision"],
        settings["navigation"]["before"]["revision"]
    );
    assert_ne!(
        overlay["navigation"]["after"]["revision"],
        settings["navigation"]["after"]["revision"]
    );
}
