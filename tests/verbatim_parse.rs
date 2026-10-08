//! The pinned VERBATIM_BLOCK reader keeps raw text through its actual terminator.

use std::{collections::HashMap, path::PathBuf, time::Duration};

use dygnosis::server::{new_service, Backend};
use dygnosis::{
    analyze, dynare_diagnose, find_preprocessor, parse, run_preprocessor, Diagnostic, JsonStage,
    Severity,
};
use tower_lsp::{lsp_types::*, LanguageServer};

const BASE: &str = "var y; varexo e; model; y=e; end;\n";

fn diagnostics(source: &str) -> Vec<Diagnostic> {
    analyze(&parse(source))
}

fn no_errors(source: &str) {
    let rows = diagnostics(source);
    assert!(
        rows.iter().all(|row| row.severity != Severity::Error),
        "{source}: {rows:?}"
    );
}

fn pinned_binary() -> Option<PathBuf> {
    let pinned = PathBuf::from("C:/dynare/7.2/preprocessor/dynare-preprocessor.exe");
    pinned.is_file().then_some(pinned).or_else(|| {
        find_preprocessor(None)
            .filter(|path| path.components().any(|part| part.as_os_str() == "7.2"))
    })
}

fn official(source: &str, accepted: bool, needle: Option<&str>) {
    let Some(binary) = pinned_binary() else {
        eprintln!("skipping verbatim honesty: pinned Dynare 7.2 binary is absent");
        return;
    };
    let result = run_preprocessor(
        source,
        &binary,
        None,
        Duration::from_secs(30),
        JsonStage::Check,
    );
    assert_eq!(result.success, accepted, "{source}: {result:?}");
    if let Some(needle) = needle {
        assert!(
            format!("{}{}", result.raw_stdout, result.raw_stderr).contains(needle),
            "{result:?}"
        );
    }
}

fn official_missing_names(source: &str, expected: &str) {
    let Some(binary) = pinned_binary() else {
        eprintln!("skipping verbatim honesty: pinned Dynare 7.2 binary is absent");
        return;
    };
    let result = run_preprocessor(
        source,
        &binary,
        None,
        Duration::from_secs(30),
        JsonStage::Check,
    );
    assert!(result.success, "{result:?}");
    let mut names: Vec<_> = result
        .raw_stdout
        .lines()
        .filter_map(|line| {
            line.strip_prefix("WARNING: in the 'steady_state_model' block, variable '")
                .and_then(|tail| tail.strip_suffix("' is not assigned a value"))
        })
        .collect();
    names.sort_unstable();
    assert_eq!(
        names,
        expected.split_whitespace().collect::<Vec<_>>(),
        "{result:?}"
    );
}

#[test]
fn bare_ends_are_raw_until_the_real_separator_or_eof() {
    for (body, tail) in [
        ("end\nend;\n", "shocks; var e=1; end;\n"),
        (
            "for j=1:2\nif j>1\ndisp(j)\nend\nend\nend \t ;\n",
            "shocks; var e=1; end;\n",
        ),
        ("for j=1:2\nend\nEND\n\t;\n", "shocks; var e=1; end;\n"),
        ("for j=1:2\nend\n", ""),
    ] {
        let source = format!("{BASE}verbatim;\n{body}{tail}");
        for source in [source.clone(), source.replace('\n', "\r\n")] {
            no_errors(&source);
            official(&source, true, None);
            let model = parse(&source);
            assert_eq!(model.endogenous.len(), 1);
            assert_eq!(model.equations.len(), 1);
            assert_eq!(model.shocks_block.is_some(), !tail.is_empty());
        }
    }
}

#[test]
fn raw_keywords_names_types_and_syntax_create_no_dynare_facts() {
    let body = "var hidden;\nparameters y;\nmodel; y=unknown(+1)/0;\nsteady_state_model; y=pp.rho;\n{ } \"raw\" 'unterminated\nparamters hidden\ny = 1\ny = 2\nend\n";
    let source =
        format!("{BASE}verbatim;\n{body}end;\nparameters p; p=1;\nshocks; var e=1; end;\n");
    no_errors(&source);
    official(&source, true, None);
    let model = parse(&source);
    assert_eq!(model.endogenous.len(), 1);
    assert_eq!(model.parameters.len(), 1);
    assert_eq!(model.intern.get(model.parameters[0].name), "p");
    assert_eq!(model.equations.len(), 1);
    assert!(model.steady_state_equations.is_empty());
    assert!(model.ss_block.is_none());
    assert_eq!(model.param_assignments.len(), 1);
    assert!(model.helper_assignments.is_empty());
    assert!(model.shocks_block.is_some());
}

#[test]
fn terminator_has_the_pinned_substring_quote_and_comment_rules() {
    // Flex has no identifier boundary or MATLAB quoting/comment state here.
    for body in [
        "friend;", "'end;", "'end;'", "\"end;", "%end;", "//end;", "/*end;", "/*end;*/",
    ] {
        let source = format!("{BASE}verbatim;\n{body}\nparameters p; p=1;\n");
        official(&source, true, None);
        no_errors(&source);
        let model = parse(&source);
        assert_eq!(model.parameters.len(), 1, "{source}: {model:?}");
        assert_eq!(model.param_assignments.len(), 1, "{source}: {model:?}");
    }
    // A comment between END and ';' is raw; it does not satisfy [[:space:]]*.
    let source = format!(
        "{BASE}verbatim;\nend /* keep */ ;\nparameters hidden;\nend;\nparameters p; p=1;\n"
    );
    official(&source, true, None);
    no_errors(&source);
    let model = parse(&source);
    assert_eq!(model.parameters.len(), 1);
    assert_eq!(model.intern.get(model.parameters[0].name), "p");

    // The raw rule also ends inside double quotes. Its trailing quote is then
    // read in INITIAL/NATIVE, where the pin refuses an unmatched double quote.
    let quoted = format!("{BASE}verbatim;\n\"end;\"\nparameters p; p=1;\n");
    official(&quoted, false, Some("character unrecognized by lexer"));
    let rows = diagnostics(&quoted);
    let errors: Vec<_> = rows.iter().filter(|row| row.code == "E001").collect();
    assert_eq!(errors.len(), 1, "{rows:?}");
    assert_eq!(errors[0].message, "character unrecognized by lexer");
    assert_eq!(
        &quoted[errors[0].span.start as usize..errors[0].span.end as usize],
        "\""
    );
    assert_eq!(parse(&quoted).parameters.len(), 1);

    for body in ["friend;", "/*end;", "%end;"] {
        let source = format!("{BASE}verbatim;\n{body}parameters p; p=1;\nshocks; var e=1; end;\n");
        official(&source, true, None);
        no_errors(&source);
        let model = parse(&source);
        assert_eq!(model.parameters.len(), 1, "{source}: {model:?}");
        assert_eq!(model.param_assignments.len(), 1, "{source}: {model:?}");
        assert!(model.shocks_block.is_some());
    }
}

#[test]
fn macro_execution_selects_raw_blocks_and_keeps_written_following_statements() {
    let source = format!("{BASE}@#for j in 1:2\nverbatim;\nend\n{{ \"raw\" }}\nend;\n@#endfor\n@#if 0\nverbatim;\nvar hidden;\nend;\n@#else\nparameters p; p=1;\n@#endif\nshocks; var e=1; end;\n");
    official(&source, true, None);
    no_errors(&source);
    let model = parse(&source);
    assert_eq!(model.endogenous.len(), 1);
    assert_eq!(model.parameters.len(), 1);
    assert_eq!(model.param_assignments.len(), 1);
    assert!(model.shocks_block.is_some());
    let emitted = format!(
        "{BASE}@#define finish = \"end;\"\nverbatim;\nend\n@{{finish}}\nparameters p; p=1;\n"
    );
    official(&emitted, true, None);
    no_errors(&emitted);
    assert_eq!(parse(&emitted).param_assignments.len(), 1);
}

#[test]
fn native_and_real_dynare_boundaries_remain_distinct_after_verbatim() {
    let source = format!("{BASE}verbatim;\nend\nend;\ndisp('native')\nparameters p; p=1;\nproof=1 ... /*\n*/\np=pp.rho;\nshocks; var e=1; end;\n");
    official(&source, true, None);
    no_errors(&source);
    let model = parse(&source);
    assert_eq!(model.param_assignments.len(), 1);
    assert!(model.shocks_block.is_some());

    let same_line =
        format!("{BASE}verbatim;\nend;\nproof=1; parameters hidden;\nparameters p; p=1;\n");
    official(&same_line, true, None);
    no_errors(&same_line);
    assert_eq!(parse(&same_line).parameters.len(), 1);

    let missing = format!("{BASE}verbatim;\nend\nend;\nshocks; var e=1 end;\n");
    let rows = diagnostics(&missing);
    assert!(rows.iter().any(|row| row.code == "E001"), "{rows:?}");
    official(&missing, false, Some("syntax error, unexpected END"));
}

#[test]
fn raw_region_excludes_generic_refusals_and_the_next_real_statement_retains_them() {
    for (statement, code, accepted) in [
        ("model; y=unknown; end;\n", "E020", false),
        ("parameters y;\n", "E030", false),
        ("var y;\n", "W031", true),
        ("forecast e;\n", "E240", false),
        ("forecast unknown;\n", "E239", false),
        ("varexo_det d; model; y=d(-1); end;\n", "E024", false),
        ("shocks; var e=1 end;\n", "E001", false),
        ("parameters p; p=pp.rho;\n", "E275", false),
        ("model; y=1/0; end;\n", "E278", false),
    ] {
        let raw = format!("{BASE}verbatim;\n{statement}end;\n");
        let quiet = diagnostics(&raw);
        assert!(quiet.iter().all(|row| row.code != code), "{raw}: {quiet:?}");
        let real = format!("{raw}{statement}");
        let fire = diagnostics(&real);
        assert!(fire.iter().any(|row| row.code == code), "{real}: {fire:?}");
        official(&raw, true, None);
        official(&real, accepted, None);
    }
}

#[test]
fn included_raw_region_resumes_on_the_written_declaration_and_shock_owners() {
    let root = "C:/verbatim-test/root.mod";
    let child = "C:/verbatim-test/raw.inc";
    let source = format!("{BASE}@#include \"raw.inc\"\n");
    let files = HashMap::from([
        (root.to_string(), "stale disk text".to_string()),
        (
            child.to_string(),
            "verbatim;\nend\n{ \"raw\" }\nend;\nparameters p; p=pp.rho;\nshocks; var e=1; end;\n"
                .to_string(),
        ),
    ]);
    let rows = dynare_diagnose(&source, Some(root), Some(&files));
    assert!(rows.iter().all(|row| row.code != "E001"), "{rows:?}");
    let errors: Vec<_> = rows.iter().filter(|row| row.code == "E275").collect();
    assert_eq!(errors.len(), 1, "{rows:?}");
    assert_eq!(errors[0].file.as_deref(), Some(child));
    assert_eq!(errors[0].line, 5);
    assert_eq!(errors[0].column, 17);
    assert_eq!(
        errors[0].message,
        "Namespace-qualified symbol pp.rho not allowed in this context"
    );
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
async fn lsp_and_mcp_keep_repeated_unsaved_include_refusals_after_raw_text() {
    let root = "C:/verbatim-wire/root.mod";
    let include = "C:/verbatim-wire/raw.inc";
    let root_uri = Url::parse("file:///C:/verbatim-wire/root.mod").unwrap();
    let include_uri = Url::parse("file:///C:/verbatim-wire/raw.inc").unwrap();
    let source =
        format!("{BASE}parameters p; p=1;\n@#for j in 1:2\n@#include \"raw.inc\"\n@#endfor\n");
    let fire = "verbatim;\nend\nend;\n/*😀中*/p=pp.rho;\n";
    let quiet = "verbatim;\nend\n/*😀中*/p=pp.rho;\nend;\n";
    let (service, _socket) = new_service();
    let server = service.inner();
    for (uri, text) in [(&include_uri, fire), (&root_uri, source.as_str())] {
        server
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: uri.clone(),
                    language_id: "dynare".into(),
                    version: 1,
                    text: text.into(),
                },
            })
            .await;
    }
    for (version, text, expected) in [(1, fire, 2), (2, quiet, 0)] {
        if version == 2 {
            server
                .did_change(DidChangeTextDocumentParams {
                    text_document: VersionedTextDocumentIdentifier {
                        uri: include_uri.clone(),
                        version,
                    },
                    content_changes: vec![TextDocumentContentChangeEvent {
                        range: None,
                        range_length: None,
                        text: text.into(),
                    }],
                })
                .await;
        }
        let files = HashMap::from([
            (root.to_string(), "stale disk contents".to_string()),
            (include.to_string(), text.to_string()),
        ]);
        let mcp = dynare_diagnose(&source, Some(root), Some(&files));
        let root_rows = pull(server, &root_uri).await;
        let lsp = pull(server, &include_uri).await;
        assert!(mcp.iter().all(|row| row.code != "E001"), "{mcp:?}");
        assert!(
            root_rows
                .iter()
                .all(|row| row.code != Some(NumberOrString::String("E275".into()))),
            "{root_rows:?}"
        );
        let mcp: Vec<_> = mcp.iter().filter(|row| row.code == "E275").collect();
        let lsp: Vec<_> = lsp
            .iter()
            .filter(|row| row.code == Some(NumberOrString::String("E275".into())))
            .collect();
        assert_eq!(mcp.len(), expected, "{mcp:?}");
        assert_eq!(lsp.len(), expected, "{lsp:?}");
        for row in mcp {
            assert_eq!(row.file.as_deref(), Some(include));
            assert_eq!(row.severity, "ERROR");
            assert_eq!(
                row.message,
                "Namespace-qualified symbol pp.rho not allowed in this context"
            );
            assert_eq!(
                (row.line, row.column, row.end_line, row.end_column),
                (4, 9, 4, 15)
            );
        }
        for row in lsp {
            assert_eq!(row.severity, Some(DiagnosticSeverity::ERROR));
            assert_eq!(
                row.message,
                "Namespace-qualified symbol pp.rho not allowed in this context"
            );
            assert_eq!(
                row.range,
                Range::new(Position::new(3, 9), Position::new(3, 15))
            );
        }
    }
}

#[test]
fn unchanged_gali_archive_and_its_encoding_only_utf8_copy_restore_all_w042_names() {
    let bytes = include_bytes!(
        "../.agents/skills/use-dynare/references/examples-code/Gali_2015/Gali_2015_chapter_7.mod"
    );
    // All non-ASCII archive bytes are >= A0, whose Windows-1252 decoding is
    // identical to their Unicode code point. This does not normalize text.
    assert!(bytes.iter().all(|byte| !(0x80..=0x9f).contains(byte)));
    let original: String = bytes.iter().map(|byte| char::from(*byte)).collect();
    let utf8 = String::from_utf8(original.as_bytes().to_vec()).unwrap();
    let expected = "a c i i_ann l m_growth_ann m_nominal m_real mu_p n nu p pi_p pi_p_ann pi_w pi_w_ann r_nat r_nat_ann r_real r_real_ann u uhat w w_gap w_nat w_real y y_gap y_nat yhat z";
    for source in [&original, &utf8] {
        no_errors(source);
        let rows = diagnostics(source);
        let mut names: Vec<_> = rows
            .iter()
            .filter(|row| row.code == "W042")
            .map(|row| {
                assert_eq!(row.severity, Severity::Warning);
                assert_eq!(
                    &source[row.span.start as usize..row.span.end as usize],
                    "steady_state_model"
                );
                row.message
                    .strip_prefix("variable '")
                    .unwrap()
                    .strip_suffix("' is not assigned a value")
                    .unwrap()
            })
            .collect();
        names.sort_unstable();
        assert_eq!(
            names,
            expected.split_whitespace().collect::<Vec<_>>(),
            "{rows:?}"
        );
        let mcp = dynare_diagnose(source, None, None);
        let lsp = dygnosis::server::diagnostics_for("file:///C:/verbatim-wire/gali.mod", source);
        assert!(mcp.iter().all(|row| row.code != "E001"), "{mcp:?}");
        assert!(
            lsp.iter()
                .all(|row| row.code != Some(NumberOrString::String("E001".into()))),
            "{lsp:?}"
        );
        let mut mcp_messages: Vec<_> = mcp
            .iter()
            .filter(|row| row.code == "W042")
            .map(|row| {
                assert_eq!(row.severity, "WARNING");
                assert_eq!(
                    (row.line, row.column, row.end_line, row.end_column),
                    (221, 1, 221, 19)
                );
                row.message.as_str()
            })
            .collect();
        let mut lsp_messages: Vec<_> = lsp
            .iter()
            .filter(|row| row.code == Some(NumberOrString::String("W042".into())))
            .map(|row| {
                assert_eq!(row.severity, Some(DiagnosticSeverity::WARNING));
                assert_eq!(
                    row.range,
                    Range::new(Position::new(220, 0), Position::new(220, 18))
                );
                row.message.as_str()
            })
            .collect();
        let mut messages: Vec<_> = rows
            .iter()
            .filter(|row| row.code == "W042")
            .map(|row| row.message.as_str())
            .collect();
        messages.sort_unstable();
        mcp_messages.sort_unstable();
        lsp_messages.sort_unstable();
        assert_eq!(mcp_messages, messages);
        assert_eq!(lsp_messages, messages);
        official_missing_names(source, expected);
    }
}
