//! Native INITIAL boundaries and refused Dynare rows preserve the recorded tree.

use dygnosis::{analyze, parse, Diagnostic, Severity};

const HEADER: &str = "var y;\nvarexo e;\nparameters rho beta;\nrho = .8; beta = .9;\nmodel;\ny = rho*y(-1) + e;\nend;\n";
const NAMESPACE: &str = "Namespace-qualified symbol pp.rho not allowed in this context";

fn errors(source: &str) -> Vec<Diagnostic> {
    analyze(&parse(source))
        .into_iter()
        .filter(|d| d.severity == Severity::Error)
        .collect()
}

fn refusal(source: &str, code: &str, message: &str) -> Diagnostic {
    let rows = errors(source);
    let row = rows
        .iter()
        .find(|row| row.code == code)
        .unwrap_or_else(|| panic!("{source}: {rows:?}"));
    assert_eq!(row.message, message, "{source}: {rows:?}");
    row.clone()
}

#[test]
fn all_native_entries_keep_their_remainder_without_dynare_records() {
    for tail in [
        "plot(1); rho = pp.rho;",
        "plot(1); shocks;",
        "plot(1); var z;",
        "load x; var z;",
        "for i=1:3; rho = pp.rho; end",
        "if true; rho = pp.rho; end",
        "foo; rho = pp.rho;",
        "1; rho = pp.rho;",
        "\"hello\";",
        "{rho}; rho = pp.rho;",
        "@rho; rho = pp.rho;",
        "+rho; rho = pp.rho;",
        "{1, 2};",
        "plot(1); /*c*/ rho = pp.rho;",
        "plot(1); steady;",
        "plot(1); model; y=0; end;",
        "plot(1); data(nobs=1);",
        "disp('a'); initval; y = 1; end;",
        "[rho, nope].prior(shape=beta, mean=.5); rho = pp.rho;",
        "[rho beta].prior(shape=beta, mean=.5); rho = pp.rho;",
        "[rho, /*c*/ beta].prior(shape=beta, mean=.5); rho = pp.rho;",
        "[rho,\tbeta].prior(shape=beta, mean=.5); rho = pp.rho;",
        "disp('... /*'); rho = pp.rho;",
        "plot(1 ..); rho = pp.rho;",
    ] {
        let source = format!("{HEADER}{tail}\n");
        let model = parse(&source);
        assert!(errors(&source).is_empty(), "{tail}: {:?}", errors(&source));
        assert_eq!(model.endogenous.len(), 1, "{tail}");
        assert_eq!(model.equations.len(), 1, "{tail}");
        assert_eq!(model.param_assignments.len(), 2, "{tail}");
        assert!(model.helper_assignments.is_empty(), "{tail}");
        assert!(model.shocks_block.is_none(), "{tail}");
        assert!(model.initval.is_empty(), "{tail}");
        assert!(model.data_statements.is_empty(), "{tail}");
        assert!(model.dotted_statements.is_empty(), "{tail}");
        assert!(model.namespace_qualified.is_empty(), "{tail}");
        let native = model.ms_unparsed_spans.last().unwrap();
        assert_eq!(
            &source[native.start as usize..native.end as usize],
            tail,
            "{tail}"
        );
    }
}

#[test]
fn native_entry_after_dynare_semicolon_includes_midline_and_dropped_characters() {
    for tail in [
        "rho = .7; plot(1); rho = pp.rho;",
        "rho = .7; {rho}; rho = pp.rho;",
        "rho = .7; @rho; rho = pp.rho;",
    ] {
        let source = format!("{HEADER}{tail}");
        let model = parse(&source);
        assert!(errors(&source).is_empty(), "{tail}: {:?}", errors(&source));
        assert_eq!(model.param_assignments.len(), 3);
        let start = source.rfind("; ").unwrap_or(0);
        let native = model.ms_unparsed_spans.last().unwrap();
        assert!(native.start as usize > HEADER.len(), "{start}: {native:?}");
        assert!(source[native.start as usize..native.end as usize].ends_with("rho = pp.rho;"));
    }
}

#[test]
fn native_newline_continuation_and_comment_ends_match_the_shared_walk() {
    for text in [
        "plot(1 ...\n); rho = pp.rho;",
        "plot(1 ... %c\n); rho = pp.rho;",
        "plot(1 ... //c\n); rho = pp.rho;",
        "plot(1); /*c\n*/ rho = pp.rho;",
        "plot(1 ... /* c */ rho = pp.rho;\nrho = pp.rho;",
    ] {
        let source = format!("{HEADER}{text}\n");
        assert!(errors(&source).is_empty(), "{text}: {:?}", errors(&source));
        assert!(parse(&source).namespace_qualified.is_empty());
    }
    for newline in ["\n", "\r\n"] {
        for tail in [
            "plot(1",
            "for i=1:3",
            "function z = foo()",
            "verbatim",
            "plot(1);",
            "disp('... /*');",
        ] {
            let source = format!("{HEADER}{tail}{newline}rho = pp.rho;{newline}");
            refusal(&source, "E275", NAMESPACE);
            assert_eq!(parse(&source).namespace_qualified.len(), 1, "{tail}");
        }
    }
    let source = format!("{HEADER}plot(1 ... /*c\n*/\n); rho = pp.rho;\nrho = pp.rho;\n");
    refusal(&source, "E275", NAMESPACE);
    assert_eq!(parse(&source).namespace_qualified.len(), 1);
}

#[test]
fn initial_comments_and_later_declarations_cannot_reclassify_native_text() {
    let source = "/* c */ % c\n// c\nvar y;\nfoo(1); var z;\nparameters foo;\nfoo = .5;\nmodel; y = foo; end;";
    let model = parse(source);
    assert_eq!(model.endogenous.len(), 1);
    assert_eq!(model.parameters.len(), 1);
    assert_eq!(model.param_assignments.len(), 1);
    assert!(errors(source).is_empty(), "{:?}", errors(source));
    assert_eq!(model.ms_unparsed_spans.len(), 1);
    let source = format!("{HEADER}rho = .7; data(nobs=1);\nmodel; y=e; end;");
    assert_eq!(parse(&source).data_statements.len(), 1);
    assert_eq!(parse(&source).equations.len(), 2);
}

#[test]
fn native_lone_quotes_and_trailing_dropped_characters_do_not_swallow_dynare_text() {
    for tail in [
        "plot(y');\nrho = pp.rho;\ndisp('x');",
        "disp('a\nrho = pp.rho;\ndisp('b');",
    ] {
        let source = format!("{HEADER}{tail}\n");
        refusal(&source, "E275", NAMESPACE);
        assert_eq!(parse(&source).namespace_qualified.len(), 1);
    }
    for tail in ["π", "{}", "@"] {
        let source = format!("{HEADER}{tail}");
        assert!(errors(&source).is_empty(), "{tail}: {:?}", errors(&source));
        let model = parse(&source);
        let span = model.ms_unparsed_spans.last().unwrap();
        assert_eq!(&source[span.start as usize..span.end as usize], tail);
    }
}

#[test]
fn verbatim_source_opener_requires_whitespace_and_a_semicolon() {
    let source = format!("{HEADER}verbatim\n;\nrho = pp.rho;\nend;\n");
    assert!(errors(&source).is_empty(), "{:?}", errors(&source));
    assert!(parse(&source).namespace_qualified.is_empty());
    let source = format!("{HEADER}verbatim /*c*/;\nrho = pp.rho;\n");
    refusal(&source, "E275", NAMESPACE);
}

#[test]
fn external_function_and_modfile_local_calls_stay_native() {
    for source in [
        format!("{HEADER}external_function(name=helper, nargs=1);\nhelper(1); rho = pp.rho;\n"),
        "steady_state_model; foo=1; end;\nfoo(1); foo=pp.rho;\n".to_string(),
    ] {
        assert!(
            errors(&source).is_empty(),
            "{source}: {:?}",
            errors(&source)
        );
        assert!(parse(&source).namespace_qualified.is_empty());
    }
}

#[test]
fn unmatched_native_double_quote_keeps_native_end_and_reports_lexer_sentence() {
    let source = format!("{HEADER}plot(\"hello); rho = pp.rho;\n");
    let row = refusal(&source, "E001", "character unrecognized by lexer");
    assert_eq!(
        &source[row.span.start as usize..row.span.end as usize],
        "\""
    );
    assert!(parse(&source).namespace_qualified.is_empty());
}

#[test]
fn dynare_dots_are_refused_before_an_assignment_can_add_value_or_write() {
    for (rhs, offset) in [("1 ...\n+ 0", 2), ("1...", 2)] {
        let source = format!("parameters rho;\nrho = {rhs};\n");
        let row = refusal(&source, "E001", "syntax error, unexpected '.'");
        assert_eq!(row.span.start as usize, source.find(rhs).unwrap() + offset);
        let model = parse(&source);
        assert!(model.param_assignments.is_empty());
        assert!(model.helper_assignments.is_empty());
        assert!(model.write_targets.is_empty());
        assert!(model.namespace_qualified.is_empty());
    }
    for rhs in ["1.5", "1.", ".5", "1 /* ... */ + 0"] {
        let source = format!("parameters rho;\nrho = {rhs};\n");
        assert_eq!(parse(&source).param_assignments.len(), 1, "{rhs}");
        assert!(
            !errors(&source).iter().any(|d| d.code == "E001"),
            "{rhs}: {:?}",
            errors(&source)
        );
    }
    assert_eq!(
        parse(&format!(
            "{HEADER}rho.prior(shape=beta, mean=.5, stdev=.1);\n"
        ))
        .dotted_statements
        .len(),
        1
    );
}

#[test]
fn rejected_model_rows_add_no_equation_use_or_complete_model_claim() {
    for row in ["y = {1};", "y = y';", "{1};", "y = 1{};"] {
        let source = format!("var y;\nvarexo e;\nmodel;\n{row}\nend;\n");
        let diagnostic = refusal(&source, "E001", "character unrecognized by lexer");
        assert_eq!(
            &source[diagnostic.span.start as usize..diagnostic.span.end as usize],
            if row.contains('{') { "{" } else { "'" }
        );
        let model = parse(&source);
        assert!(model.equations.is_empty(), "{row}");
        assert!(model.model_expression_uses.is_empty(), "{row}");
        assert!(model.namespace_qualified.is_empty());
        assert!(dygnosis::check_w010::check_w021(&model).is_empty());
        assert!(!analyze(&model)
            .iter()
            .any(|d| matches!(d.code.as_str(), "E021" | "E188" | "W013" | "I208" | "I209")));
    }
    let source = "var y; varexo e; model; y=0; y={1}; end;";
    let model = parse(source);
    assert_eq!(model.equations.len(), 1);
    assert!(dygnosis::check_w010::check_w021(&model).is_empty());
    assert!(errors("var y; varexo e; model; y=0; end;")
        .iter()
        .any(|d| d.code == "E021"));
    let quoted = "var y(long_name='a\n{b}'); model; y=0; end;";
    assert!(
        !errors(quoted).iter().any(|d| d.code == "E001"),
        "{:?}",
        errors(quoted)
    );
    let quoted = "var y(long_name='a\n{π}'); model; y=0; end;";
    assert!(
        !errors(quoted).iter().any(|d| d.code == "E001"),
        "{:?}",
        errors(quoted)
    );
    let quoted = "var y(long_name='hello'); model; y=0; end;";
    assert!(!errors(quoted).iter().any(|d| d.code == "E001"));
}

#[test]
fn model_local_heads_have_parse_type_and_call_refusals_in_both_forms() {
    for header in [
        "model_local_variable x;\n",
        "var y; model; #x=1; y=x; end;\n",
    ] {
        refusal(
            &format!("{header}x(1);\n"),
            "E001",
            "syntax error, unexpected '(', expecting EQUAL or '.'",
        );
        for tail in ["x.prior(shape=beta, mean=.5);", "x = 1;"] {
            let source = format!("{header}{tail}\n");
            refusal(&source, "E378", "x is not a parameter");
            assert!(!errors(&source).iter().any(|d| d.code == "E058"));
        }
        let source = format!("{header}x = pp.rho;\n");
        refusal(&source, "E275", NAMESPACE);
        assert!(!errors(&source).iter().any(|d| d.code == "E378"));
        assert!(!parse(&source).helper_assignments[0].native);
    }
}

#[test]
fn repeated_macro_heads_use_the_current_execution_symbol_history() {
    let source = "@#for i in 1:2\nx(1);\nmodel_local_variable x;\n@#endfor\n";
    refusal(
        source,
        "E001",
        "syntax error, unexpected '(', expecting EQUAL or '.'",
    );
    let model = parse(source);
    assert_eq!(model.ms_unparsed_spans.len(), 2);
    assert!(model
        .shape_refuses
        .iter()
        .any(|row| row.official_message.as_deref()
            == Some("syntax error, unexpected '(', expecting EQUAL or '.'")));
}

#[test]
fn closing_end_returns_to_initial_but_still_requires_its_semicolon() {
    let source = "var y; varexo e; model; y=e;\nend\nshocks; var z; end;";
    refusal(
        source,
        "E001",
        "syntax error, unexpected SHOCKS, expecting ';'",
    );
    let model = parse(source);
    assert_eq!(model.equations.len(), 1);
    assert_eq!(model.endogenous.len(), 1);
    assert!(model.shocks_block.is_none());
    assert!(model.shock_stmts.is_empty());
    assert_eq!(model.parse_issues.len(), 1, "{:?}", model.parse_issues);
    refusal(
        "var y; model; y=0; end",
        "E001",
        "syntax error, unexpected end of file, expecting ';'",
    );
    for tail in [
        "end;",
        "end /*c*/ ;",
        "end\nplot(1); rho = pp.rho;\n;",
        "end\nproof=1; rho=pp.rho;\n;",
    ] {
        let source = format!("var y; model; y=0; {tail}\n");
        assert!(errors(&source).is_empty(), "{tail}: {:?}", errors(&source));
        assert_eq!(parse(&source).equations.len(), 1);
        let report = dygnosis::expand_report(&source);
        assert!(
            report.model_map.complete && report.navigation_complete,
            "{tail}: {report:?}"
        );
    }
    refusal(
        "var y; model; y=0; end\nvar z;",
        "E001",
        "syntax error, unexpected VAR, expecting ';'",
    );
}

#[test]
fn a_rejected_row_keeps_end_for_its_block_and_the_next_statement() {
    let source = "var y; parameters rho; model; y=0 end; rho=.5;";
    refusal(source, "E001", "syntax error, unexpected END");
    let model = parse(source);
    assert_eq!(model.parse_issues.len(), 1, "{:?}", model.parse_issues);
    assert!(model.equations.is_empty());
    assert_eq!(model.param_assignments.len(), 1);
    assert_eq!(model.param_assignments[0].expression, ".5");
}

#[test]
fn collected_blocks_discard_only_rows_with_active_lexer_refusals() {
    for bad in ["{1}", "1{}", "'"] {
        let source = format!("{HEADER}shocks; var e; stderr {bad}; end;");
        refusal(&source, "E001", "character unrecognized by lexer");
        let model = parse(&source);
        assert!(model.shock_stmts.is_empty(), "{source}");
        assert!(model.shocks_vars.is_empty(), "{source}");
        let source = format!("{HEADER}shocks; var e={bad}; end;");
        refusal(&source, "E001", "character unrecognized by lexer");
        assert!(parse(&source).shock_stmts.is_empty());
    }
    let source = format!("{HEADER}shocks; var bad=1; var e={{1}}; end;");
    assert!(errors(&source).iter().any(|d| d.code == "E058"));
    assert!(!errors(&source).iter().any(|d| d.code == "E001"));
    let source = format!("{HEADER}shocks; var e=1; var e=2; var e={{1}}; end;");
    assert!(errors(&source).iter().any(|d| d.code == "E111"));
    assert_eq!(parse(&source).shock_stmts.len(), 2);

    let source = format!("{HEADER}estimated_params; rho, {{1}}; end;");
    refusal(&source, "E001", "character unrecognized by lexer");
    assert!(parse(&source).estimated_params.is_empty());
    let source = format!("{HEADER}homotopy_setup; rho, 0, {{1}}; end;");
    refusal(&source, "E001", "character unrecognized by lexer");
    assert!(parse(&source).homotopy_rows.is_empty());
    let source = format!("{HEADER}optim_weights; y {{1}}; end;");
    refusal(&source, "E001", "character unrecognized by lexer");
    assert!(parse(&source).optim_weights.is_empty());
    let source = format!("{HEADER}observation_trends; y ({{1}}); end;");
    refusal(&source, "E001", "character unrecognized by lexer");
    assert!(parse(&source).observation_trends.is_empty());
    let source = format!("{HEADER}verbatim; A={{1}}; disp('x'); end;");
    assert!(errors(&source).is_empty(), "{:?}", errors(&source));
}

#[test]
fn multiline_dynare_strings_preserve_expand_geometry_and_literal_characters() {
    let source = "var y(long_name='a\n{b}');\nparameters rho; rho=.5;\nmodel;\ny=rho;\nend;";
    let report = dygnosis::expand_report(source);
    assert!(report.complete && report.navigation_complete && report.model_map.complete);
    assert!(
        report.effective_text.contains("'a\n{b}'"),
        "{}",
        report.effective_text
    );
    assert_eq!(report.n_equations, 1);
    let written = report.origins[0].written_span;
    assert_eq!(
        &source[written.start as usize..written.end as usize],
        "y=rho"
    );
    let emitted = report.navigation[0].effective_span;
    assert_eq!(
        &report.effective_text[emitted.start as usize..emitted.end as usize],
        "y = rho"
    );
    let declaration_names: Vec<_> = report
        .model_map
        .declarations
        .iter()
        .map(|decl| {
            let span = decl.segments[0].span;
            &source[span.start as usize..span.end as usize]
        })
        .collect();
    assert_eq!(declaration_names, ["y", "rho"]);
    let mcp = dygnosis::dynare_expand(source, None, None);
    assert_eq!(mcp["complete"], true, "{mcp}");
    assert_eq!(mcp["origins"][0]["origin"]["line"], 5, "{mcp}");
    assert_eq!(mcp["origins"][0]["origin"]["column"], 1);
    assert_eq!(mcp["origins"][0]["origin"]["end_column"], 6);
}

#[test]
fn multiline_string_origin_projection_keeps_include_and_macro_evidence() {
    let root = "C:/dygnosis-slice20/root.mod";
    let child = "C:/dygnosis-slice20/child.mod";
    let source = "@#include \"child.mod\"\nmodel; y1=0; y2=0; end;";
    let child_source = "@#for i in 1:2\nvar y@{i}(long_name='a\n{b}');\n@#endfor\n";
    let mut workspace = dygnosis::Workspace::new();
    workspace.update_document(root, source);
    workspace.update_document(child, child_source);
    let report = workspace.expand_report(root).unwrap();
    assert!(report.navigation_complete && report.model_map.complete);
    assert_eq!(report.n_equations, 2);
    assert_eq!(report.model_map.declarations.len(), 2);
    for (index, declaration) in report.model_map.declarations.iter().enumerate() {
        assert_eq!(declaration.origin_frames.len(), 1);
        assert_eq!(
            declaration.origin_frames[0].value.as_deref(),
            Some(if index == 0 { "1" } else { "2" })
        );
        assert!(declaration.segments[0]
            .file
            .as_deref()
            .unwrap()
            .ends_with("child.mod"));
        let span = declaration.segments[0].span;
        assert_eq!(
            &child_source[span.start as usize..span.end as usize],
            "y@{i}"
        );
    }
    assert_eq!(report.navigation.len(), 2);
    assert!(report
        .navigation
        .iter()
        .all(|row| row.macro_frames.is_empty()));
    for origin in &report.origins {
        assert!(origin.origin_frames.is_empty());
        assert!(origin.origin_uri.as_deref().unwrap().ends_with("root.mod"));
    }
    assert!(report.effective_text.contains("'a\n{b}'"));
    let files = std::collections::HashMap::from([
        (child.to_string(), child_source.to_string()),
        (root.to_string(), source.to_string()),
    ]);
    let mcp = dygnosis::dynare_expand(source, Some(root), Some(&files));
    assert_eq!(mcp["complete"], true, "{mcp}");
    assert_eq!(mcp["navigation"].as_array().unwrap().len(), 2);
    assert_eq!(mcp["origins"][0]["origin"]["line"], 2);
}

#[test]
fn rejected_compound_rows_add_no_schedule_path_or_steady_state_claim() {
    for row in [
        "periods 1; values {1};",
        "periods {1}; values 1;",
        "periods 1; values ';",
    ] {
        let source = format!("{HEADER}shocks; var e; {row} end;");
        refusal(&source, "E001", "character unrecognized by lexer");
        let model = parse(&source);
        assert!(
            model.shocks_vars.is_empty(),
            "{source}: {:?}",
            model.shocks_vars
        );
        assert!(model.shock_stmts.is_empty());
        assert!(model.shock_blocks[0].scheduled.is_empty());
        let source = format!("{HEADER}shock_paths; var e; {row} end;");
        refusal(&source, "E001", "character unrecognized by lexer");
        assert!(parse(&source).shock_paths[0].stanzas.is_empty());
    }
    let source = format!("{HEADER}shock_paths; var e; periods 1:end; values {{1}}; end;");
    refusal(&source, "E001", "character unrecognized by lexer");
    assert!(parse(&source).shock_paths[0].stanzas.is_empty());
    let source = format!("{HEADER}shock_paths; var e; periods 1:end; values 1; end;");
    assert!(!errors(&source).iter().any(|d| d.code == "E001"));
    assert_eq!(parse(&source).shock_paths[0].stanzas.len(), 1);
    let source = format!("{HEADER}steady_state_model; y={{1}}; end;");
    refusal(&source, "E001", "character unrecognized by lexer");
    let model = parse(&source);
    assert!(model.steady_state_equations.is_empty());
    assert!(model.steady_state_rhs_uses.is_empty());
    let source = format!("{HEADER}occbin_constraints; name 'c'; bind y<{{1}}; end;");
    refusal(&source, "E001", "character unrecognized by lexer");
    assert!(parse(&source).occbin_constraints.is_empty());
    let source = "heterogeneity_dimension d; varexo(heterogeneity=d) e; shocks(heterogeneity=d); var e; stderr {1}; end;";
    refusal(source, "E001", "character unrecognized by lexer");
    assert!(parse(source).shock_blocks[0].stochastic.is_empty());
}

#[test]
fn parse_action_priority_uses_macro_execution_before_written_source_order() {
    for (syntax_iteration, code, sentence) in [
        (1, "E001", "character unrecognized by lexer"),
        (2, "E378", "x is not a parameter"),
    ] {
        let source = format!(
            "model_local_variable x;\n@#for i in 1:2\n@#if i == {}\nx=1;\n@#endif\n@#if i == {syntax_iteration}\nparameters rho; rho={{1}};\n@#endif\n@#endfor\n", 3 - syntax_iteration
        );
        refusal(&source, code, sentence);
        assert_eq!(errors(&source).len(), 1, "{:?}", errors(&source));
        let mcp = dygnosis::dynare_diagnose(&source, None, None);
        assert!(mcp
            .iter()
            .any(|row| row.code == code && row.message == sentence));
        assert!(!mcp
            .iter()
            .any(|row| row.code == if code == "E001" { "E378" } else { "E001" }));
        if let Some(binary) = dygnosis::find_preprocessor(None) {
            let result = dygnosis::run_preprocessor(
                &source,
                &binary,
                None,
                std::time::Duration::from_secs(30),
                dygnosis::JsonStage::Check,
            );
            assert!(!result.success);
            let output = format!("{} {}", result.raw_stdout, result.raw_stderr);
            assert!(output.contains(sentence), "{output}");
            assert!(
                !output.contains(if code == "E001" {
                    "x is not a parameter"
                } else {
                    "character unrecognized by lexer"
                }),
                "{output}"
            );
        }
    }
}

#[test]
fn repeated_subsample_definitions_remain_state_before_a_later_lexer_refusal() {
    let source = "parameters rho;\n@#for i in 1:2\nrho.subsamples(s=2000Q1:2000Q2);\n@#endfor\nrho.s.prior(shape=normal,mean=0,stdev=1);\nrho={1};\n";
    refusal(source, "E001", "character unrecognized by lexer");
    assert!(!errors(source).iter().any(|d| d.code == "E429"));
    let mcp = dygnosis::dynare_diagnose(source, None, None);
    assert!(!mcp.iter().any(|d| d.code == "E429"));
    if let Some(binary) = dygnosis::find_preprocessor(None) {
        let result = dygnosis::run_preprocessor(
            source,
            &binary,
            None,
            std::time::Duration::from_secs(30),
            dygnosis::JsonStage::Check,
        );
        assert!(!result.success);
        let output = format!("{} {}", result.raw_stdout, result.raw_stderr);
        assert!(
            output.contains("character unrecognized by lexer"),
            "{output}"
        );
    }
}

#[test]
fn repeated_invalid_prior_shape_precedes_its_type_and_a_later_lexer_refusal() {
    let source = "model_local_variable x; parameters rho;\n@#for i in 1:2\nx.prior(nope=1);\n@#endfor\nrho={1};\n";
    refusal(
        source,
        "E001",
        "Unexpected token in 'prior'. The grammar takes one of the option names this statement carries here.",
    );
    assert!(!errors(source).iter().any(|d| d.code == "E378"));
    let mcp = dygnosis::dynare_diagnose(source, None, None);
    assert!(mcp
        .iter()
        .any(|d| d.code == "E001" && d.message.contains("prior")));
    assert!(!mcp.iter().any(|d| d.code == "E378"));
    if let Some(binary) = dygnosis::find_preprocessor(None) {
        let result = dygnosis::run_preprocessor(
            source,
            &binary,
            None,
            std::time::Duration::from_secs(30),
            dygnosis::JsonStage::Check,
        );
        assert!(!result.success);
        let output = format!("{} {}", result.raw_stdout, result.raw_stderr);
        assert!(output.contains("syntax error"), "{output}");
        assert!(
            !output.contains("character unrecognized by lexer"),
            "{output}"
        );
        assert!(!output.contains("x is not a parameter"), "{output}");
    }
}

#[test]
fn repeated_completed_shock_actions_precede_a_later_lexer_refusal() {
    for (row, code, sentence, official) in [
        (
            "var e=1; var e=2;",
            "E111",
            "shocks: variance or stderr of shock on e declared twice",
            "shocks: variance or stderr of shock on e declared twice",
        ),
        (
            "var bad=1;",
            "E058",
            "Unknown symbol: bad.",
            "Unknown symbol: bad",
        ),
    ] {
        let source = format!("var y; varexo e; model; y=e; end;\n@#for i in 1:2\nshocks; {row} end;\n@#endfor\nshocks; var e={{1}}; end;\n");
        refusal(&source, code, sentence);
        assert!(!errors(&source).iter().any(|d| d.code == "E001"));
        let mcp = dygnosis::dynare_diagnose(&source, None, None);
        assert!(mcp.iter().any(|d| d.code == code && d.message == sentence));
        assert!(!mcp.iter().any(|d| d.code == "E001"));
        if let Some(binary) = dygnosis::find_preprocessor(None) {
            let result = dygnosis::run_preprocessor(
                &source,
                &binary,
                None,
                std::time::Duration::from_secs(30),
                dygnosis::JsonStage::Check,
            );
            assert!(!result.success);
            let output = format!("{} {}", result.raw_stdout, result.raw_stderr);
            assert!(output.contains(official), "{output}");
            assert!(
                !output.contains("character unrecognized by lexer"),
                "{output}"
            );
        }
    }
}

#[test]
fn regular_shock_shape_and_name_refusals_follow_macro_execution_order() {
    for (syntax_iteration, code, sentence, official) in [
        (2, "E058", "Unknown symbol: bad.", "Unknown symbol: bad"),
        (
            1,
            "E001",
            "syntax error, unexpected END, expecting PERIODS",
            "syntax error, unexpected END, expecting PERIODS",
        ),
    ] {
        let source = format!("var y; varexo e; model; y=e; end;\n@#for i in 1:2\n@#if i == {syntax_iteration}\nshocks; var e; end;\n@#endif\n@#if i == {}\nshocks; var bad=1; end;\n@#endif\n@#endfor\n", 3 - syntax_iteration);
        refusal(&source, code, sentence);
        assert_eq!(errors(&source).len(), 1);
        let mcp = dygnosis::dynare_diagnose(&source, None, None);
        assert!(mcp.iter().any(|d| d.code == code && d.message == sentence));
        assert!(!mcp
            .iter()
            .any(|d| d.code == if code == "E001" { "E058" } else { "E001" }));
        if let Some(binary) = dygnosis::find_preprocessor(None) {
            let result = dygnosis::run_preprocessor(
                &source,
                &binary,
                None,
                std::time::Duration::from_secs(30),
                dygnosis::JsonStage::Check,
            );
            assert!(!result.success);
            let output = format!("{} {}", result.raw_stdout, result.raw_stderr);
            assert!(output.contains(official), "{output}");
            assert!(
                !output.contains(if code == "E001" {
                    "Unknown symbol: bad"
                } else {
                    "syntax error, unexpected END"
                }),
                "{output}"
            );
        }
    }
}

#[test]
fn normal_parse_arbitration_keeps_dotted_shapes_and_subsample_state() {
    let source = "model_local_variable x;\n@#for i in 1:2\nx.prior(nope=1);\n@#endfor\n";
    assert!(errors(source).iter().any(|d| d.code == "E001"));
    assert!(!errors(source).iter().any(|d| d.code == "E378"));
    let source = "parameters rho;\n@#for i in 1:2\nrho.subsamples(s=2000Q1:2000Q2);\n@#endfor\nrho.s.prior(shape=normal,mean=0,stdev=1);\n";
    assert!(errors(source).is_empty(), "{:?}", errors(source));
    let mcp = dygnosis::dynare_diagnose(source, None, None);
    assert!(!mcp.iter().any(|d| d.code == "E429"));
    if let Some(binary) = dygnosis::find_preprocessor(None) {
        let result = dygnosis::run_preprocessor(
            source,
            &binary,
            None,
            std::time::Duration::from_secs(30),
            dygnosis::JsonStage::Check,
        );
        assert!(result.success, "{result:?}");
    }
}

#[test]
fn scalar_target_guard_keeps_earlier_grammar_and_lexer_refusals() {
    for (body, sentence) in [
        (
            "y end=1; y=2; end;",
            "syntax error, unexpected END, expecting EQUAL",
        ),
        (
            "y(0=1 end; parameters q; q=3;",
            "syntax error, unexpected '(', expecting EQUAL",
        ),
        ("y{0}=1; y=2; end;", "character unrecognized by lexer"),
    ] {
        let source = format!("var y; model; y=0; end; steady_state_model; {body}");
        refusal(&source, "E001", sentence);
        assert_eq!(errors(&source).len(), 1, "{:?}", errors(&source));
        let model = parse(&source);
        assert_eq!(
            model.steady_state_equations.len(),
            usize::from(body.contains("y=2;"))
        );
        assert_eq!(
            model.param_assignments.len(),
            usize::from(body.contains("q=3;"))
        );
        if let Some(binary) = dygnosis::find_preprocessor(None) {
            let result = dygnosis::run_preprocessor(
                &source,
                &binary,
                None,
                std::time::Duration::from_secs(30),
                dygnosis::JsonStage::Check,
            );
            assert!(!result.success);
            let output = format!("{} {}", result.raw_stdout, result.raw_stderr);
            assert!(output.contains(sentence), "{output}");
        }
    }
}

#[test]
fn generated_shapes_keep_producer_order_when_tokens_share_written_spans() {
    for generated in [
        "@#define opts = \"nope=1\"\nrho.prior(@{opts});\n",
        "@#define tail = \"x(1);\"\n@{tail}\n",
        "@#define tail = \"rho.fake(1);\"\n@{tail}\n",
        "@#define tail = \"shocks; var e; end;\"\n@{tail}\n",
        "@#define tail = \"dsample(1);\"\n@{tail}\n",
        "@#define tail = \"svar_identification; end;\"\n@{tail}\n",
        "@#define tail = \"svar_identification; exclusion lag 1; end;\"\n@{tail}\n",
        "@#define tail = \"conditional_forecast_paths; end;\"\n@{tail}\n",
        "@#define tail = \"conditional_forecast_paths; var e; periods 1; end;\"\n@{tail}\n",
        "@#define tail = \"data;\"\n@{tail}\n",
        "@#define tail = \"markov_switching;\"\n@{tail}\n",
        "@#define tail = \"svar_global_identification_check(nope=1);\"\n@{tail}\n",
        "@#define tail = \"forecast();\"\n@{tail}\n",
        "@#define tail = \"var_remove bad 1;\"\n@{tail}\n",
    ] {
        for action_first in [true, false] {
            let source = format!(
                "model_local_variable x;\n{HEADER}{}{generated}{}",
                if action_first { "x=1;\n" } else { "" },
                if action_first { "" } else { "x=1;\n" },
            );
            let expected = if action_first { "E378" } else { "E001" };
            let rows = errors(&source);
            assert_eq!(rows.len(), 1, "{source}: {rows:?}");
            assert_eq!(rows[0].code, expected, "{source}: {rows:?}");
            if action_first {
                assert_eq!(rows[0].message, "x is not a parameter");
            }
            let mcp = dygnosis::dynare_diagnose(&source, None, None);
            assert!(mcp.iter().any(|d| d.code == expected));
            assert!(!mcp
                .iter()
                .any(|d| d.code == if action_first { "E001" } else { "E378" }));
            if let Some(binary) = dygnosis::find_preprocessor(None) {
                let result = dygnosis::run_preprocessor(
                    &source,
                    &binary,
                    None,
                    std::time::Duration::from_secs(30),
                    dygnosis::JsonStage::Check,
                );
                assert!(!result.success);
                let output = format!("{} {}", result.raw_stdout, result.raw_stderr);
                assert!(
                    output.contains(if action_first {
                        "x is not a parameter"
                    } else {
                        "syntax error"
                    }),
                    "{source}: {output}"
                );
                assert!(
                    !output.contains(if action_first {
                        "syntax error"
                    } else {
                        "x is not a parameter"
                    }),
                    "{source}: {output}"
                );
            }
        }
    }
    // The grammar must finish the option list before validating its head's
    // type. The option's producer position precedes the statement action.
    let source = "model_local_variable x;\n@#define opts = \"nope=1\"\nx.prior(@{opts});\n";
    assert!(errors(source).iter().any(|d| d.code == "E001"));
    assert!(!errors(source).iter().any(|d| d.code == "E378"));
    let mcp = dygnosis::dynare_diagnose(source, None, None);
    assert!(mcp.iter().any(|d| d.code == "E001"));
    assert!(!mcp.iter().any(|d| d.code == "E378"));
    if let Some(binary) = dygnosis::find_preprocessor(None) {
        let result = dygnosis::run_preprocessor(
            source,
            &binary,
            None,
            std::time::Duration::from_secs(30),
            dygnosis::JsonStage::Check,
        );
        assert!(!result.success);
        let output = format!("{} {}", result.raw_stdout, result.raw_stderr);
        assert!(output.contains("syntax error"), "{output}");
        assert!(!output.contains("x is not a parameter"), "{output}");
    }
}

#[test]
fn lexer_refused_family_bodies_do_not_reach_later_empty_body_shapes() {
    for body in [
        "svar_identification; restriction equation 1, {y}=0; end;",
        "conditional_forecast_paths; var {e}; periods 1; values 1; end;",
    ] {
        let source = format!("{HEADER}{body}");
        refusal(&source, "E001", "character unrecognized by lexer");
        let mcp = dygnosis::dynare_diagnose(&source, None, None);
        assert!(mcp
            .iter()
            .any(|d| d.code == "E001" && d.message == "character unrecognized by lexer"));
        if let Some(binary) = dygnosis::find_preprocessor(None) {
            let result = dygnosis::run_preprocessor(
                &source,
                &binary,
                None,
                std::time::Duration::from_secs(30),
                dygnosis::JsonStage::Check,
            );
            assert!(!result.success);
            let output = format!("{} {}", result.raw_stdout, result.raw_stderr);
            assert!(
                output.contains("character unrecognized by lexer"),
                "{output}"
            );
        }
    }
}
