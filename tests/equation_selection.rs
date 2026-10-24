//! Official Transform probes for explicit invocation equation selection.

use std::time::Duration;

use dygnosis::analysis_configuration::AnalysisConfiguration;
use dygnosis::preprocessor::{JsonStage, run_preprocessor_with_options};
use dygnosis::server::new_service;
use dygnosis::workspace::model_map::parser_complete;
use dygnosis::{Model, Workspace, analyze, find_preprocessor};
use serde_json::json;
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

fn selection(mode: &str, values: &[&str]) -> AnalysisConfiguration {
    serde_json::from_value(json!({
        "equation_selection": {
            "mode": mode,
            "values": values,
        }
    }))
    .unwrap()
}

fn apply(source: &str, mode: &str, values: &[&str]) -> (Workspace, Vec<String>) {
    let mut workspace = Workspace::new();
    workspace.update_document("main.mod", source);
    workspace
        .set_root_analysis_configuration("main.mod", selection(mode, values))
        .unwrap();
    let codes = analyze(workspace.get_effective_model("main.mod").unwrap())
        .into_iter()
        .map(|diagnostic| diagnostic.code)
        .collect();
    (workspace, codes)
}

fn has_decl(model: &Model, declarations: &[dygnosis::model::Decl], name: &str) -> bool {
    declarations
        .iter()
        .any(|declaration| model.name(declaration.name) == name)
}

fn assert_no_errors_or_warnings(codes: &[String]) {
    assert!(
        codes
            .iter()
            .all(|code| !matches!(code.as_bytes().first(), Some(b'E' | b'W'))),
        "unexpected Error or Warning diagnostics: {codes:?}"
    );
}

#[test]
fn include_retypes_excluded_endogenous_used_through_a_model_local() {
    let source = "var y z q; model; #helper = z + q; [name='selected'] y=helper; [name='z-law'] z=0; [name='q-law'] q=0; end;";
    let (mut workspace, codes) = apply(source, "include", &["selected"]);
    let model = workspace.get_effective_model("main.mod").unwrap();

    assert_no_errors_or_warnings(&codes);
    assert_eq!(
        model
            .equations
            .iter()
            .filter(|row| !row.is_local)
            .map(|row| row.name.as_str())
            .collect::<Vec<_>>(),
        ["selected"]
    );
    assert!(has_decl(model, &model.endogenous, "y"));
    assert!(has_decl(model, &model.exogenous, "z"));
    assert!(has_decl(model, &model.exogenous, "q"));
    assert_eq!(workspace.get_source("main.mod"), Some(source));
    assert!(
        workspace
            .analysis_metadata("main.mod")
            .unwrap()
            .model_complete
    );
}

#[test]
fn exclude_removes_the_requested_equation_and_its_unneeded_endogenous() {
    let source = "var y z; model; [name='left'] y=z; [name='right'] z=0; end;";
    let (mut workspace, codes) = apply(source, "exclude", &["left"]);
    let model = workspace.get_effective_model("main.mod").unwrap();

    assert_no_errors_or_warnings(&codes);
    assert_eq!(
        model
            .equations
            .iter()
            .map(|row| row.name.as_str())
            .collect::<Vec<_>>(),
        ["right"]
    );
    assert!(!has_decl(model, &model.endogenous, "y"));
    assert!(has_decl(model, &model.excluded_endogenous, "y"));
    assert!(has_decl(model, &model.endogenous, "z"));
}

#[test]
fn preselection_check_errors_survive_and_withhold_selected_completeness() {
    let source = "var y; model; [name='bad'] y=missing; [name='good'] y=0; end;";
    let (mut workspace, codes) = apply(source, "include", &["good"]);
    let model = workspace.get_effective_model("main.mod").unwrap();

    assert!(
        codes.iter().any(|code| code == "E020"),
        "pre-selection E020 was lost: {codes:?}"
    );
    assert!(!parser_complete(model));
    assert!(
        !workspace
            .analysis_metadata("main.mod")
            .unwrap()
            .model_complete
    );
}

#[test]
fn unknown_selector_reports_e335_and_withholds_selected_completeness() {
    let source = "var y; model; [name='present'] y=0; end;";
    let (mut workspace, codes) = apply(source, "include", &["absent"]);
    let model = workspace.get_effective_model("main.mod").unwrap();

    assert!(
        codes.iter().any(|code| code == "E335"),
        "missing E335: {codes:?}"
    );
    assert!(!parser_complete(model));
    assert!(
        !workspace
            .analysis_metadata("main.mod")
            .unwrap()
            .model_complete
    );
}

#[test]
fn ambiguous_excluded_lhs_reports_e336() {
    let source = "var y z; model; [name='sum'] y+z=0; [name='keep'] y=0; end;";
    let (mut workspace, codes) = apply(source, "include", &["keep"]);
    let model = workspace.get_effective_model("main.mod").unwrap();

    assert!(
        codes.iter().any(|code| code == "E336"),
        "missing E336: {codes:?}"
    );
    assert!(!parser_complete(model));
    assert!(
        !workspace
            .analysis_metadata("main.mod")
            .unwrap()
            .model_complete
    );
}

#[test]
fn removed_static_equation_without_single_lhs_reports_e336() {
    let source = "var y z; model; [dynamic,name='dynamic'] y=0; [static,name='static'] y+z=0; end;";
    let (mut workspace, codes) = apply(source, "include", &["dynamic"]);
    let model = workspace.get_effective_model("main.mod").unwrap();

    assert!(
        codes.iter().any(|code| code == "E336"),
        "missing E336 for removed static equation: {codes:?}"
    );
    assert!(!parser_complete(model));
    assert!(
        !workspace
            .analysis_metadata("main.mod")
            .unwrap()
            .model_complete
    );
}

#[test]
fn constructed_zero_coefficient_does_not_keep_an_excluded_endogenous_alive() {
    let source = "var y z; model; [name='selected'] y=0*z; [name='z-law'] z=0; end;";
    let (mut workspace, codes) = apply(source, "include", &["selected"]);
    let model = workspace.get_effective_model("main.mod").unwrap();

    assert_no_errors_or_warnings(&codes);
    assert!(!has_decl(model, &model.exogenous, "z"));
    assert!(has_decl(model, &model.excluded_endogenous, "z"));
    assert!(!has_decl(model, &model.endogenous, "z"));
}

#[test]
fn selection_constructor_usage_and_static_ownership_match_pinned_transform() {
    // Guards agreement with the official constructed nodes, rather than a
    // written-token interpretation of zero products or static ownership.
    let Some(preprocessor) = find_preprocessor(None) else {
        return;
    };
    for (source, option, success, message) in [
        (
            "var y z; model; [name='keep'] y=0*z; [name='drop'] z=1; end;",
            "include_eqs=[keep]",
            true,
            "Variable 'z' has been excluded from the model",
        ),
        (
            "var y z; model; [dynamic,name='dynamic_row'] y=y(-1); [static,name='static_row'] y+z=0; [name='keep'] z=z(-1); end;",
            "include_eqs=[keep]",
            false,
            "does not have a single variable on its left-hand side",
        ),
    ] {
        let result = run_preprocessor_with_options(
            source,
            &preprocessor,
            None,
            Duration::from_secs(15),
            JsonStage::Transform,
            &[option.into()],
        );
        assert_eq!(
            result.success, success,
            "{}{}",
            result.raw_stdout, result.raw_stderr
        );
        assert!(format!("{}{}", result.raw_stdout, result.raw_stderr).contains(message));
    }
}

#[test]
fn repeated_endogenous_ownership_reports_e337() {
    let source = "var y z; model; [name='first'] y=0; [name='second'] y=1; [name='keep'] z=0; end;";
    let (mut workspace, codes) = apply(source, "include", &["keep"]);
    let model = workspace.get_effective_model("main.mod").unwrap();

    assert!(
        codes.iter().any(|code| code == "E337"),
        "missing E337: {codes:?}"
    );
    assert!(!parser_complete(model));
    assert!(
        !workspace
            .analysis_metadata("main.mod")
            .unwrap()
            .model_complete
    );
}

#[test]
fn dynamic_first_tag_consumption_can_leave_static_pair_unmatched() {
    let source = "var y; model; [dynamic,name='pair'] y=y(-1); [static,name='pair'] y=0; end;";
    let (_, codes) = apply(source, "include", &["pair"]);
    assert!(
        codes.iter().any(|code| code == "E483"),
        "expected static/dynamic selection imbalance: {codes:?}"
    );
}

#[tokio::test]
async fn lsp_model_info_withholds_model_facts_after_selection_failure() {
    let (service, _socket) = new_service();
    let server = service.inner();
    let folder = Url::parse("file:///C:/dygnosis-equation-selection").unwrap();
    let root = folder.join("model.mod").unwrap();
    let document = root.clone();
    let params: InitializeParams = serde_json::from_value(json!({
        "workspaceFolders": [{"uri": folder, "name": "equation selection"}],
        "capabilities": {"experimental": {"dygnosis": {
            "analysisConfiguration": {"schema_version": 1}
        }}},
        "initializationOptions": {"dynare": {"workspaceDiagnostics": false}}
    }))
    .unwrap();
    server.initialize(params).await.unwrap();
    server.initialized(InitializedParams {}).await;
    server
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: document.clone(),
                language_id: "dynare".into(),
                version: 1,
                text: "var y; model; [name='present'] y=0; end;".into(),
            },
        })
        .await;
    let context = server
        .execute_command(ExecuteCommandParams {
            command: "dynare/analysisContext".into(),
            arguments: vec![json!({"root_uri":root})],
            work_done_progress_params: Default::default(),
        })
        .await
        .unwrap()
        .unwrap();
    let selected = server
        .execute_command(ExecuteCommandParams {
            command: "dynare/setAnalysisConfiguration".into(),
            arguments: vec![json!({
                "root_uri": root,
                "expected_revision": context["revision"],
                "expected_selection_generation": context["analysis"]["selection_generation"],
                "analysis": {"equation_selection": {"mode":"include", "values":["absent"]}},
                "include_paths": null
            })],
            work_done_progress_params: Default::default(),
        })
        .await
        .unwrap()
        .unwrap();
    assert_eq!(selected["accepted"], true, "{selected}");
    assert_eq!(selected["analysis"]["state"], "selected", "{selected}");
    assert_eq!(
        selected["analysis"]["selection_generation"], 1,
        "{selected}"
    );
    assert_eq!(selected["analysis"]["model_complete"], false, "{selected}");
    assert_eq!(selected["partial"], true, "{selected}");
    assert!(
        selected["message"].as_str().is_some_and(|message| message
            .contains("Equation selection was not applied")
            && message.contains("absent")),
        "action outcome missing selector cause: {selected}"
    );
    assert!(
        selected["cause"]["code"]
            .as_str()
            .is_some_and(|code| code == "ANALYSIS_EQUATION_SELECTION_UNAVAILABLE"),
        "action cause code missing: {selected}"
    );
    let info = server
        .execute_command(ExecuteCommandParams {
            command: "dynare/modelInfo".into(),
            arguments: vec![json!({"root_uri":root, "document_uri":document})],
            work_done_progress_params: Default::default(),
        })
        .await
        .unwrap()
        .unwrap();

    assert_eq!(info["complete"], false, "{info}");
    assert_eq!(info["status"], "incomplete", "{info}");
    assert!(
        info.get("n_equations").is_none(),
        "authoritative count leaked: {info}"
    );
    assert!(
        info.get("endogenous").is_none(),
        "authoritative declarations leaked: {info}"
    );
    assert!(
        info["incomplete_reasons"]
            .as_array()
            .is_some_and(|reasons| reasons.iter().any(|reason| {
                reason["code"] == "ANALYSIS_EQUATION_SELECTION_UNAVAILABLE"
                    && reason["message"]
                        .as_str()
                        .is_some_and(|message| message.contains("absent"))
            })),
        "selection failure cause missing: {info}"
    );
}

#[test]
fn selector_induced_static_dynamic_imbalance_matches_pinned_preprocessor() {
    let Some(preprocessor) = find_preprocessor(None) else {
        eprintln!("skip equation selection honesty: pinned Dynare preprocessor is unavailable");
        return;
    };
    let source =
        "var y; model; [static,name='static_row'] y=0; [dynamic,name='dynamic_row'] y=y(-1); end;";
    let result = run_preprocessor_with_options(
        source,
        &preprocessor,
        None,
        Duration::from_secs(15),
        JsonStage::Transform,
        &["include_eqs=[name=(static_row)]".into()],
    );
    let output = format!("{}{}", result.raw_stderr, result.raw_stdout);
    assert!(
        !result.success,
        "pinned preprocessor accepted an imbalanced selection: {output}"
    );
    assert!(
        output.contains("You must remove the same number of equations marked"),
        "pinned preprocessor did not report selector-induced static/dynamic imbalance: {output}"
    );
}
