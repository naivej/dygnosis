use std::{collections::HashMap, time::Duration};

use dygnosis::server::{new_service, Backend};
use dygnosis::{analyze, dynare_diagnose, find_preprocessor, parse, run_preprocessor, JsonStage};
use tower_lsp::{lsp_types::*, LanguageServer};

fn official(source: &str, accepted: bool, sentence: Option<&str>) {
    let Some(pp) = find_preprocessor(None) else {
        return;
    };
    let result = run_preprocessor(source, &pp, None, Duration::from_secs(30), JsonStage::Check);
    assert_eq!(result.success, accepted, "{source}: {result:?}");
    if let Some(sentence) = sentence {
        assert!(result.raw_stdout.contains(sentence), "{source}: {result:?}");
    }
}

#[test]
fn forbidden_bare_rhs_roles_reach_scalar_and_multiple_outputs() {
    for (prefix, name, code, sentence) in [
        (
            "var y; model; #loc=1; y=loc; end;",
            "loc",
            "E282",
            "Variable loc not allowed outside model declaration. Its scope is only inside model.",
        ),
        (
            "external_function(name=foo,nargs=1); var y; model; y=0; end;",
            "foo",
            "E279",
            "Symbol 'foo' is the name of a MATLAB/Octave function, and cannot be used as a variable.",
        ),
        (
            "var y; model; y=0; end; epilogue; epi=y; end;",
            "epi",
            "E294",
            "Symbol 'epi' cannot be used outside the epilogue block.",
        ),
        (
            "var y gone; var_remove gone; model; y=0; end;",
            "gone",
            "E426",
            "Variable 'gone' can no longer be used since it has been excluded by a previous 'model_remove' or 'var_remove' statement",
        ),
        (
            "heterogeneity_dimension h; parameters(heterogeneity=h) p; var y; model; y=0; end;",
            "p",
            "E463",
            "Symbol 'p' cannot be used outside model declaration, because it is heterogeneous.",
        ),
    ] {
        for lhs in ["y", "[y,tmp]"] {
            let source = format!("{prefix} steady_state_model; {lhs}=bar({name}); end;");
            official(&source, false, Some(sentence));
            let rows = analyze(&parse(&source));
            let row = rows.iter().find(|row| row.code == code).unwrap_or_else(|| {
                panic!("missing {code} for {lhs}: {rows:?}")
            });
            assert_eq!(row.message, sentence);
            assert_eq!(&source[row.span.start as usize..row.span.end as usize], name);
        }
    }
}

#[test]
fn qualified_native_calls_keep_written_structure_without_namespace_errors() {
    for lhs in ["y", "[y,tmp]"] {
        for rhs in ["pkg.foo(1)", "pkg.sub.foo(1)", "pkg.foo(pkg.sub.bar(1))"] {
            let source = format!("var y; model; y=0; end; steady_state_model; {lhs}={rhs}; end;");
            official(&source, true, None);
            let model = parse(&source);
            let rows = analyze(&model);
            assert!(
                !rows
                    .iter()
                    .any(|row| row.code.starts_with('E') || row.code == "W042"),
                "{source}: {rows:?}"
            );
            let row = &model.steady_state_equations[0];
            assert_eq!(row.rhs.replace(' ', ""), rhs);
            assert!(matches!(
                model.exprs.get(row.rhs_expr.unwrap()).kind,
                dygnosis::expr::ExprKind::Call { .. }
            ));
        }
    }
    for name in ["pkg.foo", "pkg.sub.foo"] {
        let source = format!("var y; model; y=0; end; steady_state_model; y={name}; end;");
        let sentence = format!("Namespace-qualified symbol {name} not allowed in this context");
        official(&source, false, Some(&sentence));
        let rows = analyze(&parse(&source));
        let row = rows.iter().find(|row| row.code == "E275").unwrap();
        assert_eq!(row.message, sentence);
        assert_eq!(
            &source[row.span.start as usize..row.span.end as usize],
            name
        );
    }
}

#[test]
fn reserved_expression_functions_require_parentheses_on_the_next_token() {
    for name in [
        "exp", "log", "ln", "log10", "sin", "cos", "tan", "asin", "acos", "atan", "sinh", "cosh",
        "tanh", "asinh", "acosh", "atanh", "sqrt", "cbrt", "abs", "sign", "max", "min", "normcdf",
        "normpdf", "erf", "erfc", "CBRT",
    ] {
        for (rhs, token, unexpected) in [
            (name.to_string(), ";", "';'"),
            (format!("{name}+1"), "+", "PLUS"),
            (format!("foo({name})"), ")", "')'"),
        ] {
            let source = format!("var y; model; y=0; end; steady_state_model; y={rhs}; end;");
            let sentence = format!("syntax error, unexpected {unexpected}, expecting '('");
            official(&source, false, Some(&sentence));
            let rows = analyze(&parse(&source));
            let row = rows
                .iter()
                .find(|row| row.code == "E001")
                .unwrap_or_else(|| panic!("{source}: {rows:?}"));
            assert_eq!(row.message, sentence);
            assert_eq!(
                &source[row.span.start as usize..row.span.end as usize],
                token
            );
        }
    }
}

#[test]
fn removed_names_are_not_missing_steady_state_outputs() {
    for source in [
        "var y gone; var_remove gone; model; y=0; end; steady_state_model; y=1; end;",
        "var y gone; model; [name='Y'] y=0; [name='G'] gone=0; end; model_remove('G'); steady_state_model; y=1; end;",
        "var y gone; model; y=0; end; steady_state_model; y=1; end; var_remove gone;",
    ] {
        official(source, true, None);
        let rows = analyze(&parse(source));
        assert!(!rows.iter().any(|row| row.code == "W042"), "{source}: {rows:?}");
    }
}

#[test]
fn qualified_calls_keep_reserved_tokens_and_argument_syntax() {
    for (rhs, sentence, token) in [
        ("pkg.foo()", "syntax error, unexpected ')'", ")"),
        ("pkg.(1)", "syntax error, unexpected '('", "("),
        ("pkg.foo.", "syntax error, unexpected ';'", ";"),
        ("pkg.log(1)", "syntax error, unexpected LOG", "log"),
        (
            "cbrt.foo(1)",
            "syntax error, unexpected '.', expecting '('",
            ".",
        ),
        (
            "pkg.foo(cbrt)",
            "syntax error, unexpected ')', expecting '('",
            ")",
        ),
    ] {
        let source = format!("var y; model; y=0; end; steady_state_model; y={rhs}; end;");
        official(&source, false, Some(sentence));
        let rows = analyze(&parse(&source));
        let row = rows.iter().find(|row| row.code == "E001").unwrap();
        assert_eq!(row.message, sentence);
        assert_eq!(
            &source[row.span.start as usize..row.span.end as usize],
            token
        );
    }
}

#[test]
fn qualified_call_arguments_keep_the_ordinary_variable_call_refusal() {
    for name in ["y", "p", "tmp"] {
        for arguments in ["0", "-1", "1.5", "1,2"] {
            let call = format!("{name}({arguments})");
            let source = format!("parameters p; var y; model; y=0; end; steady_state_model; tmp=1; y=pkg.foo({call}); end;");
            let sentence = format!(
                "Using variable {name} with a lead or a lag is not allowed in this context"
            );
            official(&source, false, Some(&sentence));
            let rows = analyze(&parse(&source));
            let row = rows
                .iter()
                .find(|row| row.code == "E001")
                .unwrap_or_else(|| panic!("{source}: {rows:?}"));
            assert_eq!(row.message, sentence);
            assert_eq!(
                &source[row.span.start as usize..row.span.end as usize],
                call
            );
        }
    }
}

#[test]
fn role_checks_use_parser_context_before_later_retypes() {
    for (prefix, name, code) in [
        ("model_local_variable loc;", "loc", "E282"),
        ("external_function(name=foo,nargs=1);", "foo", "E279"),
        ("var gone; var_remove gone;", "gone", "E426"),
        (
            "heterogeneity_dimension h; parameters(heterogeneity=h) p;",
            "p",
            "E463",
        ),
    ] {
        for before in [false, true] {
            let change = format!("change_type(parameters) {name};");
            let source = format!(
                "{prefix} {} var y; model; y=0; end; steady_state_model; y={name}; end; {}",
                if before { &change } else { "" },
                if before { "" } else { &change }
            );
            official(&source, before, None);
            let rows = analyze(&parse(&source));
            assert_eq!(
                rows.iter().any(|row| row.code == code),
                !before,
                "{source}: {rows:?}"
            );
        }
    }
    // A later function declaration collides with the earlier local; it cannot
    // retroactively make that earlier bare use a function-as-variable Error.
    let source = "var y; model; y=0; end; steady_state_model; foo=1; y=foo; end; external_function(name=foo,nargs=1);";
    official(
        source,
        false,
        Some("Symbol foo declared twice with different types!"),
    );
    let rows = analyze(&parse(source));
    assert!(rows.iter().any(|row| row.code == "E030"));
    assert!(!rows.iter().any(|row| row.code == "E279"), "{rows:?}");
}

#[test]
fn final_endogenous_types_control_the_missing_output_warning() {
    for (source, warns) in [
        ("parameters gone; change_type(var) gone; var y; model; y=0; gone=0; end; steady_state_model; y=1; end;", true),
        ("var gone; change_type(parameters) gone; var y; model; y=0; end; steady_state_model; y=1; end;", false),
        ("var y gone; var_remove gone; change_type(var) gone; model; y=0; gone=0; end; steady_state_model; y=1; end;", true),
    ] {
        official(source,true, if warns {Some("variable 'gone' is not assigned a value")} else {None});
        let rows = analyze(&parse(source));
        assert_eq!(rows.iter().any(|row| row.code == "W042" && row.message == "variable 'gone' is not assigned a value"), warns, "{source}: {rows:?}");
    }
}

#[test]
fn true_builtins_and_declared_or_repeated_native_calls_stay_accepted() {
    for rhs in [
        "exp(1)",
        "log(1)",
        "ln(1)",
        "log10(1)",
        "sin(1)",
        "cos(1)",
        "tan(1)",
        "asin(0)",
        "acos(0)",
        "atan(1)",
        "sinh(1)",
        "cosh(1)",
        "tanh(1)",
        "asinh(1)",
        "acosh(1)",
        "atanh(0)",
        "sqrt(1)",
        "cbrt(1)",
        "abs(1)",
        "sign(1)",
        "max(1,2)",
        "min(1,2)",
        "normcdf(1)",
        "normpdf(1)",
        "erf(1)",
        "erfc(1)",
        "nan",
        "inf",
        "NaN",
        "INF",
    ] {
        let source = format!("var y; model; y=0; end; steady_state_model; y={rhs}; end;");
        official(&source, true, None);
        assert!(
            !analyze(&parse(&source))
                .iter()
                .any(|row| row.code.starts_with('E')),
            "{source}"
        );
    }
    for source in [
        "external_function(name=pkg.foo,nargs=1); var y; model; y=0; end; steady_state_model; y=pkg.foo(1); end;",
        "var y; model; y=0; end; steady_state_model; y=pkg.foo(1); y=pkg.foo(1,2); end;",
        "parameters pkg; var y; model; y=0; end; steady_state_model; y=pkg.foo(1); end;",
    ] {
        official(source,true,None);
        assert!(!analyze(&parse(source)).iter().any(|row| row.code.starts_with('E')), "{source}");
    }
}

#[test]
fn model_only_operators_and_other_reserved_tokens_are_not_ss_rhs_symbols() {
    for (rhs, token, spelling) in [
        ("steady_state(y)", "STEADY_STATE", "steady_state"),
        ("expectation(0)(y)", "EXPECTATION", "expectation"),
        ("var_expectation(foo)", "VAR_EXPECTATION", "var_expectation"),
        ("pac_expectation(foo)", "PAC_EXPECTATION", "pac_expectation"),
        (
            "pac_target_nonstationary(foo)",
            "PAC_TARGET_NONSTATIONARY",
            "pac_target_nonstationary",
        ),
        ("sum(y)", "SUM", "sum"),
        ("diff(y)", "DIFF", "diff"),
        ("var", "VAR", "var"),
    ] {
        for rhs in [rhs.to_string(), format!("pkg.foo({rhs})")] {
            let source = format!("var y; model; y=0; end; steady_state_model; y={rhs}; end;");
            let sentence = format!("syntax error, unexpected {token}");
            official(&source, false, Some(&sentence));
            let rows = analyze(&parse(&source));
            let row = rows.iter().find(|row| row.code == "E001").unwrap();
            assert_eq!(row.message, sentence);
            assert_eq!(
                &source[row.span.start as usize..row.span.end as usize],
                spelling
            );
        }
    }
}

#[test]
fn repeated_macro_spans_keep_each_rhs_role_at_its_parser_context() {
    for (prefix, name, code) in [
        ("model_local_variable loc;", "loc", "E282"),
        ("external_function(name=foo,nargs=1);", "foo", "E279"),
        ("var gone; var_remove gone;", "gone", "E426"),
    ] {
        let source = format!("{prefix} var y; model; y=0; end;\n@#for i in 1:2\n@#if i==2\nchange_type(parameters) {name};\n@#endif\nsteady_state_model; y=pkg.foo({name}); end;\n@#endfor\n");
        official(&source, false, None);
        let rows = analyze(&parse(&source));
        assert_eq!(
            rows.iter().filter(|row| row.code == code).count(),
            1,
            "{source}: {rows:?}"
        );
    }
}

#[test]
fn qualified_call_identity_reaches_duplicate_declarations_with_the_first_call_range() {
    for name in ["pkg.foo", "pkg.sub.foo"] {
        let source = format!("var y; model; y=0; end; steady_state_model; y={name}(1); end; external_function(name={name},nargs=1);");
        let sentence = format!("Symbol {name} declared twice.");
        official(&source, true, Some(&sentence));
        let model = parse(&source);
        let rows = analyze(&model);
        let row = rows
            .iter()
            .find(|row| row.code == "W031")
            .unwrap_or_else(|| panic!("{source}: {rows:?}"));
        assert_eq!(row.message, sentence);
        assert_eq!(
            &source[row.span.start as usize..row.span.end as usize],
            name
        );
        assert_eq!(row.related.len(), 1);
        assert_eq!(
            &source[row.related[0].span.start as usize..row.related[0].span.end as usize],
            name
        );
        assert_eq!(
            model.name(model.external_functions[0].name.unwrap().0),
            name
        );
    }
    let root = "C:/ssm-expression/model.mod";
    let include = "C:/ssm-expression/ss.inc";
    let source = "var y; model; y=0; end;\n@#include \"ss.inc\"\nexternal_function(name=pkg.sub.foo,nargs=1);";
    let included = "steady_state_model; /* 🚀 */ y=pkg.sub.foo(1); end;";
    let files = HashMap::from([
        (root.to_string(), source.to_string()),
        (include.to_string(), included.to_string()),
    ]);
    let rows = dynare_diagnose(source, Some(root), Some(&files));
    let row = rows.iter().find(|row| row.code == "W031").unwrap();
    assert_eq!(row.related.len(), 1);
    let related = &row.related[0];
    assert_eq!(related["file"].as_str(), Some(include));
    let column = included[..included.find("pkg.sub.foo").unwrap()]
        .chars()
        .count() as u64
        + 1;
    assert_eq!(related["column"].as_u64(), Some(column));
    assert_eq!(related["end_column"].as_u64(), Some(column + 11));
}

async fn pull(server: &Backend, uri: &Url) -> Vec<Diagnostic> {
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
async fn mcp_include_ranges_and_lsp_utf16_roles_clear_after_edit() {
    let root = "C:/ssm-expression/model.mod";
    let include = "C:/ssm-expression/ss.inc";
    let source =
        "external_function(name=helper,nargs=1); var y; model; y=0; end;\n@#include \"ss.inc\"";
    let included = "steady_state_model; /* 🚀 */ [y,tmp]=pkg.foo(helper); end;";
    let files = HashMap::from([
        (root.to_string(), source.to_string()),
        (include.to_string(), included.to_string()),
    ]);
    let rows = dynare_diagnose(source, Some(root), Some(&files));
    let row = rows.iter().find(|row| row.code == "E279").unwrap();
    assert_eq!(row.file.as_deref(), Some(include));
    let column = included[..included.find("helper").unwrap()].chars().count() as u32 + 1;
    assert_eq!(
        (row.line, row.column, row.end_line, row.end_column),
        (1, column, 1, column + 6)
    );
    assert!(!rows.iter().any(|row| row.code == "E275"));

    let (service, _socket) = new_service();
    let server = service.inner();
    let uri = Url::parse("file:///C:/ssm-expression/live.mod").unwrap();
    let source =
        format!("external_function(name=helper,nargs=1); var y; model; y=0; end;\r\n{included}");
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
        .find(|row| row.code == Some(NumberOrString::String("E279".into())))
        .unwrap();
    let column = included[..included.find("helper").unwrap()]
        .encode_utf16()
        .count() as u32;
    assert_eq!(
        row.range,
        Range::new(Position::new(1, column), Position::new(1, column + 6))
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
                text: source.replace("pkg.foo(helper)", "pkg.foo(helper(1))"),
            }],
        })
        .await;
    let rows = pull(server, &uri).await;
    assert!(
        !rows.iter().any(
            |row| row.code == Some(NumberOrString::String("E279".into()))
                || row.code == Some(NumberOrString::String("E275".into()))
        ),
        "{rows:?}"
    );
}
