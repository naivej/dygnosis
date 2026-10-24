//! Regression checks for request-local configuration and source-only MCP views.

use super::{canonical, source::RequestControl};
use serde_json::{Value, json};

const MODEL: &str = "@#ifdef ALT\nvar y z;\nmodel;\n[name='alternative'] y=z;\n[name='extra'] z=0;\nend;\n@#else\nvar y;\nmodel;\n[name='base'] y=0;\nend;\n@#endif\n";

fn source(text: &str) -> Value {
    json!({"kind":"map","root":"main.mod","files":{"main.mod":text}})
}
fn call(tool: &str, args: Value) -> Value {
    canonical::dispatch(tool, args, RequestControl::default())
        .unwrap()
        .structured_content
        .unwrap()
}
fn alternative() -> Value {
    json!({"name":"Alternative","defines":[{"name":"ALT","expression":"false"}]})
}

#[test]
fn equation_selection_reaches_mcp_facts_comparisons_and_edit_guards() {
    // Guards a transport dropping invocation choices, comparing Source twice,
    // or offering edits whose full-source binding proof is unavailable.
    let input = source("var a b; model; [name='a'] a=b; [name='b'] b=b(-1); end;");
    let analysis = json!({"equation_selection":{"mode":"include","values":["a"]}});
    let result = call(
        "dynare_model_info",
        json!({"source":input,"analysis":analysis,"sections":["summary","analysis"]}),
    );
    assert_eq!(result["data"]["summary"]["equation_count"], 1, "{result}");
    assert_eq!(
        result["inputs"][0]["analysis"]["selection"]["invocation_scope"],
        "explicit_macro_and_equation_inputs"
    );
    assert_eq!(
        result["inputs"][0]["analysis"]["selection"]["preview"],
        true
    );
    let compared = call(
        "dynare_compare_models",
        json!({"comparison":{"kind":"sources","before":input,"after":input},"analysis_b":analysis}),
    );
    let inputs = compared["inputs"].as_array().unwrap();
    assert!(
        inputs.iter().any(
            |input| input["analysis"]["configuration"]["equation_selection"]["values"][0] == "a"
        )
    );
    assert!(
        inputs
            .iter()
            .any(|input| input["analysis"]["configuration"]["equation_selection"].is_null())
    );
    let rows = &compared["data"]["semantic"]["rows"];
    assert!(
        rows.as_array()
            .unwrap()
            .iter()
            .any(|row| row["family"] == "equations"),
        "{rows}"
    );
    let refused = call(
        "dynare_rename",
        json!({"source":input,"analysis":analysis,"symbol":{"kind":"name","name":"a"},"new_name":"output"}),
    );
    assert_eq!(
        refused["outcome"]["reasons"][0]["code"],
        "ANALYSIS_PREVIEW_EDIT_UNSUPPORTED"
    );
    assert!(refused["proposal"].is_null());
}

#[test]
fn failed_equation_selection_is_explained_without_source_counts() {
    // Guards a failed selector being presented as a successful Source result.
    let result = call(
        "dynare_model_info",
        json!({"source":source("var a; model; [name='a'] a=a(-1); end;"),"analysis":{"equation_selection":{"mode":"include","values":["absent"]}},"sections":["summary","analysis"]}),
    );
    assert_eq!(result["coverage"][0]["complete"], false);
    assert!(result["data"]["summary"]["equation_count"].is_null());
    assert!(
        result
            .to_string()
            .contains("Equation selection was not applied")
    );
    assert!(!result["outcome"]["reasons"].as_array().unwrap().is_empty());
    let expanded = call(
        "dynare_expand",
        json!({"source":source("var a; model; [name='a'] a=a(-1); end;"),"analysis":{"equation_selection":{"mode":"include","values":["absent"]}}}),
    );
    assert_eq!(expanded["data"]["n_equations"], 0);
    assert!(
        expanded["data"]["origins"]
            .as_array()
            .is_none_or(|origins| origins.is_empty())
    );
    assert!(expanded["data"].to_string().contains("a"));
}

#[test]
fn request_choices_change_effective_facts_without_leaking_to_the_next_request() {
    // A false definition is present; request-local state must not leak to Source.
    let configured = call(
        "dynare_model_info",
        json!({"source":source(MODEL),"analysis":alternative(),"sections":["summary","analysis"]}),
    );
    let default = call(
        "dynare_model_info",
        json!({"source":source(MODEL),"sections":["summary","analysis"]}),
    );
    assert_eq!(configured["data"]["summary"]["equation_count"], 2);
    assert_eq!(default["data"]["summary"]["equation_count"], 1);
    assert_eq!(configured["schema_version"], 3);
    assert_ne!(
        configured["inputs"][0]["revision"],
        default["inputs"][0]["revision"]
    );
    assert_eq!(
        configured["data"]["analysis"]["selection"]["preview"],
        false
    );
    assert_eq!(
        configured["inputs"][0]["analysis"]["selection"]["invocation_scope"],
        "explicit_macro_inputs"
    );
}

#[test]
fn compatible_jobs_share_source_records_and_matching_model_results() {
    // Job isolation must not discard valid immutable records or repeat matching analysis.
    use super::source::{resolve_source, resolve_source_configured_reusing};
    let input: super::source::SourceInput = serde_json::from_value(source(MODEL)).unwrap();
    let control = RequestControl::default();
    let first = resolve_source(input.clone(), None, &control).unwrap();
    let repeated = resolve_source_configured_reusing(
        input.clone(),
        None,
        &control,
        None,
        None,
        Some((&first, true)),
    )
    .unwrap();
    assert!(std::sync::Arc::ptr_eq(&first.model, &repeated.model));
    assert!(std::sync::Arc::ptr_eq(&first.report, &repeated.report));
    let choices = serde_json::from_value(alternative()).unwrap();
    let configured = resolve_source_configured_reusing(
        input,
        None,
        &control,
        Some(&choices),
        None,
        Some((&first, true)),
    )
    .unwrap();
    let before = first
        .workspace
        .source_document(&first.root, &first.root)
        .unwrap()
        .macro_source()
        .unwrap();
    let after = configured
        .workspace
        .source_document(&configured.root, &configured.root)
        .unwrap()
        .macro_source()
        .unwrap();
    assert!(std::sync::Arc::ptr_eq(&before, &after));
    assert_ne!(first.revision, configured.revision);
    assert!(!first.model.is_analysis_preview());
    assert!(
        first
            .workspace
            .root_analysis_configuration(&first.root)
            .is_source()
    );
}

#[test]
fn configured_text_fix_reparses_with_initial_inputs_and_keeps_other_source() {
    // The raw auto-fix fast path must not repair Source while labelling configured facts.
    let text = "@#ifdef FIX\nvar y;\nmodel;\ny=0\nend;\n@#else\nvar z; model; z=0; end;\n@#endif\n";
    let result=super::request::dispatch("dynare_auto_fix",json!({"source":{"kind":"text","text":text},"analysis":{"defines":[{"name":"FIX","expression":"false"}]}}),RequestControl::default()).unwrap().structured_content.unwrap();
    assert_ne!(result["outcome"]["status"], "refused");
    let fixed = result["data"]["text"].as_str().unwrap();
    assert!(fixed.contains("y=0;\n"), "{result}");
    assert!(fixed.contains("@#else\nvar z; model; z=0; end;\n@#endif\n"));
    assert!(
        result["data"]["remaining_diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .all(|row| row["code"] != "E001")
    );
    assert_eq!(
        result["proposal"]["analysis"]["configuration"]["defines"][0]["name"],
        "FIX"
    );
}

#[test]
fn rename_uses_the_shared_conditional_source_proof() {
    // Configurations keep source edit proof: relevant dormant uses refuse, unrelated ones do not.
    let refused = call(
        "dynare_rename",
        json!({"source":source(MODEL),"symbol":{"kind":"name","name":"y"},"new_name":"output"}),
    );
    assert_eq!(
        refused["outcome"]["reasons"][0]["code"],
        "CONDITIONAL_EDIT_UNPROVEN"
    );
    assert!(refused["proposal"].is_null());
    let safe = call(
        "dynare_rename",
        json!({"source":source(MODEL),"analysis":alternative(),"symbol":{"kind":"name","name":"z"},"new_name":"auxiliary"}),
    );
    assert_eq!(safe["outcome"]["status"], "changed");
    assert!(
        safe["proposal"]["edits"]
            .as_array()
            .is_some_and(|rows| !rows.is_empty())
    );
}

#[test]
fn source_equations_and_candidates_do_not_become_effective_ids_or_references() {
    // Dormant source remains inspectable without entering semantic inventories.
    let written = call(
        "dynare_equations",
        json!({"source":source(MODEL),"view":"source","name":"extra"}),
    );
    let row = &written["data"]["equation_templates"][0];
    assert_eq!(row["availability"], "inactive");
    assert!(row["source_id"].is_string());
    assert!(row.get("equation_id").is_none());
    assert!(row.get("index").is_none());
    assert!(written["data"].get("count_gap").is_none());
    let candidate = call(
        "dynare_symbol_info",
        json!({"source":source(MODEL),"view":"source","symbol":{"kind":"name","name":"z"}}),
    );
    assert_eq!(
        candidate["data"]["candidates"][0]["label"],
        "source candidate"
    );
    assert!(candidate["data"].get("references").is_none());
    let effective = call(
        "dynare_symbol_info",
        json!({"source":source(MODEL),"symbol":{"kind":"name","name":"z"}}),
    );
    assert_eq!(
        effective["outcome"]["reasons"][0]["code"],
        "BINDING_UNAVAILABLE"
    );
    assert!(canonical::dispatch("dynare_expand",json!({"source":source(MODEL),"equation":{"kind":"id","equation_id":row["source_id"]},"expected_revision":effective["inputs"][0]["revision"]}),RequestControl::default()).is_err());
}

#[test]
fn discovery_targets_guard_previews_and_refuse_edit_proposals() {
    // Discovery supplies exact target guards; previews cannot issue fixes or Rename.
    let text = format!(
        "@#define ALT=0\n{}",
        MODEL.replace("@#ifdef ALT", "@#if ALT")
    );
    let discovered = call(
        "dynare_model_info",
        json!({"source":source(&text),"sections":["source_structure"]}),
    );
    let site = discovered["data"]["source_structure"]["directives"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["kind"] == "definition")
        .unwrap();
    let analysis =
        json!({"overrides":[{"kind":"definition","target":site["target"],"expression":"1"}]});
    let expanded = call(
        "dynare_expand",
        json!({"source":source(&text),"analysis":analysis}),
    );
    assert_eq!(
        expanded["inputs"][0]["analysis"]["selection"]["preview"],
        true
    );
    assert_eq!(
        expanded["inputs"][0]["analysis"]["overrides"][0]["application"],
        "applied"
    );
    for (tool, extra) in [
        (
            "dynare_rename",
            json!({"symbol":{"kind":"name","name":"y"},"new_name":"renamed"}),
        ),
        ("dynare_actions", json!({"fixes":true})),
    ] {
        let mut args = json!({"source":source(&text),"analysis":analysis});
        args.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        let refused = call(tool, args);
        assert_eq!(
            refused["outcome"]["reasons"][0]["code"],
            "ANALYSIS_PREVIEW_EDIT_UNSUPPORTED"
        );
        assert!(refused["proposal"].is_null());
    }
    let stale = call(
        "dynare_expand",
        json!({"source":source(&(text+"\n")),"analysis":analysis}),
    );
    assert_eq!(
        stale["outcome"]["reasons"][0]["code"],
        "ANALYSIS_TARGET_STALE"
    );
}

#[test]
fn configured_jobs_keep_repeated_roots_and_page_by_job_identity() {
    // Repeated roots must remain separate jobs with separate revisions and pages.
    let first = call(
        "dynare_diagnose",
        json!({"batch":{"kind":"configured_sources","jobs":[{"source":source(MODEL),"analysis":{}},{"source":source(MODEL),"analysis":alternative()}]},"page_size":1}),
    );
    assert_eq!(first["data"]["summary"]["jobs"], 2);
    assert_eq!(first["data"]["summary"]["distinct_roots"], 1);
    assert_eq!(first["data"]["roots"][0]["job_id"], "job:1");
    assert_eq!(first["data"]["roots"][1]["job_id"], "job:2");
    assert_eq!(first["data"]["roots"][1]["root"], "main.mod");
    assert_ne!(
        first["data"]["root_revisions"]["job:1"],
        first["data"]["root_revisions"]["job:2"]
    );
    assert!(
        first["selection"]["collections"]
            .get("diagnostics:job:1")
            .is_some()
    );
    let continued = call(
        "dynare_diagnose",
        json!({"batch":{"kind":"configured_sources","jobs":[{"source":source(MODEL),"analysis":{}},{"source":source(MODEL),"analysis":alternative()}]},"page_size":1,"continuation":first["selection"]["continuation"],"expected_revisions":first["data"]["root_revisions"]}),
    );
    assert_ne!(continued["outcome"]["status"], "refused");
    assert!(
        canonical::dispatch(
            "dynare_diagnose",
            json!({"batch":{"kind":"sources","sources":[source(MODEL)]},"analysis":alternative()}),
            RequestControl::default()
        )
        .is_err()
    );
}

#[test]
fn refused_comparison_retains_captured_and_requested_choices_without_fake_counts() {
    // A late acquisition refusal must retain Before's actual choice and After's requested choice.
    let result = call(
        "dynare_compare_models",
        json!({"comparison":{"kind":"sources","before":source(MODEL),"after":source(MODEL)},"analysis_a":alternative(),"analysis_b":{"name":"After Source"},"expected_revision_b":"stale"}),
    );
    let summaries = &result["data"]["analysis_summaries"];
    assert_eq!(result["outcome"]["reasons"][0]["code"], "INPUT_CHANGED");
    assert_eq!(
        summaries["before"]["analysis"]["configuration"]["defines"][0]["name"],
        "ALT"
    );
    assert_eq!(summaries["before"]["complete"], true);
    assert!(summaries["before"]["counts"].is_null());
    assert_eq!(
        summaries["after"]["requested_analysis"]["name"],
        "After Source"
    );
    assert!(summaries["after"].get("analysis").is_none());
    assert!(summaries["after"]["complete"].is_null());
    assert!(summaries["after"]["counts"].is_null());
    assert_eq!(summaries["after"]["cause"]["code"], "INPUT_CHANGED");
    assert!(result["data"].get("navigation").is_none());
    assert!(result["data"].get("semantic").is_none());
}

#[test]
fn incomplete_comparison_retains_the_actual_side_cause_and_withholds_model_counts() {
    // An executed missing include must not erase the complete side or publish a zero count.
    let result = call(
        "dynare_compare_models",
        json!({"comparison":{"kind":"sources","before":source("@#include \"missing.inc\"\nvar y; model; y=0; end;"),"after":source(MODEL)},"analysis_b":alternative()}),
    );
    let summaries = &result["data"]["analysis_summaries"];
    assert_eq!(summaries["before"]["complete"], false);
    assert!(summaries["before"]["analysis"].is_object());
    assert!(
        summaries["before"]["cause"]["message"]
            .as_str()
            .unwrap()
            .contains("missing.inc")
    );
    assert_eq!(summaries["after"]["complete"], true);
    for side in ["before", "after"] {
        assert!(summaries[side]["counts"].is_null());
        assert!(summaries[side]["counts_cause"].is_string());
    }
    assert!(result["data"].get("navigation").is_none());
    assert!(result["data"].get("semantic").is_none());
}

#[test]
fn saved_configuration_pair_spends_one_source_union_but_distinct_supplied_inputs_add() {
    // Guards counting identical saved bodies/candidates twice after the shared
    // union already fitted, while keeping separate supplied inputs additive.
    struct Temporary(std::path::PathBuf);
    impl Drop for Temporary {
        fn drop(&mut self) {
            if let (Ok(parent), Ok(target)) =
                (std::env::temp_dir().canonicalize(), self.0.canonicalize())
                && target.parent() == Some(parent.as_path())
                && target
                    .file_name()
                    .is_some_and(|name| name.to_string_lossy().starts_with("dygnosis-mcp-union-"))
            {
                let _ = std::fs::remove_dir_all(target);
            }
        }
    }
    let directory = Temporary(std::env::temp_dir().join(format!(
            "dygnosis-mcp-union-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        )));
    std::fs::create_dir(&directory.0).unwrap();
    let root = directory.0.join("main.mod");
    std::fs::write(&root, MODEL).unwrap();
    let disk = json!({"kind":"disk","root":root});
    let first = super::source::resolve_source(
        serde_json::from_value(disk.clone()).unwrap(),
        None,
        &RequestControl::default(),
    )
    .unwrap();
    assert!(!first.guards.is_empty());
    let supplied = source(MODEL);
    for byte_ceiling in [true, false] {
        let mut control = RequestControl::default();
        if byte_ceiling {
            control.limits.max_source_bytes = MODEL.len();
        } else {
            control.limits.max_files = first.guards.len();
        }
        let run = |before: Value, after: Value| {
            canonical::dispatch("dynare_compare_models", json!({"comparison":{"kind":"sources","before":before,"after":after},"analysis_a":{},"analysis_b":alternative()}), control.clone()).unwrap().structured_content.unwrap()
        };
        let saved = run(disk.clone(), disk.clone());
        assert_ne!(saved["outcome"]["status"], "refused", "{saved}");
        assert_ne!(
            saved["inputs"][0]["revision"],
            saved["inputs"][1]["revision"]
        );
        let distinct = run(supplied.clone(), supplied.clone());
        assert_eq!(distinct["outcome"]["status"], "refused", "{distinct}");
        assert_eq!(distinct["outcome"]["reasons"][0]["code"], "SOURCE_LIMIT");
        assert!(distinct["data"].get("navigation").is_none());
    }
    let owner_a = directory.0.join("owner_a.mod");
    let owner_b = directory.0.join("owner_b.mod");
    std::fs::write(&owner_a, MODEL).unwrap();
    std::fs::write(&owner_b, MODEL).unwrap();
    let mut before = disk.clone();
    before["owner_roots"] = json!([owner_a]);
    let mut after = disk;
    after["owner_roots"] = json!([owner_b]);
    let owned = super::source::resolve_source(
        serde_json::from_value(before.clone()).unwrap(),
        None,
        &RequestControl::default(),
    )
    .unwrap();
    for byte_ceiling in [true, false] {
        let mut control = RequestControl::default();
        if byte_ceiling {
            control.limits.max_source_bytes = MODEL.len() * 2;
        } else {
            control.limits.max_files = owned.guards.len();
        }
        let result = canonical::dispatch("dynare_compare_models", json!({"comparison":{"kind":"sources","before":before,"after":after},"analysis_a":{},"analysis_b":alternative()}), control).unwrap().structured_content.unwrap();
        assert_eq!(result["outcome"]["status"], "refused", "{result}");
        assert_eq!(result["outcome"]["reasons"][0]["code"], "SOURCE_LIMIT");
        assert!(result["data"].get("navigation").is_none());
    }
}

#[test]
fn repository_revision_refusal_keeps_readonly_analysis_summaries() {
    // The canonical expected-revision guard must not drop already captured side choices.
    struct Temporary(std::path::PathBuf);
    impl Drop for Temporary {
        fn drop(&mut self) {
            if let (Ok(parent), Ok(target)) =
                (std::env::temp_dir().canonicalize(), self.0.canonicalize())
                && target.parent() == Some(parent.as_path())
                && target.file_name().is_some_and(|name| {
                    name.to_string_lossy().starts_with("dygnosis-mcp-revision-")
                })
            {
                let _ = std::fs::remove_dir_all(target);
            }
        }
    }
    let directory = Temporary(std::env::temp_dir().join(format!(
            "dygnosis-mcp-revision-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        )));
    std::fs::create_dir(&directory.0).unwrap();
    std::fs::write(directory.0.join("main.mod"), MODEL).unwrap();
    let initialized = std::process::Command::new("git")
        .arg("-C")
        .arg(&directory.0)
        .args(["init", "--quiet"])
        .output()
        .unwrap();
    assert!(
        initialized.status.success(),
        "{}",
        String::from_utf8_lossy(&initialized.stderr)
    );
    let mut arguments = json!({"comparison":{"kind":"repository","repository_path":directory.0,"before":{"kind":"working","root_file":"main.mod"},"after":{"kind":"working","root_file":"main.mod"}},"analysis_a":{"name":"Before Source"},"analysis_b":alternative()});
    let first = call("dynare_compare_models", arguments.clone());
    assert_ne!(first["outcome"]["status"], "refused");
    arguments["expected_revision_a"] = first["inputs"][0]["revision"].clone();
    arguments["expected_revision_b"] = json!("stale");
    let refused = call("dynare_compare_models", arguments);
    assert_eq!(refused["outcome"]["reasons"][0]["code"], "INPUT_CHANGED");
    let summaries = &refused["data"]["analysis_summaries"];
    assert_eq!(
        summaries["before"]["analysis"]["configuration"]["name"],
        "Before Source"
    );
    assert_eq!(
        summaries["after"]["analysis"]["configuration"]["defines"][0]["name"],
        "ALT"
    );
    for side in ["before", "after"] {
        assert!(summaries[side]["complete"].is_null());
        assert!(summaries[side]["counts"].is_null());
        assert_eq!(summaries[side]["cause"]["code"], "INPUT_CHANGED");
    }
    assert!(refused["data"].get("navigation").is_none());
    assert!(refused["data"].get("semantic").is_none());
    assert!(refused["data"].get("sources").is_none());
    assert!(refused["proposal"].is_null());
}

#[test]
fn repeated_root_jobs_spend_the_finite_job_allowance() {
    // Repeated roots must not bypass the finite root-job ceiling through deduplication.
    let mut control = RequestControl::default();
    control.limits.max_roots = 1;
    let result=canonical::dispatch("dynare_diagnose",json!({"batch":{"kind":"configured_sources","jobs":[{"source":source(MODEL),"analysis":{}},{"source":source(MODEL),"analysis":alternative()}]}}),control).unwrap().structured_content.unwrap();
    assert_eq!(result["data"]["summary"]["unfinished"], 1);
    assert_eq!(result["data"]["roots"][1]["job_id"], "job:2");
    assert_eq!(result["data"]["roots"][1]["status"], "unfinished");
    assert!(result["data"]["root_revisions"].get("job:2").is_none());
}

#[test]
fn source_pages_reject_changed_filters_and_keep_source_totals() {
    // Paging must neither truncate source counts nor accept another source selection.
    let first = call(
        "dynare_equations",
        json!({"source":source(MODEL),"view":"source","page_size":1}),
    );
    assert_eq!(
        first["data"]["equation_templates"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        first["selection"]["collections"]["equation_templates"]["available_matches"],
        3
    );
    assert_eq!(first["data"]["source_item_count"], 3);
    let next = call(
        "dynare_equations",
        json!({"source":source(MODEL),"view":"source","page_size":1,"expected_revision":first["inputs"][0]["revision"],"continuation":first["selection"]["continuation"]}),
    );
    assert_eq!(
        next["selection"]["collections"]["equation_templates"]["offset"],
        1
    );
    let changed = call(
        "dynare_equations",
        json!({"source":source(MODEL),"view":"source","name":"extra","page_size":1,"expected_revision":first["inputs"][0]["revision"],"continuation":first["selection"]["continuation"]}),
    );
    assert_eq!(changed["outcome"]["reasons"][0]["code"], "INPUT_CHANGED");
}

#[test]
fn explicit_source_inspection_has_coverage_without_execution_ownership() {
    // Browsing a supplied dormant helper cannot add model facts or hide an unavailable file.
    let input = json!({"kind":"map","root":"main.mod","files":{"main.mod":"var y; model; y=0; end;","helper.inc":"var dormant;\n"}});
    let inspected = call(
        "dynare_model_info",
        json!({"source":input,"sections":["summary","source_structure"],"file":"helper.inc"}),
    );
    assert_eq!(inspected["data"]["summary"]["equation_count"], 1);
    assert_eq!(
        inspected["data"]["source_structure"]["coverage"]["indexed_files"][0]["executed"],
        false
    );
    assert_eq!(
        inspected["data"]["source_structure"]["coverage"]["indexed_files"][0]["explicitly_selected"],
        true
    );
    let missing = call(
        "dynare_equations",
        json!({"source":input,"view":"source","file":"not-supplied.inc"}),
    );
    assert_eq!(missing["data"]["source_coverage"]["complete"], false);
    assert_eq!(missing["coverage"][0]["complete"], true);
    assert!(missing["data"]["source_item_count"].is_null());
    assert!(missing["selection"]["collections"]["equation_templates"]["total_matches"].is_null());
    assert_eq!(missing["outcome"]["status"], "partial");
}

#[test]
fn schema_and_decode_reject_misplaced_or_unknown_analysis_inputs() {
    // Additive choices must not weaken existing strict tool and nested schemas.
    for tool in ["dynare_syntax", "dynare_diagnostic_info"] {
        assert!(
            canonical::input_schema(tool)["properties"]
                .get("analysis")
                .is_none()
        );
        assert!(
            canonical::dispatch(tool, json!({"analysis":{}}), RequestControl::default()).is_err()
        );
    }
    for analysis in [
        json!({"include_dirs":[]}),
        json!({"defines":[{"name":"ALT","expression":"1","unknown":true}]}),
    ] {
        assert!(
            canonical::dispatch(
                "dynare_model_info",
                json!({"source":source(MODEL),"analysis":analysis}),
                RequestControl::default()
            )
            .is_err()
        );
    }
}

#[test]
fn disk_helper_changes_cannot_relabel_captured_models_with_refreshed_guards() {
    // Source inspection and metadata can refresh a Workspace. Neither may pair
    // that new identity with the model/report Arcs captured before a disk edit.
    use super::source::{
        CaptureBoundary, SourceInput, resolve_source_configured, with_capture_hook,
    };
    use crate::engine::analysis_configuration::SourceStructureQuery;
    use crate::engine::language::parser::PARSE_EXPANDED_BUILDS;
    struct Temporary(std::path::PathBuf);
    impl Drop for Temporary {
        fn drop(&mut self) {
            if let (Ok(parent), Ok(target)) =
                (std::env::temp_dir().canonicalize(), self.0.canonicalize())
                && target.parent() == Some(parent.as_path())
                && target
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with("dygnosis-mcp-capture-"))
            {
                let _ = std::fs::remove_dir_all(target);
            }
        }
    }
    let directory = Temporary(std::env::temp_dir().join(format!(
            "dygnosis-mcp-capture-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        )));
    std::fs::create_dir_all(&directory.0).unwrap();
    let root = directory.0.join("main.mod");
    let helper = directory.0.join("active.inc");
    std::fs::write(&root, "var y;\nmodel;\n@#include \"active.inc\"\nend;\n").unwrap();
    for boundary in [
        CaptureBoundary::SourceStructure,
        CaptureBoundary::AnalysisMetadata,
    ] {
        std::fs::write(&helper, "y=0;\n").unwrap();
        let changed = helper.clone();
        let query = SourceStructureQuery::default();
        let result = with_capture_hook(
            boundary,
            move || {
                PARSE_EXPANDED_BUILDS.with(|count| count.set(0));
                std::fs::write(changed, "y=1;\n").unwrap();
            },
            || {
                resolve_source_configured(
                    SourceInput::Disk {
                        root: root.to_string_lossy().into_owned(),
                        include_dirs: Vec::new(),
                        owner_roots: Vec::new(),
                    },
                    None,
                    &RequestControl::default(),
                    None,
                    (boundary == CaptureBoundary::SourceStructure).then_some(&query),
                )
            },
        );
        assert_eq!(
            result
                .err()
                .expect("changed capture must return no snapshot")
                .code,
            "INPUT_CHANGED"
        );
        assert_eq!(
            PARSE_EXPANDED_BUILDS.with(std::cell::Cell::get),
            0,
            "refusal must not reparse the changed source"
        );
    }
}
