//! The existing stochastic row grammar must reach the shared duplicate keys.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use dygnosis::server::{new_service, Backend};
use dygnosis::{analyze, dynare_diagnose, parse, run_preprocessor, JsonStage, Severity};
use tower_lsp::lsp_types::*;
use tower_lsp::LanguageServer;

const PREFIX: &str = "heterogeneity_dimension d;\nvar y;\nvarexo e;\nvar(heterogeneity=d) a;\nvarexo(heterogeneity=d) eh fh gh;\nmodel; y=SUM(a)+e; end;\nmodel(heterogeneity=d); a=a(-1)+eh+fh+gh; end;\n";
const CASES: [(&str, &str, &str, &str); 12] = [
    (
        "var eh=.1;",
        "var eh=.2;",
        "E111",
        "shocks: variance or stderr of shock on eh declared twice",
    ),
    (
        "var eh; stderr .1;",
        "var eh; stderr .2;",
        "E111",
        "shocks: variance or stderr of shock on eh declared twice",
    ),
    (
        "var eh=.1;",
        "var eh; stderr .2;",
        "E111",
        "shocks: variance or stderr of shock on eh declared twice",
    ),
    (
        "var eh; stderr .1;",
        "var eh=.2;",
        "E111",
        "shocks: variance or stderr of shock on eh declared twice",
    ),
    (
        "var eh,fh=.1;",
        "var eh,fh=.2;",
        "E111",
        "shocks: covariance or correlation shock on variable pair (eh, fh) declared twice",
    ),
    (
        "var eh,fh=.1;",
        "var fh,eh=.2;",
        "E111",
        "shocks: covariance or correlation shock on variable pair (fh, eh) declared twice",
    ),
    (
        "corr eh,fh=.1;",
        "corr eh,fh=.2;",
        "E111",
        "shocks: covariance or correlation shock on variable pair (eh, fh) declared twice",
    ),
    (
        "corr eh,fh=.1;",
        "corr fh,eh=.2;",
        "E111",
        "shocks: covariance or correlation shock on variable pair (fh, eh) declared twice",
    ),
    (
        "var eh,fh=.1;",
        "corr fh,eh=.2;",
        "E111",
        "shocks: covariance or correlation shock on variable pair (fh, eh) declared twice",
    ),
    (
        "corr eh,fh=.1;",
        "var fh,eh=.2;",
        "E111",
        "shocks: covariance or correlation shock on variable pair (fh, eh) declared twice",
    ),
    (
        "skew eh=.1;",
        "skew eh=.2;",
        "E393",
        "shocks: skewness of eh declared twice",
    ),
    (
        "skew eh,fh,gh=.1;",
        "skew gh,eh,fh=.2;",
        "E394",
        "shocks: co-skewness of (gh, eh, fh) declared twice",
    ),
];

fn block(body: &str) -> String {
    format!("shocks(heterogeneity=d);\n{body}\nend;\n")
}

fn pinned_check(source: &str, message: Option<&str>) {
    let binary = PathBuf::from("C:/dynare/7.2/preprocessor/dynare-preprocessor.exe");
    if !binary.is_file() {
        return;
    }
    let report = run_preprocessor(
        source,
        &binary,
        None,
        Duration::from_secs(30),
        JsonStage::Check,
    );
    let text = format!("{} {}", report.raw_stdout, report.raw_stderr);
    if let Some(message) = message {
        assert!(
            !report.success && text.contains(message),
            "{source}\n{text}"
        );
    } else {
        assert!(report.success, "{source}\n{text}");
    }
}

fn duplicate_codes(source: &str) -> Vec<dygnosis::Diagnostic> {
    analyze(&parse(source))
        .into_iter()
        .filter(|row| matches!(row.code.as_str(), "E111" | "E393" | "E394"))
        .collect()
}

#[test]
fn twelve_recorded_shapes_keep_official_wording_and_first_row() {
    for (first, second, code, message) in CASES {
        let source = format!("{PREFIX}{}", block(&format!("{first}\n{second}\n{second}")));
        let model = parse(&source);
        assert!(
            model.shock_stmts.is_empty(),
            "heterogeneous rows stay on their block"
        );
        assert_eq!(model.shock_blocks[0].stochastic.len(), 3);
        let duplicates = duplicate_codes(&source);
        assert_eq!(duplicates.len(), 2, "{source}: {duplicates:?}");
        for row in duplicates {
            assert_eq!(row.code, code);
            assert_eq!(row.severity, Severity::Error);
            assert_eq!(row.message, message);
            assert_eq!(row.related.len(), 1);
            assert_eq!(
                row.related[0].span,
                model.shock_blocks[0].stochastic[0].span
            );
        }
        pinned_check(&source, Some(message));
        let rows = dynare_diagnose(&source, None, None);
        let rows: Vec<_> = rows.iter().filter(|row| row.code == code).collect();
        assert_eq!(rows.len(), 2);
        assert!(rows
            .iter()
            .all(|row| row.related[0]["line"] == 9 && row.related[0]["column"] == 1));
        let quiet = format!("{PREFIX}{}", block(first));
        assert!(duplicate_codes(&quiet).is_empty());
        pinned_check(&quiet, None);
    }
}

#[test]
fn block_resets_match_the_pinned_skew_map_lifetime() {
    for (first, second, code, message) in CASES {
        let source = format!("{PREFIX}{}{}", block(first), block(second));
        let duplicates = duplicate_codes(&source);
        if code == "E111" {
            assert!(duplicates.is_empty(), "{source}");
            pinned_check(&source, None);
        } else {
            assert_eq!(duplicates.len(), 1);
            assert_eq!(duplicates[0].message, message);
            assert_eq!(
                duplicates[0].related[0].span.start as usize,
                source.find(first).unwrap()
            );
            pinned_check(&source, Some(message));
        }
    }
    for (body, message) in [
        ("shocks(heterogeneity=d); skew eh=.1; end; shocks; skew eh=.2; end;", Some("shocks: skewness of eh declared twice")),
        ("shocks; skew e=.1; end; shocks(heterogeneity=d); skew e=.2; end;", None),
        ("shocks(heterogeneity=d); skew e=.1; end; shocks; var e=.1; end; shocks(heterogeneity=d); skew e=.2; end;", None),
        ("shocks; skew e=.1; end; shocks; skew e=.2; end;", None),
    ] {
        let source = format!("{PREFIX}{body}");
        assert_eq!(!duplicate_codes(&source).is_empty(), message.is_some(), "{source}");
        pinned_check(&source, message);
    }
    let source = format!(
        "{PREFIX}heterogeneity_dimension d2; {}shocks(heterogeneity=d2); skew eh=.2; end;",
        block("skew eh=.1;")
    );
    assert_eq!(duplicate_codes(&source)[0].code, "E393");
    pinned_check(&source, Some("shocks: skewness of eh declared twice"));
}

#[test]
fn permutations_same_name_triples_and_retyping_preserve_keys() {
    for body in [
        "skew eh=.1; skew eh,eh,eh=.2;",
        "skew eh,eh,eh=.1; skew eh=.2;",
        "skew eh,fh,gh=.1; skew fh,gh,eh=.2;",
    ] {
        let source = format!("{PREFIX}{}", block(body));
        let row = duplicate_codes(&source).remove(0);
        pinned_check(&source, Some(&row.message));
    }
    let retyped = format!(
        "{PREFIX}change_type(parameters) eh; change_type(varexo) eh; {}",
        block("var eh=.1; var eh=.2;")
    );
    assert_eq!(duplicate_codes(&retyped)[0].code, "E111");
    pinned_check(
        &retyped,
        Some("shocks: variance or stderr of shock on eh declared twice"),
    );
    let quiet = retyped.replace("var eh=.2;", "");
    assert!(duplicate_codes(&quiet).is_empty());
    pinned_check(&quiet, Some("shocks: setting a variance on 'eh' is not allowed, because it is not a heterogeneous exogenous variable"));
    assert!(analyze(&parse(&quiet)).iter().any(|row| row.code == "E465"));
    assert_eq!(duplicate_codes(&retyped).len(), 1);
}

#[test]
fn macro_order_and_written_include_first_locations_reach_mcp() {
    let root = "C:/hetero-shock-links/root.mod";
    let child = "C:/hetero-shock-links/first.inc";
    let text = format!(
        "{PREFIX}shocks(heterogeneity=d);\n@#include \"first.inc\"\n/* 🧮 */ var eh=.2;\nend;"
    );
    let files = HashMap::from([
        (root.to_string(), text.clone()),
        (child.to_string(), "/* 🚀 */ var eh=.1;\r\n".to_string()),
    ]);
    let rows = dynare_diagnose(&text, Some(root), Some(&files));
    let row = rows.iter().find(|row| row.code == "E111").unwrap();
    assert_eq!(row.file, None); // The active file is implicit in MCP.
    assert_eq!((row.line, row.column), (10, 9));
    assert_eq!(row.related[0]["file"], child);
    assert_eq!(row.related[0]["line"], 1);
    assert_eq!(row.related[0]["column"], 9);
    assert_eq!(row.related[0]["end_column"], 19);
    let macro_text = format!(
        "{PREFIX}shocks(heterogeneity=d);\n@#for k in [2,1]\nvar eh=@{{k}};\n@#endfor\nend;"
    );
    let rows = dynare_diagnose(&macro_text, Some(root), None);
    let row = rows.iter().find(|row| row.code == "E111").unwrap();
    assert_eq!(row.related[0]["line"], 10);
    assert_eq!(row.related[0]["column"], 1);
    pinned_check(
        &macro_text,
        Some("shocks: variance or stderr of shock on eh declared twice"),
    );
}

#[test]
fn shipped_generic_checks_reach_existing_row_variants() {
    for body in [
        "var missing=.1;",
        "var missing; stderr .1;",
        "var eh,missing=.1;",
        "corr missing,eh=.1;",
        "skew missing=.1;",
        "skew eh,missing,gh=.1;",
    ] {
        let source = format!("{PREFIX}{}", block(body));
        let rows = analyze(&parse(&source));
        assert!(
            rows.iter()
                .any(|row| row.code == "E058" && row.message == "Unknown symbol: missing."),
            "{source}: {rows:?}"
        );
        pinned_check(&source, Some("Unknown symbol: missing"));
    }
    for (body, code) in [
        ("var e=.1;", "E465"),
        ("var e; stderr .1;", "E466"),
        ("var e,eh=.1;", "E467"),
        ("corr e,eh=.1;", "E468"),
    ] {
        let source = format!("{PREFIX}{}", block(body));
        let rows = analyze(&parse(&source));
        let row = rows.iter().find(|row| row.code == code).unwrap();
        pinned_check(&source, Some(&row.message));
    }
    // Heterogeneous skew rows use the shared parse handler but are not consumed
    // by HeterogeneousShocksStatement::checkPass on the pin.
    for body in ["skew y=.1;", "skew y,eh,gh=.1;"] {
        let source = format!("{PREFIX}{}", block(body));
        assert!(analyze(&parse(&source))
            .iter()
            .all(|row| row.code != "E270"));
        pinned_check(&source, None);
    }
}

async fn open(server: &Backend, uri: &Url, text: &str, version: i32) {
    server
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "dynare".into(),
                version,
                text: text.into(),
            },
        })
        .await;
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
async fn lsp_first_sites_use_utf16_and_clear_after_row_removal() {
    let (service, _socket) = new_service();
    let server = service.inner();
    let uri = Url::parse("untitled:heterogeneous-duplicates.mod").unwrap();
    let mut version = 0;
    for (first, second, code, message) in CASES {
        let source = format!(
            "{PREFIX}{}",
            block(&format!("/* 🚀 */ {first}\n/* 🧮 */ {second}"))
        );
        version += 1;
        open(server, &uri, &source, version).await;
        let rows = pull(server, &uri).await;
        let row = rows
            .iter()
            .find(|row| row.code == Some(NumberOrString::String(code.into())))
            .unwrap();
        assert_eq!(row.message, message);
        assert_eq!(row.range.start, Position::new(9, 9));
        let related = &row.related_information.as_ref().unwrap()[0];
        assert_eq!(related.location.uri, uri);
        assert_eq!(
            related.location.range,
            Range::new(
                Position::new(8, 9),
                Position::new(8, 9 + first.encode_utf16().count() as u32)
            )
        );
        version += 1;
        server
            .did_change(DidChangeTextDocumentParams {
                text_document: VersionedTextDocumentIdentifier {
                    uri: uri.clone(),
                    version,
                },
                content_changes: vec![TextDocumentContentChangeEvent {
                    range: None,
                    range_length: None,
                    text: source.replace(second, ""),
                }],
            })
            .await;
        assert!(!pull(server, &uri)
            .await
            .iter()
            .any(|row| row.code == Some(NumberOrString::String(code.into()))));
        version += 1;
        server
            .did_change(DidChangeTextDocumentParams {
                text_document: VersionedTextDocumentIdentifier {
                    uri: uri.clone(),
                    version,
                },
                content_changes: vec![TextDocumentContentChangeEvent {
                    range: None,
                    range_length: None,
                    text: source.clone(),
                }],
            })
            .await;
        let rows = pull(server, &uri).await;
        let restored = rows
            .iter()
            .find(|row| row.code == Some(NumberOrString::String(code.into())))
            .unwrap();
        assert_eq!(restored.related_information, row.related_information);
    }
}

#[tokio::test]
async fn lsp_include_and_repeated_macro_sites_keep_written_locations() {
    let root = Url::parse("file:///C:/hetero-shock-links/root.mod").unwrap();
    let child = Url::parse("file:///C:/hetero-shock-links/first.inc").unwrap();
    let source = format!(
        "{PREFIX}shocks(heterogeneity=d);\n@#include \"first.inc\"\n/* 🧮 */ var eh=.2;\nend;"
    );
    let (service, _socket) = new_service();
    let server = service.inner();
    open(server, &child, "/* 🚀 */ var eh=.1;\r\n", 1).await;
    open(server, &root, &source, 1).await;
    let rows = pull(server, &root).await;
    let row = rows
        .iter()
        .find(|row| row.code == Some(NumberOrString::String("E111".into())))
        .unwrap();
    assert_eq!(row.range.start, Position::new(9, 9));
    let related = &row.related_information.as_ref().unwrap()[0];
    assert_eq!(related.location.uri, child);
    assert_eq!(
        related.location.range,
        Range::new(Position::new(0, 9), Position::new(0, 19))
    );
    let source = format!(
        "{PREFIX}shocks(heterogeneity=d);\n@#for k in [2,1]\nvar eh=@{{k}};\n@#endfor\nend;"
    );
    open(server, &root, &source, 2).await;
    let rows = pull(server, &root).await;
    let row = rows
        .iter()
        .find(|row| row.code == Some(NumberOrString::String("E111".into())))
        .unwrap();
    assert_eq!(row.range.start, Position::new(9, 0));
    let related = &row.related_information.as_ref().unwrap()[0];
    assert_eq!(related.location.uri, root);
    assert_eq!(related.location.range.start, Position::new(9, 0));
}

#[test]
fn ordinary_blocks_check_retained_skew_types_at_the_written_row() {
    for (body, name, message) in [
        ("shocks(heterogeneity=d); skew eh=.1; end; shocks; var e=.1; end;", "skew eh=.1;", "shocks: setting skewness for 'eh', 'eh', 'eh' is not allowed; skewness can only be specified for exogenous variables"),
        ("shocks(heterogeneity=d); skew eh,fh,gh=.1; end; shocks; var e=.1; end;", "skew eh,fh,gh=.1;", "shocks: setting skewness for 'eh', 'fh', 'gh' is not allowed; skewness can only be specified for exogenous variables"),
        ("shocks; skew eh=.1; end;", "skew eh=.1;", "shocks: setting skewness for 'eh', 'eh', 'eh' is not allowed; skewness can only be specified for exogenous variables"),
    ] {
        let source = format!("{PREFIX}{body}");
        let rows = analyze(&parse(&source));
        let row = rows.iter().find(|row| row.code == "E270").unwrap_or_else(|| panic!("{source}: {rows:?}"));
        assert_eq!(row.message, message);
        assert_eq!(row.span.start as usize, source.find(name).unwrap());
        assert_eq!(row.span.end as usize, source.find(name).unwrap() + name.len());
        pinned_check(&source, Some(message));
    }
    let source = format!(
        "{}shocks(heterogeneity=d); skew e=.1; end; shocks(overwrite); end;",
        PREFIX.replace("varexo e;", "varexo e; change_type(parameters) e;")
    );
    let rows = analyze(&parse(&source));
    let row = rows.iter().find(|row| row.code == "E270").unwrap();
    pinned_check(&source, Some(&row.message));
    for body in [
        "shocks(heterogeneity=d); skew eh=.1; end;",
        "shocks(heterogeneity=d); skew e=.1; end; shocks; var e=.1; end;",
        "shocks(heterogeneity=d); skew eh=.1; end; change_type(varexo) eh; shocks; var e=.1; end;",
    ] {
        let source = format!("{PREFIX}{body}");
        assert!(analyze(&parse(&source))
            .iter()
            .all(|row| row.code != "E270"));
        pinned_check(&source, None);
    }
    let source = format!(
        "{}shocks; var e=.1; end;",
        PREFIX
            .replace("varexo e;", "varexo e; trend_var(growth_factor=1) t; shocks(heterogeneity=d); skew t=.1; end; change_type(varexo) t;")
            .replace("y=SUM(a)+e;", "y=SUM(a)+e+t;")
    );
    assert!(analyze(&parse(&source))
        .iter()
        .all(|row| row.code != "E270"));
    pinned_check(&source, None);
}

#[tokio::test]
async fn retained_skew_type_error_maps_to_its_included_row_in_both_transports() {
    let root = Url::parse("file:///C:/hetero-skew-types/root.mod").unwrap();
    let child = Url::parse("file:///C:/hetero-skew-types/first.inc").unwrap();
    let source = format!(
        "{PREFIX}shocks(heterogeneity=d);\n@#include \"first.inc\"\nend;\nshocks; var e=.1; end;"
    );
    let child_text = "/* 🚀 */ skew eh=.1;\r\n";
    let (service, _socket) = new_service();
    let server = service.inner();
    open(server, &child, child_text, 1).await;
    open(server, &root, &source, 1).await;
    let rows = pull(server, &child).await;
    let row = rows
        .iter()
        .find(|row| row.code == Some(NumberOrString::String("E270".into())))
        .unwrap();
    assert_eq!(
        row.range,
        Range::new(Position::new(0, 9), Position::new(0, 20))
    );
    let root_path = root
        .to_file_path()
        .unwrap()
        .to_string_lossy()
        .replace('\\', "/");
    let child_path = child
        .to_file_path()
        .unwrap()
        .to_string_lossy()
        .replace('\\', "/");
    let files = HashMap::from([
        (root_path.clone(), source.clone()),
        (child_path.clone(), child_text.into()),
    ]);
    let rows = dynare_diagnose(&source, Some(&root_path), Some(&files));
    let mcp = rows.iter().find(|row| row.code == "E270").unwrap();
    assert_eq!(mcp.message, row.message);
    assert_eq!(mcp.file.as_deref(), Some(child_path.as_str()));
    assert_eq!(
        (mcp.line, mcp.column, mcp.end_line, mcp.end_column),
        (1, 9, 1, 20)
    );
    open(
        server,
        &root,
        &source.replace("shocks; var e=.1; end;", ""),
        2,
    )
    .await;
    assert!(!pull(server, &child)
        .await
        .iter()
        .any(|row| row.code == Some(NumberOrString::String("E270".into()))));
    open(server, &root, &source, 3).await;
    assert!(pull(server, &child)
        .await
        .iter()
        .any(|row| row.code == Some(NumberOrString::String("E270".into()))));
}

#[test]
fn review_learnt_in_one_consumes_and_resets_retained_skewness() {
    let source = format!("{PREFIX}shocks(heterogeneity=d); skew e=.1; end; shocks(learnt_in=1); var e; periods 1; values .1; end; shocks(heterogeneity=d); skew e=.2; end;");
    pinned_check(&source, None);
    assert!(
        duplicate_codes(&source).is_empty(),
        "{source}: {:?}",
        duplicate_codes(&source)
    );
    let source = format!("{PREFIX}shocks(heterogeneity=d); skew eh=.1; end; shocks(learnt_in=1); var e; periods 1; values .1; end;");
    let message = "shocks: setting skewness for 'eh', 'eh', 'eh' is not allowed; skewness can only be specified for exogenous variables";
    pinned_check(&source, Some(message));
    let rows = analyze(&parse(&source));
    assert!(
        rows.iter()
            .any(|row| row.code == "E270" && row.message == message),
        "{source}: {rows:?}"
    );
}

#[test]
fn review_whole_block_macro_copies_own_only_their_execution_rows() {
    for body in [
        "var e=@{k};",
        "var e; stderr @{k};",
        "var e,f=@{k};",
        "corr e,f=@{k}/10;",
        "skew e=@{k};",
        "skew e,f,g=@{k};",
    ] {
        let source = format!("var y; varexo e f g; model; y=e+f+g; end;\n@#for k in 1:2\nshocks;\n{body}\nend;\n@#endfor\n");
        pinned_check(&source, None);
        let model = parse(&source);
        assert_eq!(model.shock_blocks.len(), 2);
        assert!(
            model
                .shock_blocks
                .iter()
                .all(|block| block.stochastic.len() == 1),
            "{source}: {:?}",
            model.shock_blocks
        );
        for (index, block) in model.shock_blocks.iter().enumerate() {
            assert_eq!(
                block.stochastic[0].rhs_expr,
                model.shock_stmts[index].rhs_expr
            );
        }
        assert!(
            duplicate_codes(&source).is_empty(),
            "{source}: {:?}",
            duplicate_codes(&source)
        );
    }
    let source = format!(
        "{PREFIX}@#for k in 1:2\nshocks(heterogeneity=d);\nvar eh=@{{k}};\nend;\n@#endfor\n"
    );
    pinned_check(&source, None);
    let model = parse(&source);
    assert!(model
        .shock_blocks
        .iter()
        .all(|block| block.stochastic.len() == 1));
    assert!(duplicate_codes(&source).is_empty());
    let source = source.replace("var eh", "skew eh");
    pinned_check(&source, Some("shocks: skewness of eh declared twice"));
    assert_eq!(duplicate_codes(&source)[0].code, "E393");
}

#[test]
fn review_other_shock_boundaries_keep_retained_skewness() {
    for boundary in [
        "shocks(learnt_in=2); var e; periods 2; values .1; end;",
        "shocks(learnt_in=2000Q1); var e; periods 2000Q1; values .1; end;",
        "shocks(surprise); var e; periods 1; values .1; end;",
        "mshocks; var e; periods 1; values 1.1; end;",
        "mshocks(learnt_in=1); var e; periods 1; values 1.1; end;",
        "mshocks(learnt_in=2); var e; periods 2; values 1.1; end;",
    ] {
        let source = format!("{PREFIX}shocks(heterogeneity=d); skew e=.1; end; {boundary} shocks(heterogeneity=d); skew e=.2; end;");
        let message = "shocks: skewness of e declared twice";
        pinned_check(&source, Some(message));
        assert_eq!(duplicate_codes(&source)[0].message, message);
        let source = format!("{PREFIX}shocks(heterogeneity=d); skew eh=.1; end; {boundary}");
        pinned_check(&source, None);
        assert!(analyze(&parse(&source))
            .iter()
            .all(|row| row.code != "E270"));
    }
}

#[tokio::test]
async fn review_boundary_and_whole_macro_ranges_agree_in_both_transports() {
    let (service, _socket) = new_service();
    let server = service.inner();
    let uri = Url::parse("untitled:review-shock-boundaries.mod").unwrap();
    let source = format!("{PREFIX}shocks(heterogeneity=d);\n/* 🚀 */ skew eh=.1;\nend;\nshocks(learnt_in=1); var e; periods 1; values .1; end;\n");
    open(server, &uri, &source, 1).await;
    let rows = pull(server, &uri).await;
    let row = rows
        .iter()
        .find(|row| row.code == Some(NumberOrString::String("E270".into())))
        .unwrap();
    assert_eq!(
        row.range,
        Range::new(Position::new(8, 9), Position::new(8, 20))
    );
    let mcp = dynare_diagnose(&source, None, None);
    let mcp = mcp.iter().find(|row| row.code == "E270").unwrap();
    assert_eq!(mcp.message, row.message);
    assert_eq!(
        (mcp.line, mcp.column, mcp.end_line, mcp.end_column),
        (9, 9, 9, 20)
    );
    let quiet =
        source.replace("skew eh=.1;", "skew e=.1;") + "shocks(heterogeneity=d); skew e=.2; end;";
    open(server, &uri, &quiet, 2).await;
    assert!(!pull(server, &uri).await.iter().any(|row| matches!(&row.code, Some(NumberOrString::String(code)) if code == "E393" || code == "E270")));
    assert!(dynare_diagnose(&quiet, None, None)
        .iter()
        .all(|row| row.code != "E393" && row.code != "E270"));
    for (index, body) in ["var e=@{k};", "corr e,f=@{k}/10;", "skew e=@{k};"]
        .iter()
        .enumerate()
    {
        let quiet = format!("var y; varexo e f g; model; y=e+f+g; end;\n@#for k in 1:2\nshocks;\n/* 🚀 */ {body}\nend;\n@#endfor\n");
        open(server, &uri, &quiet, 3 + index as i32 * 2).await;
        assert!(!pull(server, &uri).await.iter().any(|row| matches!(&row.code, Some(NumberOrString::String(code)) if code == "E111" || code == "E393")));
        assert!(dynare_diagnose(&quiet, None, None)
            .iter()
            .all(|row| row.code != "E111" && row.code != "E393"));
        let fire = quiet.replace("\nend;", &format!("\n/* 🧮 */ {body}\nend;"));
        open(server, &uri, &fire, 4 + index as i32 * 2).await;
        let rows = pull(server, &uri).await;
        let code = if body.starts_with("skew") {
            "E393"
        } else {
            "E111"
        };
        let row = rows
            .iter()
            .find(|row| row.code == Some(NumberOrString::String(code.into())))
            .unwrap();
        assert_eq!(row.range.start, Position::new(4, 9));
        let related = &row.related_information.as_ref().unwrap()[0];
        assert_eq!(related.location.uri, uri);
        assert_eq!(related.location.range.start, Position::new(3, 9));
        let mcp = dynare_diagnose(&fire, None, None);
        let mcp = mcp.iter().find(|row| row.code == code).unwrap();
        assert_eq!((mcp.line, mcp.column), (5, 9));
        assert_eq!(mcp.related[0]["line"], 4);
        assert_eq!(mcp.related[0]["column"], 9);
        pinned_check(&fire, Some(&row.message));
    }
}
