//! Configured inputs must reach the same diagnostics as equivalent source.

use std::time::Duration;

use dygnosis::analysis_configuration::{
    AnalysisConfiguration, AnalysisOverride, InitialDefinition, SourceStructureQuery,
};
use dygnosis::{JsonStage, Workspace, analyze};

fn make_workspace(source: &str) -> Workspace {
    let mut workspace = Workspace::new();
    workspace.update_document("main.mod", source);
    workspace
}

fn has_code(workspace: &mut Workspace, code: &str) -> bool {
    analyze(workspace.get_effective_model("main.mod").unwrap())
        .iter()
        .any(|diagnostic| diagnostic.code == code)
}

// Initial macro inputs must select the existing parser/check routes, while
// source inspection must not diagnose unexecuted model text.
#[test]
fn initial_inputs_preserve_generic_diagnostic_reach() {
    for (code, source) in [
        (
            "E001",
            "var y;model;\n@#ifdef ON\ny=0\n@#else\ny=0;\n@#endif\nend;",
        ),
        (
            "E020",
            "var y;model;\n@#ifdef ON\ny=missing;\n@#else\ny=0;\n@#endif\nend;",
        ),
        (
            "E030",
            "var y;\n@#ifdef ON\nparameters y;\n@#endif\nmodel;y=0;end;",
        ),
        (
            "E058",
            "var y;model;y=0;end;\n@#ifdef ON\ninitval;missing=0;end;\n@#endif\n",
        ),
        (
            "E065",
            "var y;varexo e;model;\n@#ifdef ON\ny=steady_state(e);\n@#else\ny=e;\n@#endif\nend;",
        ),
    ] {
        let mut workspace = make_workspace(source);
        assert!(!has_code(&mut workspace, code), "Source: {code}");
        workspace
            .source_structure("main.mod", &SourceStructureQuery::default())
            .unwrap();
        assert!(!has_code(&mut workspace, code), "source inspection: {code}");
        workspace
            .set_root_analysis_configuration(
                "main.mod",
                AnalysisConfiguration {
                    defines: vec![InitialDefinition {
                        name: "ON".into(),
                        expression: "false".into(),
                    }],
                    ..Default::default()
                },
            )
            .unwrap();
        assert!(has_code(&mut workspace, code), "configured: {code}");

        if let Some(preprocessor) = dygnosis::find_preprocessor(None) {
            let off = dygnosis::run_preprocessor(
                source,
                &preprocessor,
                None,
                Duration::from_secs(15),
                JsonStage::Check,
            );
            assert!(
                off.success,
                "Source {code}: {} {}",
                off.raw_stdout, off.raw_stderr
            );
            let equivalent = format!("@#define ON=false\n{source}");
            let on = dygnosis::run_preprocessor(
                &equivalent,
                &preprocessor,
                None,
                Duration::from_secs(15),
                JsonStage::Check,
            );
            assert!(!on.success, "configured {code} was accepted by the pin");
            let needle = match code {
                "E001" => "syntax error",
                "E020" | "E058" => "Unknown symbol",
                "E030" => "declared twice",
                "E065" => "STEADY_STATE",
                _ => unreachable!(),
            };
            assert!(
                format!("{}{}", on.raw_stdout, on.raw_stderr)
                    .to_lowercase()
                    .contains(&needle.to_lowercase()),
                "{code}: {} {}",
                on.raw_stdout,
                on.raw_stderr
            );
        }
    }
}

// Forcing a chain bypasses its evaluated guards, not the syntax validation of
// the written macro file or model diagnostics in the selected arm.
#[test]
fn forced_branch_keeps_selected_diagnostics_and_original_source() {
    let source = "var y;model;\n@#if UNKNOWN\ny=0;\n@#else\ny=missing;\n@#endif\nend;";
    let mut workspace = make_workspace(source);
    assert!(has_code(&mut workspace, "E063"));
    let structure = workspace
        .source_structure("main.mod", &SourceStructureQuery::default())
        .unwrap();
    let site = structure.files[0].sites.first().unwrap();
    workspace
        .set_root_analysis_configuration(
            "main.mod",
            AnalysisConfiguration {
                overrides: vec![AnalysisOverride::Branch {
                    target: site.target.clone(),
                    branch_id: Some(site.branches[1].branch_id.clone()),
                }],
                ..Default::default()
            },
        )
        .unwrap();
    assert!(!has_code(&mut workspace, "E063"));
    assert!(has_code(&mut workspace, "E020"));
    assert_eq!(workspace.get_source("main.mod"), Some(source));
    let metadata = workspace.analysis_metadata("main.mod").unwrap();
    assert!(metadata.selection.preview && metadata.macro_complete);

    let mut syntax = make_workspace("@#if 0\n@#define BROKEN=\n@#endif\nvar y;model;y=0;end;");
    syntax
        .set_root_analysis_configuration(
            "main.mod",
            AnalysisConfiguration {
                defines: vec![InitialDefinition {
                    name: "UNRELATED".into(),
                    expression: "1".into(),
                }],
                ..Default::default()
            },
        )
        .unwrap();
    assert!(has_code(&mut syntax, "E062"));
}
