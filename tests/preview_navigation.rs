use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use dygnosis::expand::expand_report;
use dygnosis::mcp::dynare_expand;
use dygnosis::server::{new_service, Backend};
use dygnosis::span::{LineIndex, Position, Span};
use serde_json::{json, Value};
use tower_lsp::lsp_types::*;
use tower_lsp::LanguageServer;

fn byte_slice(text: &str, span: Span) -> &str {
    &text[span.start as usize..span.end as usize]
}

fn json_slice<'a>(text: &'a str, range: &Value, lsp: bool) -> &'a str {
    // Normalize lone CR without changing byte offsets or doubling CRLF lines.
    let normalized: String = text
        .char_indices()
        .map(|(index, ch)| {
            if ch == '\r' && text.as_bytes().get(index + 1) != Some(&b'\n') {
                '\n'
            } else {
                ch
            }
        })
        .collect();
    let index = LineIndex::new(&normalized);
    let position = |end: bool| {
        if lsp {
            let point = &range[if end { "end" } else { "start" }];
            Position {
                line: point["line"].as_u64().unwrap() as u32,
                character: point["character"].as_u64().unwrap() as u32,
            }
        } else {
            Position {
                line: range[if end { "end_line" } else { "line" }]
                    .as_u64()
                    .unwrap() as u32
                    - 1,
                character: range[if end { "end_column" } else { "column" }]
                    .as_u64()
                    .unwrap() as u32
                    - 1,
            }
        }
    };
    let offset = |end| {
        if lsp {
            index.offset_utf16(&normalized, position(end))
        } else {
            index.offset(&normalized, position(end))
        }
    };
    &text[offset(false) as usize..offset(true) as usize]
}

#[test]
fn emitted_ranges_keep_all_scopes_locals_static_rows_and_utf16_text() {
    let source = "heterogeneity_dimension d;\r\nvar y; var(heterogeneity=d) a;\rmodel;\n#k=1;\n[static] y=k;\n[dynamic,name='😀'] y=y(-1);\nend;\nmodel(heterogeneity=d); #h=2; [static] a=h; [dynamic] a=a(-1); end;\n";
    let report = expand_report(source);
    assert!(report.complete && report.model_map.complete);
    assert_eq!(report.n_equations, 2);
    assert_eq!(report.navigation.len(), 6);
    let texts: Vec<_> = report
        .navigation
        .iter()
        .map(|row| byte_slice(&report.effective_text, row.effective_span))
        .collect();
    assert_eq!(
        texts,
        [
            "#k = 1",
            "[static] y = k",
            "[dynamic, name = '😀'] y = y(-1)",
            "#h = 2",
            "[static] a = h",
            "[dynamic] a = a(-1)"
        ]
    );
    assert!(report
        .navigation
        .windows(2)
        .all(|rows| rows[0].effective_span.end < rows[1].effective_span.start));
    assert_eq!(
        report
            .navigation
            .iter()
            .filter(|row| row.equation.number.is_some())
            .count(),
        2
    );
    assert_eq!(report.navigation[2].equation.number, Some(1));
    assert_eq!(report.navigation[5].equation.number, Some(1));
    assert_eq!(
        report.navigation[5].equation.dimension.as_deref(),
        Some("d")
    );
    let result = dynare_expand(source, None, None);
    for (row, expected) in result["navigation"].as_array().unwrap().iter().zip(texts) {
        assert_eq!(
            json_slice(
                result["effective_text"].as_str().unwrap(),
                &row["effective_range"],
                false
            ),
            expected
        );
        assert!(!row["written_locations"].as_array().unwrap().is_empty());
    }
}

#[test]
fn repeated_nested_macro_copies_keep_distinct_ranges_and_directive_sites() {
    let source = "var y; model;\n@#for i in 1:2\n@#if i > 0\n@#for j in 1:2\ny=@{i}+@{j};\n@#endfor\n@#endif\n@#endfor\nend;\n";
    let report = expand_report(source);
    assert_eq!(report.navigation.len(), 4);
    let rows = &report.navigation;
    assert!(rows
        .windows(2)
        .all(|pair| pair[0].equation.id != pair[1].equation.id
            && pair[0].effective_span != pair[1].effective_span));
    assert!(rows
        .iter()
        .all(|row| row.equation.source.segments == rows[0].equation.source.segments));
    for row in rows {
        assert_eq!(
            row.macro_frames
                .iter()
                .map(|frame| frame.kind.as_str())
                .collect::<Vec<_>>(),
            ["for", "if", "for"]
        );
        assert!(
            byte_slice(source, row.macro_frames[0].directive_segments[0].span)
                .starts_with("@#for i")
        );
        assert!(
            byte_slice(source, row.macro_frames[1].directive_segments[0].span)
                .starts_with("@#if i")
        );
        assert!(
            byte_slice(source, row.macro_frames[2].directive_segments[0].span)
                .starts_with("@#for j")
        );
        assert_eq!(
            byte_slice(source, row.equation.source.segments[0].span),
            "y=@{i}+@{j}"
        );
    }
    assert_eq!(
        rows.iter()
            .map(|row| row.macro_frames[0].value.as_deref().unwrap())
            .collect::<Vec<_>>(),
        ["1", "1", "2", "2"]
    );
    assert_eq!(
        rows.iter()
            .map(|row| row.macro_frames[2].value.as_deref().unwrap())
            .collect::<Vec<_>>(),
        ["1", "2", "1", "2"]
    );
}

#[test]
fn removed_replacement_rows_keep_emitted_order_and_surviving_numbers() {
    let source = "var y z; model; #a=1; [name='old'] y=a; [static] z=0; [dynamic,name='keep'] z=z(-1); end; model_replace('old'); [name='new'] y=2; end;";
    let report = expand_report(source);
    assert_eq!(report.navigation.len(), 5);
    assert_eq!(report.n_equations, 2);
    let old = &report.navigation[1];
    assert!(!old.equation.active);
    assert_eq!(old.equation.number, None);
    assert_eq!(
        byte_slice(&report.effective_text, old.effective_span),
        "[name = 'old'] y = a"
    );
    assert_eq!(report.navigation[3].equation.number, Some(1));
    assert_eq!(report.navigation[4].equation.number, Some(2));
    assert_ne!(
        report.navigation[3].equation.statement_id,
        report.navigation[4].equation.statement_id
    );
}

#[test]
fn partial_row_macro_copies_and_taken_branches_keep_their_context() {
    let source = "var y; model; y=0\n@#for i in 1:2\n+@{i}\n@#endfor\n;\n@#if 0\ny=1;\n@#elseif 1\ny=2;\n@#else\ny=3;\n@#endif\nend;";
    let report = expand_report(source);
    assert_eq!(report.navigation.len(), 2);
    let partial = &report.navigation[0];
    assert_eq!(
        byte_slice(&report.effective_text, partial.effective_span),
        "y = 0+1+2"
    );
    assert_eq!(
        partial
            .macro_frames
            .iter()
            .map(|frame| frame.value.as_deref().unwrap())
            .collect::<Vec<_>>(),
        ["1", "2"]
    );
    let branch = &report.navigation[1].macro_frames[0];
    assert_eq!(branch.kind, "elseif");
    assert!(byte_slice(source, branch.directive_segments[0].span).starts_with("@#elseif"));
    assert!(
        report.origins[0].origin_frames.is_empty(),
        "legacy partial-row origins keep their meaning"
    );
}

#[test]
fn cross_file_rows_and_macro_bodies_are_clipped_into_verified_targets() {
    let source = "var y; model;\n@#for i in 1:2\ny=\n@#include \"rhs.inc\"\n;\n@#endfor\nend;";
    let files = HashMap::from([
        ("root.mod".to_string(), source.to_string()),
        ("rhs.inc".to_string(), "@{i}\n".to_string()),
    ]);
    let result = dynare_expand(source, Some("root.mod"), Some(&files));
    assert_eq!(result["complete"], true, "{result}");
    for (index, row) in result["navigation"].as_array().unwrap().iter().enumerate() {
        assert_eq!(
            json_slice(
                result["effective_text"].as_str().unwrap(),
                &row["effective_range"],
                false
            ),
            format!("y = {}", index + 1)
        );
        let locations = row["written_locations"].as_array().unwrap();
        assert_eq!(locations.len(), 2);
        let snippets: Vec<_> = locations
            .iter()
            .map(|location| {
                let file = location["file"].as_str().unwrap();
                json_slice(&files[file], &location["range"], false)
            })
            .collect();
        assert_eq!(snippets, ["y=", "@{i}"]);
        let frame = &row["macro_frames"][0];
        assert_eq!(frame["value"], (index + 1).to_string());
        assert_eq!(frame["directive_locations"][0]["file"], "root.mod");
        assert!(frame["body_locations"].as_array().unwrap().len() >= 2);
        for target in frame["body_locations"].as_array().unwrap() {
            assert!(!json_slice(
                &files[target["file"].as_str().unwrap()],
                &target["range"],
                false
            )
            .is_empty());
        }
    }
}

#[test]
fn incomplete_macro_parse_and_missing_include_withhold_navigation() {
    for source in [
        "var y; model; y=@{missing}; end;",
        "var y; model; y=1;",
        "@#include \"missing.inc\"\nvar y; model; y=1; end;",
    ] {
        let result = dynare_expand(source, None, None);
        assert_eq!(result["complete"], false, "{result}");
        assert_eq!(result["status"], "incomplete");
        assert_eq!(result["navigation"], json!([]));
    }
}

#[tokio::test]
async fn missing_macro_terminators_withhold_only_new_navigation_in_both_transports() {
    let base = "var y; model; y=0; end;\n";
    let (service, _socket) = new_service();
    let backend = service.inner();
    let root = Url::parse("file:///C:/dygnosis-preview/termination.mod").unwrap();
    for (open_suffix, close_suffix) in [
        ("@#if 1\n", "@#endif\n"),
        ("@#if 0\n", "@#endif\n"),
        ("@#ifdef absent\n", "@#endif\n"),
        ("@#ifndef absent\n", "@#endif\n"),
        ("@#for i in 1:1\n", "@#endfor\n"),
        ("@#for i in []\n", "@#endfor\n"),
        ("@#if 0\n@#for i in 1:1\n", "@#endfor\n@#endif\n"),
        ("@#for i in []\n@#if 1\n", "@#endif\n@#endfor\n"),
    ] {
        let unclosed = format!("{base}{open_suffix}");
        let closed = format!("{unclosed}{close_suffix}");
        let unclosed_report = expand_report(&unclosed);
        let closed_report = expand_report(&closed);
        // Termination is a new navigation proof, not a silent change to legacy expansion.
        assert!(unclosed_report.complete && closed_report.complete);
        assert!(!unclosed_report.navigation_complete, "{unclosed}");
        assert!(closed_report.navigation_complete, "{closed}");
        assert_eq!(unclosed_report.effective_text, closed_report.effective_text);
        assert_eq!(unclosed_report.origins, closed_report.origins);
        assert_eq!(unclosed_report.n_equations, closed_report.n_equations);
        assert_eq!(unclosed_report.model_map, closed_report.model_map);
        assert!(!dygnosis::check_e062(&dygnosis::parse(&unclosed)).is_empty());
        assert!(dygnosis::check_e062(&dygnosis::parse(&closed)).is_empty());
        for (text, expected_complete) in [(&unclosed, false), (&closed, true)] {
            let mcp = dynare_expand(text, None, None);
            open(backend, &root, text, 1).await;
            let lsp = preview(backend, &root).await;
            for result in [&mcp, &lsp] {
                assert_eq!(result["complete"], expected_complete, "{text}: {result}");
                assert_eq!(
                    result["navigation"].as_array().unwrap().len(),
                    usize::from(expected_complete)
                );
                if !expected_complete {
                    assert_eq!(result["status"], "incomplete");
                }
            }
            assert_eq!(mcp["n_equations"], 1);
            assert_eq!(mcp["origins"].as_array().unwrap().len(), 1);
            assert_eq!(lsp["origins"].as_array().unwrap().len(), 1);
            assert_eq!(mcp["effective_text"], lsp["effective_text"]);
        }
    }
    // Actual model rows inside an unfinished active block also lose navigation.
    for (opener, closer) in [("@#if 1", "@#endif"), ("@#for i in 1:1", "@#endfor")] {
        let unclosed = format!("{opener}\n{base}");
        let closed = format!("{unclosed}{closer}\n");
        for (text, expected_complete) in [(&unclosed, false), (&closed, true)] {
            let report = expand_report(text);
            assert!(report.complete);
            assert_eq!(report.n_equations, 1);
            assert_eq!(report.navigation_complete, expected_complete);
            let mcp = dynare_expand(text, None, None);
            open(backend, &root, text, 2).await;
            let lsp = preview(backend, &root).await;
            for result in [&mcp, &lsp] {
                assert_eq!(result["complete"], expected_complete, "{text}: {result}");
                assert_eq!(
                    result["navigation"].as_array().unwrap().len(),
                    usize::from(expected_complete)
                );
                assert_eq!(result["origins"].as_array().unwrap().len(), 1);
            }
        }
    }
}

#[tokio::test]
async fn included_closers_cannot_make_an_unterminated_written_file_navigable() {
    let root = Url::parse("file:///C:/dygnosis-preview/split-block.mod").unwrap();
    let include = Url::parse("file:///C:/dygnosis-preview/split-block.inc").unwrap();
    let (service, _socket) = new_service();
    let backend = service.inner();
    for (opener, closer) in [("@#if 1", "@#endif"), ("@#for i in 1:1", "@#endfor")] {
        let source = format!("{opener}\nvar y; model; y=0; end;\n@#include \"split-block.inc\"\n");
        let body = format!("{closer}\n");
        let files = HashMap::from([
            (root.as_str().to_string(), source.clone()),
            (include.as_str().to_string(), body.clone()),
        ]);
        let mcp = dynare_expand(&source, Some(root.as_str()), Some(&files));
        open(backend, &root, &source, 1).await;
        open(backend, &include, &body, 1).await;
        let lsp = preview(backend, &root).await;
        for result in [&mcp, &lsp] {
            assert_eq!(result["complete"], false, "{result}");
            assert_eq!(result["navigation"], json!([]));
            assert_eq!(result["origins"].as_array().unwrap().len(), 1);
        }
        assert_eq!(mcp["n_equations"], 1);
        let valid_source = "var y; model;\n@#include \"split-block.inc\"\nend;";
        let valid_body = format!("{opener}\ny=0;\n{closer}\n");
        let files = HashMap::from([
            (root.as_str().to_string(), valid_source.to_string()),
            (include.as_str().to_string(), valid_body.clone()),
        ]);
        let mcp = dynare_expand(valid_source, Some(root.as_str()), Some(&files));
        open(backend, &root, valid_source, 2).await;
        open(backend, &include, &valid_body, 2).await;
        let lsp = preview(backend, &root).await;
        for result in [&mcp, &lsp] {
            assert_eq!(result["complete"], true, "{result}");
            assert_eq!(result["navigation"].as_array().unwrap().len(), 1);
        }
    }
}

#[tokio::test]
async fn branch_structure_proof_agrees_in_free_mapped_and_lsp_previews() {
    let root = Url::parse("file:///C:/dygnosis-preview/branches.mod").unwrap();
    let (service, _socket) = new_service();
    let backend = service.inner();
    for (branches, expected_complete) in [
        ("@#else\n@#else\n", false),
        ("@#else\n@#elseif 1\n", false),
        ("@#else\n", true),
        ("@#elseif 0\n@#else\n", true),
        ("", true),
    ] {
        let source = format!("@#if 1\nvar y; model; y=0; end;\n{branches}@#endif\n");
        let report = expand_report(&source);
        assert!(report.complete && report.model_map.complete);
        assert_eq!(report.n_equations, 1);
        assert_eq!(report.navigation_complete, expected_complete);
        assert_eq!(
            dygnosis::check_e062(&dygnosis::parse(&source)).is_empty(),
            expected_complete
        );
        let free = dynare_expand(&source, None, None);
        let files = HashMap::from([(root.as_str().to_string(), source.clone())]);
        let mapped = dynare_expand(&source, Some(root.as_str()), Some(&files));
        open(backend, &root, &source, 1).await;
        let lsp = preview(backend, &root).await;
        assert_eq!(free["effective_text"], mapped["effective_text"]);
        assert_eq!(free["effective_text"], lsp["effective_text"]);
        assert_eq!(free["n_equations"], mapped["n_equations"]);
        for result in [&free, &mapped, &lsp] {
            assert_eq!(result["complete"], expected_complete, "{source}: {result}");
            assert_eq!(
                result["navigation"].as_array().unwrap().len(),
                usize::from(expected_complete)
            );
            assert_eq!(result["origins"].as_array().unwrap().len(), 1);
            if !expected_complete {
                assert_eq!(result["status"], "incomplete");
            }
        }
    }
}

#[tokio::test]
async fn collected_macro_errors_gate_navigation_in_every_input_mode() {
    let root = Url::parse("file:///C:/dygnosis-preview/tuple-arity.mod").unwrap();
    let (service, _socket) = new_service();
    let backend = service.inner();
    for (directive, expected_complete) in [
        ("@#for(i) in [(1,2)]", false),
        ("@#for(i,j) in [(1,2)]", true),
        ("@#for i in [1,2]", true),
    ] {
        let source = format!("var y; model; y=0; end;\n{directive}\n@#endfor\n");
        // check_for_tuple records E284 before unroll_for. A one-name loop can
        // still unroll its empty body, so the old incomplete flag stays false.
        let model = dygnosis::parse(&source);
        assert_eq!(
            model
                .macro_type_errors
                .iter()
                .any(|(_, code, _)| *code == "E284"),
            !expected_complete
        );
        assert!(model.macro_incomplete_span.is_none());
        let report = expand_report(&source);
        assert!(report.complete && report.model_map.complete);
        assert_eq!(report.n_equations, 1);
        assert_eq!(report.origins.len(), 1);
        assert_eq!(report.navigation_complete, expected_complete);
        let free = dynare_expand(&source, None, None);
        let files = HashMap::from([(root.as_str().to_string(), source.clone())]);
        let mapped = dynare_expand(&source, Some(root.as_str()), Some(&files));
        open(backend, &root, &source, 1).await;
        let lsp = preview(backend, &root).await;
        for result in [&free, &mapped, &lsp] {
            assert_eq!(
                result["complete"], expected_complete,
                "{directive}: {result}"
            );
            assert_eq!(
                result["navigation"].as_array().unwrap().len(),
                usize::from(expected_complete)
            );
            assert_eq!(result["origins"].as_array().unwrap().len(), 1);
        }
        assert_eq!(free["effective_text"], mapped["effective_text"]);
        assert_eq!(free["effective_text"], lsp["effective_text"]);
        assert_eq!(free["n_equations"], mapped["n_equations"]);
    }
}

#[tokio::test]
async fn dormant_include_contents_do_not_refuse_navigation_or_supply_targets() {
    let root = Url::parse("file:///C:/dygnosis-preview/activity.mod").unwrap();
    let bad = Url::parse("file:///C:/dygnosis-preview/bad.inc").unwrap();
    let definitions = Url::parse("file:///C:/dygnosis-preview/defs.inc").unwrap();
    let (service, _socket) = new_service();
    let backend = service.inner();
    let base = "var y; model; y=0; end;\n";
    for (source, defs, body, expected_complete) in [
        (
            format!("{base}@#if 0\n@#include \"bad.inc\"\n@#endif\n"),
            "",
            "@#if 1\n",
            true,
        ),
        (
            format!("@#if 1\n{base}@#if 0\n@#include \"bad.inc\"\n@#endif\n@#endif\n"),
            "",
            "@#if 1\n",
            true,
        ),
        (
            format!("{base}@#if 1\n@#include \"bad.inc\"\n@#endif\n"),
            "",
            "@#if 1\n",
            false,
        ),
        (
            format!("{base}@#for i in []\n@#include \"bad.inc\"\n@#endfor\n"),
            "",
            "@#if 1\n",
            true,
        ),
        (
            format!("{base}@#include \"defs.inc\"\n@#if enabled\n@#include \"bad.inc\"\n@#endif\n"),
            "@#define enabled=0\n",
            "@#if 1\n",
            true,
        ),
        (
            format!("{base}@#include \"defs.inc\"\n@#if enabled\n@#include \"bad.inc\"\n@#endif\n"),
            "@#define enabled=1\n",
            "@#if 1\n",
            false,
        ),
        (
            format!("{base}@#include \"defs.inc\"\n@#if enabled\n@#include \"bad.inc\"\n@#endif\n"),
            "@#define enabled=length([1,2])\n",
            "@#if 1\n",
            false,
        ),
        (
            format!("{base}@#include \"defs.inc\"\n"),
            "",
            "@#if 1\n",
            true,
        ),
    ] {
        let files = HashMap::from([
            (root.as_str().to_string(), source.clone()),
            (bad.as_str().to_string(), body.to_string()),
            (definitions.as_str().to_string(), defs.to_string()),
        ]);
        let mcp = dynare_expand(&source, Some(root.as_str()), Some(&files));
        open(backend, &root, &source, 1).await;
        open(backend, &bad, body, 1).await;
        open(backend, &definitions, defs, 1).await;
        let lsp = preview(backend, &root).await;
        for result in [&mcp, &lsp] {
            assert_eq!(result["complete"], expected_complete, "{source}: {result}");
            if expected_complete {
                assert_eq!(result["navigation"].as_array().unwrap().len(), 1);
            } else {
                assert_eq!(result["navigation"], json!([]));
            }
        }
        assert_eq!(mcp["effective_text"], lsp["effective_text"]);
        if expected_complete {
            assert_eq!(mcp["n_equations"], 1);
            assert_eq!(mcp["origins"].as_array().unwrap().len(), 1);
            assert_eq!(
                lsp["navigation"][0]["written_locations"][0]["uri"],
                root.as_str()
            );
            for frame in lsp["navigation"][0]["macro_frames"].as_array().unwrap() {
                for location in frame["directive_locations"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .chain(frame["body_locations"].as_array().unwrap())
                {
                    assert_eq!(location["uri"], root.as_str());
                }
            }
        }
    }
    // A dormant malformed file can corrupt the legacy splice's branch stack.
    // Keep that legacy text, but never navigate rows emitted only by that fault.
    let source = format!("{base}@#if 0\n@#include \"bad.inc\"\n@#endif\n");
    let body = "@#else\nvar z; model; z=1; end;\n";
    let files = HashMap::from([
        (root.as_str().to_string(), source.clone()),
        (bad.as_str().to_string(), body.to_string()),
    ]);
    let mcp = dynare_expand(&source, Some(root.as_str()), Some(&files));
    open(backend, &root, &source, 1).await;
    open(backend, &bad, body, 1).await;
    let lsp = preview(backend, &root).await;
    for result in [&mcp, &lsp] {
        assert!(result["effective_text"].as_str().unwrap().contains("z = 1"));
        assert_eq!(result["complete"], false);
        assert_eq!(result["navigation"], json!([]));
    }
}

#[tokio::test]
async fn included_definitions_and_nested_active_blocks_keep_verified_macro_context() {
    let root = Url::parse("file:///C:/dygnosis-preview/definitions.mod").unwrap();
    let definitions = Url::parse("file:///C:/dygnosis-preview/definitions.inc").unwrap();
    let leaf = Url::parse("file:///C:/dygnosis-preview/leaf.inc").unwrap();
    let body = Url::parse("file:///C:/dygnosis-preview/equations.inc").unwrap();
    let source = "var y;\n@#include \"definitions.inc\"\nmodel;\n@#if enabled\n@#include \"equations.inc\"\n@#endif\nend;";
    let files = HashMap::from([
        (root.as_str().to_string(), source.to_string()),
        (
            definitions.as_str().to_string(),
            "@#include \"leaf.inc\"\n".to_string(),
        ),
        (
            leaf.as_str().to_string(),
            "@#define enabled=1\n".to_string(),
        ),
        (
            body.as_str().to_string(),
            "@#for i in 1:2\ny=@{i};\n@#endfor\n".to_string(),
        ),
    ]);
    let mcp = dynare_expand(source, Some(root.as_str()), Some(&files));
    let (service, _socket) = new_service();
    let backend = service.inner();
    for (file, text) in &files {
        open(backend, &Url::parse(file).unwrap(), text, 1).await;
    }
    let lsp = preview(backend, &root).await;
    for result in [&mcp, &lsp] {
        assert_eq!(result["complete"], true, "{result}");
        assert_eq!(result["navigation"].as_array().unwrap().len(), 2);
        for (index, row) in result["navigation"].as_array().unwrap().iter().enumerate() {
            assert_eq!(row["macro_frames"][0]["kind"], "if");
            assert_eq!(row["macro_frames"][1]["kind"], "for");
            assert_eq!(row["macro_frames"][1]["value"], (index + 1).to_string());
        }
    }
    assert_eq!(mcp["n_equations"], 2);
    assert_eq!(
        lsp["navigation"][0]["written_locations"][0]["uri"],
        body.as_str()
    );
    assert_eq!(
        lsp["navigation"][0]["macro_frames"][0]["directive_locations"][0]["uri"],
        root.as_str()
    );
    assert_eq!(
        lsp["navigation"][0]["macro_frames"][1]["directive_locations"][0]["uri"],
        body.as_str()
    );
}

async fn open(backend: &Backend, uri: &Url, text: &str, version: i32) {
    backend
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

async fn preview(backend: &Backend, root: &Url) -> Value {
    backend
        .execute_command(ExecuteCommandParams {
            command: "dynare/showEffectiveModel".into(),
            arguments: vec![json!({"root_uri":root})],
            work_done_progress_params: Default::default(),
        })
        .await
        .unwrap()
        .unwrap()
}

#[tokio::test]
async fn lsp_and_mcp_agree_on_utf16_scalar_ranges_and_written_source_versions() {
    let source = "var y; model;\r\n[name='😀'] y=1;\rend;";
    let root = Url::parse("file:///C:/dygnosis-preview/root.mod").unwrap();
    let (service, _socket) = new_service();
    let backend = service.inner();
    let initialized = backend
        .initialize(InitializeParams::default())
        .await
        .unwrap();
    assert_eq!(
        initialized.capabilities.experimental.unwrap()["dygnosis"]["effectivePreview"]
            ["navigation_schema_version"],
        1
    );
    open(backend, &root, source, 7).await;
    let lsp = preview(backend, &root).await;
    let mcp = dynare_expand(source, None, None);
    assert_eq!(lsp["navigation_schema_version"], 1);
    assert_eq!(lsp["root_uri"], root.as_str());
    assert_eq!(lsp["document_version"], 7);
    assert_eq!(lsp["complete"], true);
    assert_eq!(lsp["effective_text"], mcp["effective_text"]);
    assert_eq!(lsp["navigation"][0]["id"], mcp["navigation"][0]["id"]);
    let expanded = lsp["effective_text"].as_str().unwrap();
    assert_eq!(
        json_slice(expanded, &lsp["navigation"][0]["effective_range"], true),
        json_slice(expanded, &mcp["navigation"][0]["effective_range"], false)
    );
    let lsp_end = lsp["navigation"][0]["effective_range"]["end"]["character"]
        .as_u64()
        .unwrap();
    let mcp_end = mcp["navigation"][0]["effective_range"]["end_column"]
        .as_u64()
        .unwrap();
    assert_eq!(
        lsp_end, mcp_end,
        "one extra UTF-16 unit cancels MCP's one-based column"
    );
    let target = &lsp["navigation"][0]["written_locations"][0];
    assert_eq!(target["uri"], root.as_str());
    assert_eq!(target["document_version"], 7);
    assert_eq!(target["range"]["start"]["line"], 1);
    assert_eq!(
        json_slice(source, &target["range"], true),
        "[name='😀'] y=1"
    );
}

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Files(PathBuf);
impl Files {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "dygnosis-preview-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&root).unwrap();
        Self(root)
    }
    fn write(&self, name: &str, text: &str) -> Url {
        let path = self.0.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, text).unwrap();
        Url::from_file_path(path).unwrap()
    }
}
impl Drop for Files {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[tokio::test]
async fn unopened_roots_refresh_disk_include_overlay_and_settings_revisions() {
    let files = Files::new();
    let root = files.write("root.mod", "var y; model;\n@#include \"body.inc\"\nend;");
    let include = files.write("body.inc", "y=1;");
    let (service, _socket) = new_service();
    let backend = service.inner();
    let first = preview(backend, &root).await;
    assert_eq!(first["complete"], true, "{first}");
    let candidates = first["dependency_candidates"].as_array().unwrap();
    for uri in [&root, &include] {
        assert!(candidates.iter().any(|candidate| {
            dygnosis::include_resolver::normalize_uri(candidate.as_str().unwrap())
                == dygnosis::include_resolver::normalize_uri(uri.as_str())
        }));
    }
    assert!(first["document_version"].is_null());
    assert_eq!(
        first["navigation"][0]["written_locations"][0]["uri"],
        include.as_str()
    );
    files.write("body.inc", "y=2;");
    let disk = preview(backend, &root).await;
    assert_ne!(disk["revision"], first["revision"]);
    assert!(disk["effective_text"].as_str().unwrap().contains("y = 2"));
    open(backend, &include, "y=3;", 4).await;
    let overlay = preview(backend, &root).await;
    assert_ne!(overlay["revision"], disk["revision"]);
    assert!(overlay["effective_text"]
        .as_str()
        .unwrap()
        .contains("y = 3"));
    assert_eq!(
        overlay["navigation"][0]["written_locations"][0]["document_version"],
        4
    );
    backend
        .did_change_configuration(DidChangeConfigurationParams {
            settings: json!({"dynare":{"formatIndent":4}}),
        })
        .await;
    let configured = preview(backend, &root).await;
    assert_ne!(configured["revision"], overlay["revision"]);
    assert_eq!(configured["navigation"], overlay["navigation"]);
    backend
        .did_close(DidCloseTextDocumentParams {
            text_document: TextDocumentIdentifier { uri: include },
        })
        .await;
    let closed = preview(backend, &root).await;
    assert_ne!(closed["revision"], configured["revision"]);
    assert!(closed["effective_text"].as_str().unwrap().contains("y = 2"));
}

#[tokio::test]
async fn root_ownership_virtual_targets_and_missing_includes_never_guess() {
    let root = Url::parse("file:///C:/dygnosis-preview/owner.mod").unwrap();
    let include = Url::parse("file:///C:/dygnosis-preview/shared.inc").unwrap();
    let (service, _socket) = new_service();
    let backend = service.inner();
    open(
        backend,
        &root,
        "var y; model;\n@#include \"shared.inc\"\nend;",
        1,
    )
    .await;
    open(backend, &include, "y=1;", 2).await;
    let mapped = preview(backend, &root).await;
    assert_eq!(mapped["complete"], true);
    assert_eq!(preview(backend, &include).await["code"], "ROOT_REQUIRED");
    let other = Url::parse("file:///C:/dygnosis-preview/other.dyn").unwrap();
    open(
        backend,
        &other,
        "var y; model; y=0;\n@#include \"shared.inc\"\nend;",
        1,
    )
    .await;
    let other_preview = preview(backend, &other).await;
    assert_ne!(mapped["root_uri"], other_preview["root_uri"]);
    assert_ne!(mapped["revision"], other_preview["revision"]);
    assert_eq!(
        mapped["navigation"][0]["written_locations"],
        other_preview["navigation"][1]["written_locations"]
    );
    let missing = Url::parse("file:///C:/dygnosis-preview/missing.mod").unwrap();
    open(
        backend,
        &missing,
        "var y; model; y=1;\n@#include \"absent.inc\"\nend;",
        1,
    )
    .await;
    let incomplete = preview(backend, &missing).await;
    assert_eq!(incomplete["navigation"], json!([]));
    assert!(incomplete["dependency_candidates"]
        .as_array()
        .unwrap()
        .iter()
        .any(|uri| uri.as_str().unwrap().ends_with("/absent.inc")));
    let virtual_root = Url::parse("custom-preview://test/root.mod").unwrap();
    open(backend, &virtual_root, "var y; model; y=1; end;", 1).await;
    let virtual_preview = preview(backend, &virtual_root).await;
    assert_eq!(
        virtual_preview["navigation"][0]["written_locations"],
        json!([])
    );
    let untitled = Url::parse("untitled:Unsaved.mod").unwrap();
    open(backend, &untitled, "var y; model; y=1; end;", 1).await;
    assert_eq!(
        preview(backend, &untitled).await["navigation"][0]["written_locations"][0]["uri"],
        untitled.as_str()
    );
}
