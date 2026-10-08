use std::{collections::HashMap, path::PathBuf, time::Duration};

use dygnosis::server::{new_service, Backend};
use dygnosis::{
    analyze, dynare_diagnose, find_preprocessor, parse, run_preprocessor, Diagnostic, JsonStage,
    Severity,
};
use tower_lsp::{lsp_types::*, LanguageServer};

const MODEL: &str = "var y r z; model(linear); y=r+z; end;";
const OBJECTIVE: &str = "planner_objective y^2;";
const SSM: &str = "steady_state_model; y=0; end;";

fn missing(rows: &[Diagnostic]) -> Vec<&str> {
    let mut names: Vec<_> = rows
        .iter()
        .filter(|row| row.code == "W042")
        .map(|row| {
            row.message
                .strip_prefix("variable '")
                .unwrap()
                .split('\'')
                .next()
                .unwrap()
        })
        .collect();
    names.sort_unstable();
    names
}

fn official(source: &str, accepted: bool, names: &[&str], refusal: Option<&str>) {
    let pinned = PathBuf::from("C:/dynare/7.2/preprocessor/dynare-preprocessor.exe");
    let binary = pinned.is_file().then_some(pinned).or_else(|| {
        find_preprocessor(None)
            .filter(|path| path.components().any(|part| part.as_os_str() == "7.2"))
    });
    let Some(pp) = binary else {
        return;
    };
    let result = run_preprocessor(source, &pp, None, Duration::from_secs(30), JsonStage::Check);
    assert_eq!(result.success, accepted, "{source}: {result:?}");
    let mut actual: Vec<_> = result
        .raw_stdout
        .lines()
        .filter_map(|line| {
            line.strip_prefix("WARNING: in the 'steady_state_model' block, variable '")
                .and_then(|tail| tail.strip_suffix("' is not assigned a value"))
        })
        .collect();
    actual.sort_unstable();
    assert_eq!(actual, names, "{source}: {result:?}");
    if let Some(sentence) = refusal {
        assert!(result.raw_stdout.contains(sentence), "{source}: {result:?}");
    }
}

#[test]
fn ramsey_exempts_only_its_instruments_before_or_after_the_steady_state_block() {
    for command in ["ramsey_model", "ramsey_policy"] {
        for (instruments, expected) in [("r", vec!["z"]), ("r,z", vec![])] {
            for before in [true, false] {
                let policy = format!("{command}(instruments=({instruments}));");
                let source = if before {
                    format!("{MODEL}{OBJECTIVE}{policy}{SSM}")
                } else {
                    format!("{MODEL}{SSM}{OBJECTIVE}{policy}")
                };
                official(&source, true, &expected, None);
                let rows = analyze(&parse(&source));
                assert_eq!(missing(&rows), expected, "{source}: {rows:?}");
                for warning in rows.iter().filter(|row| row.code == "W042") {
                    assert_eq!(warning.severity, Severity::Warning);
                    assert_eq!(
                        &source[warning.span.start as usize..warning.span.end as usize],
                        "steady_state_model"
                    );
                    assert!(warning.fix.is_none());
                }
            }
        }
    }
}

#[test]
fn ordinary_discretionary_and_unlisted_ramsey_names_keep_their_warnings() {
    for policy in [
        "",
        "discretionary_policy(instruments=(r));",
        "ramsey_model;",
        "ramsey_policy;",
    ] {
        let objective = if policy.is_empty() { "" } else { OBJECTIVE };
        let source = format!("{MODEL}{objective}{policy}{SSM}");
        official(&source, true, &["r", "z"], None);
        let rows = analyze(&parse(&source));
        assert_eq!(missing(&rows), ["r", "z"], "{source}: {rows:?}");
    }
}

#[test]
fn mixed_policy_keeps_the_last_supplied_instruments_before_the_e202_refusal() {
    for command in ["ramsey_model", "ramsey_policy"] {
        for (ramsey_options, before, expected) in [
            ("(instruments=(r))", true, vec!["r"]),
            ("(instruments=(r))", false, vec!["z"]),
            ("", true, vec!["r"]),
            ("", false, vec!["r"]),
        ] {
            let ramsey = format!("{command}{ramsey_options};");
            let discretionary = "discretionary_policy(instruments=(z));";
            let policy = if before {
                format!("{ramsey}{discretionary}")
            } else {
                format!("{discretionary}{ramsey}")
            };
            let source = format!("{MODEL}{OBJECTIVE}{policy}{SSM}");
            official(
                &source,
                false,
                &expected,
                Some("You cannot use the discretionary_policy command when you use either ramsey_model or ramsey_policy and vice versa"),
            );
            let rows = analyze(&parse(&source));
            assert_eq!(missing(&rows), expected, "{source}: {rows:?}");
            assert!(
                rows.iter().any(|row| row.code == "E202"),
                "{source}: {rows:?}"
            );
        }
    }
}

#[test]
fn macro_mixed_policy_uses_execution_order_when_written_spans_run_backwards() {
    for command in ["ramsey_model", "ramsey_policy"] {
        let source = format!(
            "{MODEL}{OBJECTIVE}{SSM}\n@#for n in [2,1]\n@#if n == 1\n{command}(instruments=(r));\n@#else\ndiscretionary_policy(instruments=(z));\n@#endif\n@#endfor\n"
        );
        official(
            &source,
            false,
            &["z"],
            Some("You cannot use the discretionary_policy command"),
        );
        let rows = analyze(&parse(&source));
        assert_eq!(missing(&rows), ["z"], "{source}: {rows:?}");
        assert!(
            rows.iter().any(|row| row.code == "E202"),
            "{source}: {rows:?}"
        );
    }
}

#[test]
fn bracketed_targets_accept_and_empty_blocks_refuse_before_check() {
    for body in ["[y,z]=foo(1);", ""] {
        let source = format!(
            "{MODEL}{OBJECTIVE}ramsey_model(instruments=(r));steady_state_model;{body}end;"
        );
        official(
            &source,
            !body.is_empty(),
            &[],
            body.is_empty().then_some("syntax error, unexpected END"),
        );
        let model = parse(&source);
        let diagnostics = analyze(&model);
        assert!(missing(&diagnostics).is_empty());
        if body.is_empty() {
            assert!(model.ss_block.is_none());
            assert!(diagnostics.iter().any(|row| {
                row.code == "E001" && row.message == "syntax error, unexpected END"
            }));
        } else {
            assert!(model.ss_block.is_some());
        }
    }
}

#[test]
fn declared_retyped_and_repeated_instruments_keep_the_same_exemption() {
    for prefix in [
        "var y r z;",
        "parameters r; change_type(var) r; var y z;",
        "var y r z; var_remove r; change_type(var) r;",
    ] {
        let source = format!(
            "{prefix}model(linear);y=r+z;end;{OBJECTIVE}ramsey_model(instruments=(r,r));{SSM}"
        );
        official(&source, true, &["z"], None);
        let rows = analyze(&parse(&source));
        assert_eq!(missing(&rows), ["z"], "{source}: {rows:?}");
        assert!(
            rows.iter()
                .all(|row| !matches!(row.code.as_str(), "E101" | "E317" | "E271")),
            "{source}: {rows:?}"
        );
    }
}

#[test]
fn final_types_and_removal_control_noninstrument_warnings() {
    for (prefix, model_body, suffix, expected) in [
        ("var y r z;", "y=r+z;", "", vec!["z"]),
        (
            "var y r z; change_type(parameters) z;",
            "y=r+z;",
            "",
            vec![],
        ),
        (
            "parameters z; change_type(var) z; var y r;",
            "y=r+z;",
            "",
            vec!["z"],
        ),
        ("var y r z; var_remove z;", "y=r;", "", vec![]),
        (
            "var y r z;",
            "[name='Y'] y=r; [name='Z'] z=0;",
            "model_remove('Z');",
            vec![],
        ),
    ] {
        let source = format!(
            "{prefix}model(linear);{model_body}end;{suffix}{OBJECTIVE}ramsey_model(instruments=(r));{SSM}"
        );
        official(&source, true, &expected, None);
        let rows = analyze(&parse(&source));
        assert_eq!(missing(&rows), expected, "{source}: {rows:?}");
    }
}

#[test]
fn invalid_instruments_stop_the_warning_before_later_declarations_or_retypes() {
    for command in ["ramsey_model", "ramsey_policy", "discretionary_policy"] {
        for (prefix, instrument, suffix, code, sentence) in [
            ("", "missing", "", "E101", "Unknown symbol: missing"),
            (
                "",
                "missing",
                "var missing;",
                "E101",
                "Unknown symbol: missing",
            ),
            ("parameters p;", "p", "", "E317", "p is not endogenous."),
            (
                "parameters p;",
                "p",
                "change_type(var) p;",
                "E317",
                "p is not endogenous.",
            ),
            ("varexo e;", "e", "", "E317", "e is not endogenous."),
        ] {
            let source = format!(
                "{prefix}{MODEL}{OBJECTIVE}{SSM}{command}(instruments=(r,{instrument}));{suffix}"
            );
            official(&source, false, &[], Some(sentence));
            let rows = analyze(&parse(&source));
            assert!(
                rows.iter().any(|row| row.code == code),
                "{source}: {rows:?}"
            );
            assert!(missing(&rows).is_empty(), "{source}: {rows:?}");
        }
    }
}

#[test]
fn repeated_or_mixed_ramsey_commands_stop_w042_at_parse() {
    for (policy, code, sentence) in [
        (
            "ramsey_model; ramsey_model;",
            "E297",
            "Several 'ramsey_model' statements cannot appear",
        ),
        (
            "ramsey_policy; ramsey_model;",
            "E298",
            "A 'ramsey_model' statement cannot follow a 'ramsey_policy' statement.",
        ),
        (
            "ramsey_model; ramsey_policy;",
            "E299",
            "A 'ramsey_policy' statement cannot follow a 'ramsey_model' statement.",
        ),
        (
            "ramsey_policy; ramsey_policy;",
            "E300",
            "Several 'ramsey_policy' statements cannot appear",
        ),
    ] {
        for before in [true, false] {
            let source = if before {
                format!("{MODEL}{OBJECTIVE}{policy}{SSM}")
            } else {
                format!("{MODEL}{SSM}{OBJECTIVE}{policy}")
            };
            official(&source, false, &[], Some(sentence));
            let rows = analyze(&parse(&source));
            assert!(
                rows.iter().any(|row| row.code == code),
                "{source}: {rows:?}"
            );
            assert!(missing(&rows).is_empty(), "{source}: {rows:?}");
        }
    }
}

#[test]
fn other_recorded_policy_parse_refusals_also_stop_w042() {
    for (prefix, command, code, sentence) in [
        (
            "parameters optimal_policy_discount_factor;",
            "ramsey_model(planner_discount=0.9);",
            "E301",
            "ramsey_model: the 'planner_discount' option cannot be used",
        ),
        (
            "parameters optimal_policy_discount_factor;",
            "ramsey_policy(planner_discount=0.9);",
            "E302",
            "ramsey_policy: the 'planner_discount' option cannot be used",
        ),
        (
            "var optimal_policy_discount_factor;",
            "discretionary_policy(instruments=(r));",
            "E378",
            "optimal_policy_discount_factor is not a parameter",
        ),
    ] {
        let source = format!("{prefix}{MODEL}{OBJECTIVE}{SSM}{command}");
        official(&source, false, &[], Some(sentence));
        let rows = analyze(&parse(&source));
        assert!(
            rows.iter().any(|row| row.code == code),
            "{source}: {rows:?}"
        );
        assert!(missing(&rows).is_empty(), "{source}: {rows:?}");
    }
}

#[test]
fn macro_execution_order_retains_the_refusal_when_spans_repeat_or_run_backwards() {
    for (policy, code) in [
        ("@#for n in 1:2\nramsey_model;\n@#endfor\n", "E297"),
        (
            "@#for n in [2,1]\n@#if n == 1\nramsey_model;\n@#else\nramsey_policy;\n@#endif\n@#endfor\n",
            "E298",
        ),
    ] {
        let source = format!("{MODEL}{OBJECTIVE}{SSM}\n{policy}");
        official(&source, false, &[], None);
        let rows = analyze(&parse(&source));
        assert!(rows.iter().any(|row| row.code == code), "{source}: {rows:?}");
        assert!(missing(&rows).is_empty(), "{source}: {rows:?}");
    }
    let source = format!(
        "{MODEL}{OBJECTIVE}{SSM}\n@#if 0\ndiscretionary_policy(instruments=(z));\n@#else\nramsey_model(instruments=(r));\n@#endif\n"
    );
    official(&source, true, &["z"], None);
    assert_eq!(missing(&analyze(&parse(&source))), ["z"]);
}

#[test]
fn later_transform_and_writer_refusals_do_not_hide_w042() {
    for (source, code, expected) in [
        (
            format!(
                "var y r z; varexo e; model(linear); y=r+z+e; end;{OBJECTIVE}ramsey_model(instruments=(r));{SSM}shocks(surprise); var e; periods 1; values 1; end;"
            ),
            "E178",
            vec!["z"],
        ),
        (
            format!(
                "var y r z; varexo e; varexo_det d; model; y=e; r=y; z=r; end;{SSM}d.subsamples(s=2000Q1:2000Q2);"
            ),
            "E431",
            vec!["r", "z"],
        ),
    ] {
        official(&source, true, &expected, None);
        let rows = analyze(&parse(&source));
        assert!(
            rows.iter().any(|row| row.code == code),
            "{source}: {rows:?}"
        );
        assert_eq!(missing(&rows), expected, "{source}: {rows:?}");
    }
}

#[test]
fn mcp_uses_unsaved_include_owners_and_clears_w042_after_a_policy_refusal() {
    let root = "C:/ramsey-scope/root.mod";
    let include = "C:/ramsey-scope/ss.inc";
    let root_source =
        format!("{MODEL}{OBJECTIVE}ramsey_model(instruments=(r));\n@#include \"ss.inc\"\n");
    let files = HashMap::from([
        (root.to_string(), "old disk contents".to_string()),
        (include.to_string(), format!("/* 🚀 */{SSM}")),
    ]);
    let rows = dynare_diagnose(&root_source, Some(root), Some(&files));
    let warnings: Vec<_> = rows.iter().filter(|row| row.code == "W042").collect();
    assert_eq!(warnings.len(), 1, "{rows:?}");
    assert_eq!(warnings[0].message, "variable 'z' is not assigned a value");
    assert_eq!(warnings[0].file.as_deref(), Some(include));
    assert_eq!(warnings[0].column, 8);
    assert_eq!(warnings[0].end_column, 26);
    let refused = format!("{root_source}ramsey_model;\n");
    let rows = dynare_diagnose(&refused, Some(root), Some(&files));
    assert!(rows.iter().any(|row| row.code == "E297"), "{rows:?}");
    assert!(rows.iter().all(|row| row.code != "W042"), "{rows:?}");
}

async fn pull(server: &Backend, uri: &Url) -> Vec<tower_lsp::lsp_types::Diagnostic> {
    match server
        .diagnostic(DocumentDiagnosticParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            identifier: None,
            previous_result_id: None,
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        })
        .await
        .unwrap()
    {
        DocumentDiagnosticReportResult::Report(DocumentDiagnosticReport::Full(full)) => {
            full.full_document_diagnostic_report.items
        }
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn lsp_unsaved_policy_edits_update_the_same_w042_warning() {
    let (service, _socket) = new_service();
    let server = service.inner();
    let uri = Url::parse("file:///C:/ramsey-scope/live.mod").unwrap();
    let source = format!("{MODEL}{OBJECTIVE}ramsey_model(instruments=(r));\n{SSM}");
    server
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "dynare".into(),
                version: 1,
                text: source.clone(),
            },
        })
        .await;
    let rows = pull(server, &uri).await;
    let warnings: Vec<_> = rows
        .iter()
        .filter(|row| row.code == Some(NumberOrString::String("W042".into())))
        .collect();
    assert_eq!(warnings.len(), 1, "{rows:?}");
    assert_eq!(warnings[0].message, "variable 'z' is not assigned a value");
    assert_eq!(
        warnings[0].range,
        Range::new(Position::new(1, 0), Position::new(1, 18))
    );
    server
        .did_change(DidChangeTextDocumentParams {
            text_document: VersionedTextDocumentIdentifier {
                uri: uri.clone(),
                version: 2,
            },
            content_changes: vec![TextDocumentContentChangeEvent {
                range: None,
                range_length: None,
                text: format!("{source}ramsey_model;"),
            }],
        })
        .await;
    let rows = pull(server, &uri).await;
    assert!(rows
        .iter()
        .any(|row| row.code == Some(NumberOrString::String("E297".into()))));
    assert!(rows
        .iter()
        .all(|row| row.code != Some(NumberOrString::String("W042".into()))));
}
