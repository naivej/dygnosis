use std::{collections::HashMap, time::Duration};

use dygnosis::server::{new_service, Backend};
use dygnosis::{
    analyze, dynare_diagnose, dynare_model_info, find_preprocessor, parse, run_preprocessor,
    ExprKind, JsonStage, Severity,
};
use tower_lsp::{lsp_types::*, LanguageServer};

fn official(source: &str, accepted: bool, sentence: Option<&str>) {
    let Some(preprocessor) = find_preprocessor(None) else {
        return;
    };
    let result = run_preprocessor(
        source,
        &preprocessor,
        None,
        Duration::from_secs(30),
        JsonStage::Check,
    );
    assert_eq!(result.success, accepted, "{source}: {result:?}");
    if let Some(sentence) = sentence {
        assert!(result.raw_stdout.contains(sentence), "{source}: {result:?}");
    }
}

fn error(source: &str, code: &str, sentence: &str) -> dygnosis::Diagnostic {
    official(source, false, Some(sentence));
    let rows = analyze(&parse(source));
    rows.into_iter()
        .find(|row| row.code == code && row.message == sentence)
        .unwrap_or_else(|| {
            panic!(
                "missing {code}: {sentence}\n{source}: {:?}",
                analyze(&parse(source))
            )
        })
}

#[test]
fn declared_qualified_model_calls_keep_all_names_and_arguments() {
    for name in ["pkg.foo", "pkg.sub.foo", "pkg.sub.more.foo"] {
        for rhs in [format!("{name}(y(-1))"), format!("{name}(1)")] {
            let source =
                format!("external_function(name={name},nargs=1); var y; model; y={rhs}; end;");
            official(&source, true, None);
            let model = parse(&source);
            let rows = analyze(&model);
            assert!(
                !rows.iter().any(|row| row.severity == Severity::Error),
                "{source}: {rows:?}"
            );
            let expression = model.exprs.get(model.equations[0].rhs_expr.unwrap());
            let ExprKind::Call { callee, args } = &expression.kind else {
                panic!("{expression:?}")
            };
            assert_eq!(model.name(*callee), name);
            assert_eq!(args.len(), 1);
            assert_eq!(
                &source[expression.span.start as usize..expression.span.end as usize],
                rhs
            );
        }
    }
    let source = "parameters pkg; pkg=1; external_function(name=pkg.sub.foo,nargs=2); external_function(name=other.bar,nargs=1); var y; model; #loc=pkg . sub . foo(pkg,other.bar(y(-1))); y=loc; end;";
    official(source, true, None);
    let model = parse(source);
    assert!(
        !analyze(&model)
            .iter()
            .any(|row| row.severity == Severity::Error),
        "{:?}",
        analyze(&model)
    );
    let ExprKind::Call { callee, args } =
        &model.exprs.get(model.equations[0].rhs_expr.unwrap()).kind
    else {
        panic!("{:?}", model.equations)
    };
    assert_eq!(model.name(*callee), "pkg.sub.foo");
    assert_eq!(args.len(), 2);
}

#[test]
fn undeclared_qualified_calls_use_the_external_declaration_refusal() {
    for name in ["pkg.foo", "pkg.sub.foo"] {
        for declaration in ["", "external_function(name=pkg.foo,nargs=1);"] {
            if !declaration.is_empty() && name == "pkg.foo" {
                continue;
            }
            let source = format!("{declaration}var y; model; y={name}(y(-1)); end; external_function(name={name},nargs=1);");
            let sentence = format!("To use an external function ({name}) within the model block, you must first declare it via the external_function() statement.");
            let row = error(&source, "E001", &sentence);
            assert_eq!(
                &source[row.span.start as usize..row.span.end as usize],
                name
            );
            assert!(!analyze(&parse(&source))
                .iter()
                .any(|row| row.code == "E275"));
        }
    }
}

#[test]
fn qualified_primary_calls_match_nargs_and_derivative_only_names_refuse() {
    for (options, rhs, name, sentence) in [
        ("name=pkg.foo", "pkg.foo(y,y)", "pkg.foo", "The number of arguments passed to pkg.foo() does not match those of a previous call or declaration of this function."),
        ("name=pkg.sub.foo,nargs=2", "pkg.sub.foo(y)", "pkg.sub.foo", "The number of arguments passed to pkg.sub.foo() does not match those of a previous call or declaration of this function."),
        ("name=pkg.foo,nargs=1,first_deriv_provided=pkg.jac", "pkg.jac(y)", "pkg.jac", "Using a derivative of an external function (pkg.jac) in the model block is currently not allowed."),
    ] {
        let source = format!("external_function({options}); var y; model; y={rhs}; end;");
        let row = error(&source,"E001",sentence);
        assert_eq!(&source[row.span.start as usize..row.span.end as usize], name);
    }
}

#[test]
fn qualified_call_arguments_reach_unknown_role_and_excluded_checks() {
    for (prefix, argument, code, sentence) in [
        ("", "missing", "E020", "Undeclared identifier 'missing' in equation. Fix: add 'missing' to a var, varexo, or parameters declaration."),
        ("external_function(name=bar,nargs=1);", "bar", "E280", "Symbol bar is a function name external to Dynare. It cannot be used like a variable without input argument inside model."),
        ("parameters p; p=local;", "local", "E281", "Variable local not allowed inside model declaration. Its scope is only outside model."),
        ("var gone; var_remove gone;", "gone", "E426", "Variable 'gone' can no longer be used since it has been excluded by a previous 'model_remove' or 'var_remove' statement"),
    ] {
        let source = format!("{prefix}external_function(name=pkg.sub.foo,nargs=1); var y; model; y=pkg.sub.foo({argument}); end;");
        // E020 has the existing editor wording warrant.
        let official_sentence = if code == "E020" { "Unknown symbol: missing" } else { sentence };
        official(&source, false, Some(official_sentence));
        let rows = analyze(&parse(&source));
        assert!(rows.iter().any(|row| row.code == code && row.message == sentence), "{source}: {rows:?}");
        assert!(!rows.iter().any(|row| row.code == "E275"), "{source}: {rows:?}");
    }
}

#[test]
fn implicit_nonmodel_call_still_needs_a_prior_explicit_declaration() {
    let source = "var y z; steady_state_model; z=pkg.sub.foo(1); end; model; y=pkg.sub.foo(y(-1)); z=0; end;";
    let sentence = "Before using pkg.sub.foo() in the model block, you must first declare it via the external_function() statement";
    let _ = error(source, "E001", sentence);
    let accepted = source.replace(
        "end; model;",
        "end; external_function(name=pkg.sub.foo,nargs=1); model;",
    );
    official(&accepted, true, None);
    assert!(
        !analyze(&parse(&accepted))
            .iter()
            .any(|row| row.severity == Severity::Error),
        "{:?}",
        analyze(&parse(&accepted))
    );
}

#[test]
fn bare_qualified_model_values_remain_syntax_refusals() {
    for name in ["pkg.foo", "pkg.sub.foo"] {
        let source =
            format!("external_function(name={name},nargs=1); var y; model; y={name}; end;");
        let _ = error(
            &source,
            "E001",
            "syntax error, unexpected ';', expecting '(' or '.'",
        );
    }
    let source = "var y; model; y=0; end; steady_state_model; y=pkg.sub.foo; end;";
    let _ = error(
        source,
        "E275",
        "Namespace-qualified symbol pkg.sub.foo not allowed in this context",
    );
}

#[test]
fn qualified_model_calls_preserve_reserved_tokens_and_argument_syntax() {
    for (rhs, sentence, token) in [
        ("pkg.sub.foo()", "syntax error, unexpected ')'", ")"),
        ("pkg.sub.foo(y,)", "syntax error, unexpected ')'", ")"),
        ("pkg.sub.foo(,y)", "syntax error, unexpected COMMA", ","),
        (
            "pkg.sub.foo(y y)",
            "syntax error, unexpected IDENTIFIER, expecting COMMA or ')'",
            "y",
        ),
        ("pkg.sub.foo(+)", "syntax error, unexpected ')'", ")"),
        ("pkg.sub.foo(y+)", "syntax error, unexpected ')'", ")"),
        (
            "pkg.sub.foo('label')",
            "syntax error, unexpected QUOTED_STRING",
            "'label'",
        ),
        (
            "pkg.sub.foo(y=1)",
            "syntax error, unexpected EQUAL, expecting COMMA or ')'",
            "=",
        ),
        (
            "pkg.sub.foo(y",
            "syntax error, unexpected ';', expecting COMMA or ')'",
            ";",
        ),
        ("pkg.(y)", "syntax error, unexpected '('", "("),
        ("pkg.sub.foo.", "syntax error, unexpected ';'", ";"),
        ("pkg.exp(y)", "syntax error, unexpected EXP", "exp"),
        (
            "cbrt.foo(y)",
            "syntax error, unexpected '.', expecting '('",
            ".",
        ),
        (
            "pkg.sub.foo(exp)",
            "syntax error, unexpected ')', expecting '('",
            ")",
        ),
    ] {
        let source =
            format!("external_function(name=pkg.sub.foo,nargs=1); var y; model; y={rhs}; end;");
        let row = error(&source, "E001", sentence);
        assert_eq!(
            &source[row.span.start as usize..row.span.end as usize],
            token
        );
    }
}

#[test]
fn qualified_model_arguments_require_complete_grouped_expressions() {
    for (rhs, sentence, token, offset) in [
        ("pkg.fn((y,y))", "syntax error, unexpected COMMA", ",", 9),
        ("pkg.fn(((y,y)))", "syntax error, unexpected COMMA", ",", 10),
        (
            "pkg.fn((y y),y)",
            "syntax error, unexpected IDENTIFIER",
            "y",
            10,
        ),
        ("pkg.fn((),y)", "syntax error, unexpected ')'", ")", 8),
        ("pkg.fn((y=1),y)", "syntax error, unexpected EQUAL", "=", 9),
        ("pkg.fn((y;y),y)", "syntax error, unexpected ';'", ";", 9),
        (
            "pkg.fn(pkg.inner((y,y)),y)",
            "syntax error, unexpected COMMA",
            ",",
            19,
        ),
        (
            "pkg.fn(max((y,y)),y)",
            "syntax error, unexpected COMMA",
            ",",
            13,
        ),
    ] {
        let prefix = "external_function(name=pkg.fn,nargs=2); external_function(name=pkg.inner,nargs=2); var y; model; y=";
        let source = format!("{prefix}{rhs}; end;");
        let row = error(&source, "E001", sentence);
        assert_eq!(row.span.start as usize, prefix.len() + offset);
        assert_eq!(row.span.end as usize, prefix.len() + offset + token.len());
    }
    for rhs in [
        "pkg.fn((y),y)",
        "pkg.fn(((y+y)),(y))",
        "pkg.fn((pkg.inner(y,y)),y)",
    ] {
        let source = format!("external_function(name=pkg.fn,nargs=2); external_function(name=pkg.inner,nargs=2); var y; model; y={rhs}; end;");
        official(&source, true, None);
        assert!(!analyze(&parse(&source))
            .iter()
            .any(|row| row.severity == Severity::Error));
    }
}

#[test]
fn qualified_model_calls_keep_model_facts_and_macro_execution_order() {
    let source = "parameters p; p=1; external_function(name=pkg.sub.foo,nargs=2); var y; model; y=pkg.sub.foo(p,y(-1)); end;";
    official(source, true, None);
    let model = parse(source);
    let refs = model.ident_refs(&model.equations[0]);
    assert_eq!(refs.len(), 3);
    assert!(refs
        .iter()
        .any(|r| model.name(r.name) == "p" && r.timing == 0));
    assert!(refs
        .iter()
        .any(|r| model.name(r.name) == "y" && r.timing == -1));
    assert!(!analyze(&model).iter().any(|row| row.code == "W022"));
    let info = dynare_model_info(source, None, None);
    assert_eq!(info["n_model_equations"], 1);
    assert_eq!(info["predetermined"], serde_json::json!(["y"]));

    let missing = "var y;\n@#for i in 1:2\n@#if i==2\nexternal_function(name=pkg.sub.foo,nargs=1);\n@#endif\nmodel; y=pkg.sub.foo(y(-1)); end;\n@#endfor\n";
    let sentence = "To use an external function (pkg.sub.foo) within the model block, you must first declare it via the external_function() statement.";
    let _ = error(missing, "E001", sentence);
    let rows = analyze(&parse(missing));
    assert_eq!(rows.iter().filter(|row| row.message == sentence).count(), 1);
    let accepted = "var y;\n@#for i in 1:2\nexternal_function(name=pkg.sub.foo,nargs=1);\nmodel; y=pkg.sub.foo(y(-1)); end;\n@#endfor\n";
    official(accepted, true, None);
    assert!(!analyze(&parse(accepted))
        .iter()
        .any(|row| row.severity == Severity::Error));
}

#[test]
fn qualified_model_refusals_keep_include_origins_in_mcp() {
    let root = "C:/qualified-model/model.mod";
    let child = "C:/qualified-model/equation.inc";
    let source = "var y; model;\n@#include \"equation.inc\"\nend; external_function(name=pkg.sub.foo,nargs=1);";
    let included = "/* 🚀 */ y=pkg . sub . foo(y(-1));";
    let files = HashMap::from([
        (root.to_string(), source.to_string()),
        (child.to_string(), included.to_string()),
    ]);
    let rows = dynare_diagnose(source, Some(root), Some(&files));
    let row = rows.iter().find(|row| row.code == "E001").unwrap();
    assert_eq!(row.file.as_deref(), Some(child));
    let start = included.find("pkg").unwrap();
    let end = included.find("(y").unwrap();
    assert_eq!(row.column as usize, included[..start].chars().count() + 1);
    assert_eq!(row.end_column as usize, included[..end].chars().count() + 1);

    let declared = format!("external_function(name=pkg.sub.foo,nargs=1); {source}");
    assert!(!dynare_diagnose(&declared, Some(root), Some(&files))
        .iter()
        .any(|row| row.severity == "ERROR"));
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
async fn lsp_qualified_model_call_range_uses_utf16_and_clears_after_declaration() {
    let line = "model; /* 🚀 */ y=pkg . sub . foo(y(-1)); end;";
    let source = format!("var y;\r\n{line}");
    let (service, _socket) = new_service();
    let server = service.inner();
    let uri = Url::parse("file:///C:/qualified-model/live.mod").unwrap();
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
    let row = rows
        .iter()
        .find(|row| row.code == Some(NumberOrString::String("E001".into())))
        .unwrap();
    let start = line[..line.find("pkg").unwrap()].encode_utf16().count() as u32;
    let end = line[..line.find("(y").unwrap()].encode_utf16().count() as u32;
    assert_eq!(
        row.range,
        Range::new(Position::new(1, start), Position::new(1, end))
    );
    assert_eq!(row.message, "To use an external function (pkg.sub.foo) within the model block, you must first declare it via the external_function() statement.");
    server
        .did_change(DidChangeTextDocumentParams {
            text_document: VersionedTextDocumentIdentifier {
                uri: uri.clone(),
                version: 2,
            },
            content_changes: vec![TextDocumentContentChangeEvent {
                range: None,
                range_length: None,
                text: format!("external_function(name=pkg.sub.foo,nargs=1); {source}"),
            }],
        })
        .await;
    let rows = pull(server, &uri).await;
    assert!(
        !rows
            .iter()
            .any(|row| row.severity == Some(DiagnosticSeverity::ERROR)),
        "{rows:?}"
    );
}

#[test]
fn model_expression_consumers_keep_qualified_call_identity_and_refusals() {
    for body in [
        "var y; model; #loc=pkg.sub.foo(y(-1)); y=loc; end;",
        "heterogeneity_dimension h; var(heterogeneity=h) hy; var y; model; y=0; end; model(heterogeneity=h); hy=pkg.sub.foo(hy(-1)); end;",
        "var y; model; [name='eq'] y=0; end; model_replace('eq'); y=pkg.sub.foo(y(-1)); end;",
        "var y; model; y=0; end; epilogue; epi=pkg.sub.foo(y); end;",
        "var y; model; y=0; end; planner_objective pkg.sub.foo(y); ramsey_model;",
        "trend_var(growth_factor=pkg.sub.foo(1)) t; var y; model; y=t; end;",
        "log_trend_var(log_growth_factor=pkg.sub.foo(1)) t; var y; model; y=t; end;",
        "trend_var(growth_factor=1) t; var(deflator=pkg.sub.foo(t)) y; model; y=t; end;",
    ] {
        let source = format!("external_function(name=pkg.sub.foo,nargs=1); {body}");
        official(&source,true,None);
        assert!(!analyze(&parse(&source)).iter().any(|row| row.severity == Severity::Error),"{source}: {:?}",analyze(&parse(&source)));
        let wrong_arity = source.replace("nargs=1", "nargs=2");
        let _ = error(&wrong_arity, "E001", "The number of arguments passed to pkg.sub.foo() does not match those of a previous call or declaration of this function.");
    }
}
