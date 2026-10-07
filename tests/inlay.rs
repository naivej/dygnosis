use dygnosis::assignment_values::assignment_values;
use dygnosis::model_info::assigned_number;
use dygnosis::parse;
use dygnosis::server::new_service;
use serde_json::json;
use tower_lsp::lsp_types::*;
use tower_lsp::LanguageServer;

fn values(text: &str) -> Vec<(String, Option<f64>, bool)> {
    let model = parse(text);
    assignment_values(&model)
        .into_iter()
        .map(|row| {
            (
                model.statements[row.statement_id].name.clone(),
                row.value,
                row.written_plain_number,
            )
        })
        .collect()
}

#[test]
fn assignments_follow_execution_and_legacy_final_readers_keep_their_meaning() {
    let declared = "parameters helper p;\nhelper=2;\np=helper+1;\nhelper=p*2;\np=helper+1;";
    let actual = values(declared);
    assert_eq!(
        actual.iter().map(|row| row.1).collect::<Vec<_>>(),
        [Some(2.0), Some(3.0), Some(6.0), Some(7.0)]
    );
    let native = "helper=2;\nparameters p;\np=helper+1;\nhelper=p*2;\np=helper+1;";
    assert!(values(native).iter().all(|row| row.1.is_none()));
    assert_eq!(assigned_number(&parse(native), "p"), None);
    assert_eq!(
        assigned_number(&parse("parameters p; p=1/(1/0);"), "p"),
        Some(0.0)
    );
    assert_eq!(values("parameters p; p=1/(1/0);")[0].1, None);
}

#[test]
fn plain_numbers_are_written_syntax_and_macro_literals_are_occurrence_values() {
    let actual = values("parameters a b c d e; a=2; b=-2; c=+2e-3; d=(2); e=--2;");
    assert_eq!(
        actual.iter().map(|row| row.2).collect::<Vec<_>>(),
        [true, true, true, false, false]
    );
    assert_eq!(actual[3].1, Some(2.0));
    let actual = values("parameters p;\n@#for k in 1:3\np=@{k}+1;\n@#endfor\n");
    assert_eq!(
        actual.iter().map(|row| row.1).collect::<Vec<_>>(),
        [Some(2.0), Some(3.0), Some(4.0)]
    );
    assert!(actual.iter().all(|row| !row.2));
    let actual = values("@#define k=2\nparameters p; p=@{k};\n");
    assert_eq!(actual[0].1, Some(2.0));
    assert!(!actual[0].2);
    assert_eq!(
        assigned_number(&parse("@#define k=2\nparameters p; p=@{k};\n"), "p"),
        None
    );
}

#[test]
fn arithmetic_is_finite_and_unknown_assignments_do_not_reuse_older_values() {
    let actual = values("parameters p q r a b c d e; p=2; q=-p+3*4/2^2; r=unknown+1; p=exp(1); a=p+1; b=1e308*1e308; c=1/0; d=(-1)^0.5; e=2+3;");
    assert_eq!(actual[1].1, Some(1.0));
    assert!(actual[2..8].iter().all(|row| row.1.is_none()));
    assert_eq!(actual[8].1, Some(5.0));
    assert_eq!(values("parameters p q; p=2; q=p(0)+1;")[1].1, None);
}

#[test]
fn skipped_native_rhs_text_and_unbalanced_parentheses_are_not_scalar_proofs() {
    for expression in ["2.*3", "2:3", "(2", "2+", "2@", "[2,3]", "2,3"] {
        let text = format!("parameters p;\np=2;\nhelper={expression};\np=p+1;");
        let actual = values(&text);
        assert!(actual[1].1.is_none(), "{expression}: {actual:?}");
        assert!(
            actual.last().unwrap().1.is_none(),
            "{expression}: {actual:?}"
        );
    }
    assert_eq!(values("helper=(2 /* comment */ +3);")[0].1, None);
}

#[test]
fn mutations_invalidate_state_and_explicit_assignments_restore_it() {
    for barrier in [
        "native_call();",
        "load_params_and_steady_state(values);",
        "steady;",
        "verbatim; p=4; end;",
    ] {
        let actual = values(&format!(
            "parameters p q; p=2; {barrier}\nq=p+1; p=3; q=p+1;"
        ));
        assert_eq!(
            actual.iter().map(|row| row.1).collect::<Vec<_>>(),
            [Some(2.0), None, Some(3.0), Some(4.0)],
            "{barrier}"
        );
    }
    let actual = values("parameters p q;\np=2;\nhelper=recalibrate();\nq=p+1;\np=3;\nq=p+1;");
    assert_eq!(actual[2].1, None);
    assert_eq!(actual[4].1, Some(4.0));
    assert_eq!(values("p=2;\nparameters p q;\nq=p+1;")[1].1, None);
}

#[test]
fn native_control_flow_never_establishes_unconditional_values_inside_a_branch() {
    for flow in [
        "if flag; p=4; q=p+1; end;",
        "for k=1:2; p=4; q=p+1; end;",
        "if flag\n for k=1:2\n p=4;\n q=p+1;\n end\n end\n",
        "@#define control=\"if flag\"\n@{control}; p=4; q=p+1; end;\n",
    ] {
        let actual = values(&format!("parameters p q; p=2;\n{flow}\nq=p+1; p=3; q=p+1;"));
        assert_eq!(actual.last().unwrap().1, Some(4.0), "{flow}: {actual:?}");
        assert!(
            actual[1..actual.len() - 2]
                .iter()
                .all(|row| row.1.is_none()),
            "{flow}: {actual:?}"
        );
    }
    let actual = values("parameters p q; if flag; p=2; q=p+1; p=3; q=p+1;");
    assert!(actual.iter().all(|row| row.1.is_none()));
}

#[test]
fn written_native_callee_shadows_survive_unknown_values_and_invalidate_later_knowledge() {
    let fresh =
        values("parameters a b c d e;\na=1;\nexp=exp(a);\nb=a+1;\nc=exp(a);\nd=a+1;\na=2;\ne=a+1;");
    assert_eq!(fresh[1].1, None);
    assert_eq!(fresh[2].1, None);
    assert_eq!(fresh[4].1, None);
    assert_eq!(fresh[6].1, Some(3.0));
    for assignment in ["exp=2;", "exp=unknown;"] {
        let text =
            format!("{assignment}\nparameters a b c d;\na=1;\nb=exp(a);\nc=a+1;\na=2;\nd=a+1;");
        let actual = values(&text);
        assert_eq!(actual[2].1, None, "{assignment}: {actual:?}");
        assert_eq!(actual[3].1, None, "{assignment}: {actual:?}");
        assert_eq!(actual[5].1, Some(3.0), "{assignment}: {actual:?}");
        assert_eq!(assigned_number(&parse(&text), "b"), None);
    }
    let ordinary = values("parameters a b c; a=1; b=exp(a); c=a+1;");
    assert_eq!(ordinary[1].1, None);
    assert_eq!(ordinary[2].1, Some(2.0));
}

fn uri(name: &str) -> Url {
    Url::parse(&format!("file:///C:/dygnosis-inlay/{name}")).unwrap()
}
async fn open(server: &dygnosis::server::Backend, uri: &Url, text: &str) {
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
async fn hints(
    server: &dygnosis::server::Backend,
    uri: &Url,
    range: Option<Range>,
) -> Vec<InlayHint> {
    server
        .inlay_hint(InlayHintParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            range: range.unwrap_or(Range::new(
                Position::new(0, 0),
                Position::new(u32::MAX, u32::MAX),
            )),
            work_done_progress_params: WorkDoneProgressParams::default(),
        })
        .await
        .unwrap()
        .unwrap()
}
fn labels(rows: &[InlayHint]) -> Vec<String> {
    rows.iter()
        .map(|row| {
            let InlayHintLabel::String(label) = &row.label else {
                panic!("string label")
            };
            label.clone()
        })
        .collect()
}

#[tokio::test]
async fn hints_are_per_assignment_and_live_settings_clear_and_restore_them() {
    let (service, _socket) = new_service();
    let server = service.inner();
    let root = uri("ordered.mod");
    open(
        server,
        &root,
        "parameters helper p;\nhelper=2;\np=helper+1;\nhelper=p*2;\np=helper+1;",
    )
    .await;
    assert_eq!(
        labels(&hints(server, &root, None).await),
        ["= 3", "= 6", "= 7"]
    );
    server
        .did_change_configuration(DidChangeConfigurationParams {
            settings: json!({"parameterValueHints":false}),
        })
        .await;
    assert!(hints(server, &root, None).await.is_empty());
    server
        .did_change_configuration(DidChangeConfigurationParams {
            settings: json!({"parameterValueHints":true}),
        })
        .await;
    assert_eq!(hints(server, &root, None).await.len(), 3);
}

#[tokio::test]
async fn repeated_sites_require_every_occurrence_to_be_known_and_identical() {
    let (service, _socket) = new_service();
    let server = service.inner();
    let root = uri("copies.mod");
    for (body, expected) in [
        ("p=2+3;", vec!["= 5"]),
        ("p=@{k}+1;", vec![]),
        ("p=1/(@{k}-1);", vec![]),
        ("p=1/(2-@{k});", vec![]),
    ] {
        open(
            server,
            &root,
            &format!("parameters p;\n@#for k in 1:2\n{body}\n@#endfor\n"),
        )
        .await;
        assert_eq!(
            labels(&hints(server, &root, None).await),
            expected,
            "{body}"
        );
    }
}

#[tokio::test]
async fn include_ownership_activity_and_partial_input_control_authority() {
    let (service, _socket) = new_service();
    let server = service.inner();
    let root = uri("root.mod");
    let child = uri("body.inc");
    open(server, &child, "p=p+1;\n").await;
    assert!(hints(server, &child, None).await.is_empty());
    open(
        server,
        &root,
        "parameters p; p=2;\n@#include \"body.inc\"\n",
    )
    .await;
    assert_eq!(labels(&hints(server, &child, None).await), ["= 3"]);
    open(
        server,
        &uri("other.mod"),
        "parameters p; p=4;\n@#include \"body.inc\"\n",
    )
    .await;
    assert!(hints(server, &child, None).await.is_empty());
    for (text, expected) in [
        ("parameters p; p=2+3;\n@#include \"absent.inc\"\n", 0),
        (
            "parameters p; p=2+3;\n@#if 0\n@#include \"absent.inc\"\n@#endif\n",
            1,
        ),
        ("parameters p; p=2+3; model;", 0),
        ("parameters p; p=2+", 0),
        ("parameters p; p=(2);", 1),
    ] {
        open(server, &root, text).await;
        assert_eq!(hints(server, &root, None).await.len(), expected, "{text}");
    }
}

#[tokio::test]
async fn utf16_mixed_newlines_and_request_ranges_use_written_positions() {
    let (service, _socket) = new_service();
    let server = service.inner();
    let root = uri("utf16.mod");
    open(
        server,
        &root,
        "parameters p q;\r\n/* 🚀 */ p=2+3;\rq=p+1;\n",
    )
    .await;
    let rows = hints(server, &root, None).await;
    assert_eq!(labels(&rows), ["= 5", "= 6"]);
    assert_eq!(rows[0].position, Position::new(1, 15));
    assert_eq!(rows[1].position, Position::new(2, 6));
    let subset = hints(
        server,
        &root,
        Some(Range::new(Position::new(2, 0), Position::new(2, 6))),
    )
    .await;
    assert_eq!(labels(&subset), ["= 6"]);
}

#[tokio::test]
async fn resource_preferences_and_include_edits_affect_only_the_requested_surface() {
    let (service, _socket) = new_service();
    let server = service.inner();
    let a = uri("a/root.mod");
    let b = uri("b/part.inc");
    server.initialize(InitializeParams {
        workspace_folders: Some(vec![WorkspaceFolder { uri:uri("a"), name:"a".into() }, WorkspaceFolder { uri:uri("b"),name:"b".into() }]),
        initialization_options: Some(json!({"configuration":{"schemaVersion":1,"loose":{},"folders":[
            {"uri":uri("a"),"settings":{"searchPaths":["C:/dygnosis-inlay/b"],"parameterValueHints":false}},
            {"uri":uri("b"),"settings":{"parameterValueHints":true}}
        ]}})), ..InitializeParams::default()
    }).await.unwrap();
    open(server, &b, "p=p+1;\n").await;
    open(server, &a, "parameters p; p=2;\n@#include \"part.inc\"\n").await;
    assert!(hints(server, &a, None).await.is_empty());
    assert_eq!(labels(&hints(server, &b, None).await), ["= 3"]);
    open(server, &b, "p=p+2;\n").await;
    assert_eq!(labels(&hints(server, &b, None).await), ["= 4"]);
}

#[tokio::test]
async fn disk_include_changes_revalidate_values_and_split_assignments_have_no_fake_target() {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let directory = std::env::temp_dir().join(format!(
        "dygnosis-inlay-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let source = directory.join("values.inc");
    std::fs::write(&source, "p=p+1;\n").unwrap();
    let root = Url::from_file_path(directory.join("root.mod")).unwrap();
    let child = Url::from_file_path(&source).unwrap();
    let (service, _socket) = new_service();
    let server = service.inner();
    open(
        server,
        &root,
        "parameters p; p=2;\n@#include \"values.inc\"\n",
    )
    .await;
    assert_eq!(labels(&hints(server, &child, None).await), ["= 3"]);
    std::fs::write(&source, "p=p+2;\n").unwrap();
    assert_eq!(labels(&hints(server, &child, None).await), ["= 4"]);
    open(
        server,
        &root,
        "parameters p; p=2+\n@#include \"values.inc\"\n;\n",
    )
    .await;
    open(server, &child, "3\n").await;
    assert!(hints(server, &root, None).await.is_empty());
    assert!(hints(server, &child, None).await.is_empty());
    let resolved = directory.canonicalize().unwrap();
    let temporary = std::env::temp_dir().canonicalize().unwrap();
    assert!(resolved.starts_with(&temporary) && resolved != temporary);
    std::fs::remove_dir_all(resolved).unwrap();
}
