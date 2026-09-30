use std::time::Duration;

use dygnosis::model_info::{classify_variable_timing, structure_summary, TimingClass};
use dygnosis::server::new_service;
use dygnosis::{
    dynare_model_info, equations, find_preprocessor, parse, run_preprocessor, IdentClass, JsonStage,
};
use serde_json::json;
use tower_lsp::lsp_types::*;
use tower_lsp::LanguageServer;

const SOURCE: &str = "var y z;\nparameters q;\nvarexo e;\nvarexo_det d;\nchange_type(parameters) z;\nchange_type(var) q;\nchange_type(varexo_det) e;\nchange_type(varexo) d;\nmodel;\ny=q(-1)+z+e+d;\nq=y(-1);\nend;\n";

#[test]
fn final_types_agree_across_equations_timing_and_model_info() {
    let model = parse(SOURCE);
    let rows = equations(&model);
    for (name, class) in [
        ("y", IdentClass::Endogenous),
        ("q", IdentClass::Endogenous),
        ("z", IdentClass::Parameter),
        ("e", IdentClass::VarexoDet),
        ("d", IdentClass::Varexo),
    ] {
        let uses: Vec<_> = rows
            .iter()
            .flat_map(|row| &row.idents)
            .filter(|ident| ident.name == name)
            .collect();
        assert!(!uses.is_empty());
        for ident in uses {
            assert_eq!(ident.class, class, "{name}");
            assert_eq!(
                ident.timing_class.is_some(),
                class == IdentClass::Endogenous,
                "{name}"
            );
        }
    }
    let timing = classify_variable_timing(&model);
    assert_eq!(timing.len(), 2);
    assert_eq!(timing["q"].class, TimingClass::Predetermined);
    assert_eq!(timing["y"].class, TimingClass::Predetermined);
    let summary = structure_summary(&model);
    assert_eq!(
        (
            summary.endogenous,
            summary.predetermined,
            summary.static_vars,
            summary.varexo
        ),
        (2, 2, 0, 2)
    );
    let info = dynare_model_info(SOURCE, None, None);
    assert_eq!(info["endogenous"], json!(["y", "q"]));
    assert_eq!(info["parameters"], json!(["z"]));
    assert_eq!(info["exogenous"], json!(["e", "d"]));
    assert_eq!(info["n_endogenous"], 2);
    assert_eq!(info["n_parameters"], 1);
    assert_eq!(info["n_exogenous"], 2);
    assert_eq!(info["n_predetermined"], 2);
    assert_eq!(info["n_static"], 0);
    assert_eq!(model.name(model.endogenous[1].name), "z");
    assert_eq!(model.name(model.parameters[0].name), "q");
    if let Some(pp) = find_preprocessor(None) {
        let official =
            run_preprocessor(SOURCE, &pp, None, Duration::from_secs(30), JsonStage::Check);
        assert!(official.success, "{official:?}");
    }
}

#[test]
fn repeated_changes_and_restoration_show_the_last_type() {
    for source in [
        "var y z; change_type(parameters) z; change_type(varexo) z; change_type(parameters) z; model; y=z; end;",
        "var y z; var_remove z; change_type(parameters) z; model; y=z; end;",
    ] {
        let model = parse(source);
        assert_eq!(structure_summary(&model).endogenous, 1);
        assert_eq!(equations(&model)[0].idents[1].class, IdentClass::Parameter);
        assert_eq!(dynare_model_info(source, None, None)["parameters"], json!(["z"]));
        if let Some(pp) = find_preprocessor(None) {
            let official = run_preprocessor(source, &pp, None, Duration::from_secs(30), JsonStage::Check);
            assert!(official.success, "{official:?}");
        }
    }
}

#[tokio::test]
async fn hover_uses_final_types_and_definition_keeps_written_location() {
    let uri = Url::parse("file:///final_type_views.mod").unwrap();
    let (service, _socket) = new_service();
    service
        .inner()
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "dynare".into(),
                version: 1,
                text: SOURCE.into(),
            },
        })
        .await;
    for (name, expected, timing) in [
        ("q", "**Endogenous variable**", true),
        ("z", "**Parameter**", false),
        ("e", "**Exogenous deterministic variable**", false),
        ("d", "**Exogenous variable**", false),
    ] {
        let character = SOURCE.lines().nth(9).unwrap().find(name).unwrap() as u32;
        let position = TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position::new(9, character),
        };
        let hover = service
            .inner()
            .hover(HoverParams {
                text_document_position_params: position.clone(),
                work_done_progress_params: Default::default(),
            })
            .await
            .unwrap()
            .unwrap();
        let HoverContents::Markup(markdown) = hover.contents else {
            panic!("expected Markdown hover")
        };
        assert!(markdown.value.contains(expected), "{}", markdown.value);
        assert_eq!(
            markdown.value.contains("Timing:"),
            timing,
            "{}",
            markdown.value
        );
        let definition = service
            .inner()
            .goto_definition(GotoDefinitionParams {
                text_document_position_params: position,
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            })
            .await
            .unwrap()
            .unwrap();
        let GotoDefinitionResponse::Scalar(location) = definition else {
            panic!("expected declaration location")
        };
        let expected_line = match name {
            "q" => 1,
            "z" => 0,
            "e" => 2,
            "d" => 3,
            _ => unreachable!(),
        };
        assert_eq!(location.range.start.line, expected_line, "{name}");
    }
}

#[test]
fn retyping_a_heterogeneous_name_makes_its_final_dimension_ordinary() {
    for (decl, change, body, name, kind) in [
        (
            "var(heterogeneity=h) x;",
            "change_type(var) x;",
            "y=x; x=x(-1);",
            "x",
            "endogenous",
        ),
        (
            "parameters(heterogeneity=h) q;",
            "change_type(parameters) q;",
            "y=q;",
            "q",
            "parameters",
        ),
        (
            "var(heterogeneity=h) x;",
            "change_type(var) x; var x;",
            "y=x; x=x(-1);",
            "x",
            "endogenous",
        ),
    ] {
        let source =
            format!("heterogeneity_dimension h; var y; {decl} {change} model; {body} end;");
        let info = dynare_model_info(&source, None, None);
        assert!(
            info[kind].as_array().unwrap().contains(&json!(name)),
            "{info}"
        );
        assert_eq!(
            info["heterogeneity_dimensions"][0][kind],
            json!([]),
            "{info}"
        );
        let model = parse(&source);
        if kind == "endogenous" {
            assert_eq!(structure_summary(&model).endogenous, 2);
            assert_eq!(model.final_endogenous().len(), 2);
            assert!(model
                .endogenous
                .iter()
                .find(|decl| model.name(decl.name) == name)
                .unwrap()
                .heterogeneity
                .is_some());
        }
        if let Some(pp) = find_preprocessor(None) {
            let official = run_preprocessor(
                &source,
                &pp,
                None,
                Duration::from_secs(30),
                JsonStage::Check,
            );
            assert!(official.success, "{official:?}");
        }
    }
}
