use std::path::{Path, PathBuf};

use dygnosis::diagnostic::analyze;
use dygnosis::expand::expand_report;
use dygnosis::lexer::{tokenize, Token, TokenKind};
use dygnosis::macro_expand::expand_macros;
use dygnosis::parse;

fn kinds(src: &str) -> Vec<TokenKind> {
    tokenize(src).into_iter().map(|t| t.kind).collect()
}

fn expanded(src: &str) -> Vec<Token> {
    expand_macros(src, tokenize(src))
}

fn expanded_kinds(src: &str) -> Vec<TokenKind> {
    expanded(src).into_iter().map(|t| t.kind).collect()
}

fn lexemes<'a>(src: &'a str, tokens: &'a [Token]) -> Vec<&'a str> {
    tokens
        .iter()
        .filter(|t| t.kind != TokenKind::Eof)
        .map(|t| t.text(src))
        .collect()
}

fn copilot_archive(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(".agents/skills/dynare-copilot/references/model-archive")
        .join(name)
        .join(format!("{name}.mod"))
}

fn copilot_example(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(".agents/skills/dynare-copilot/references/examples")
        .join(format!("{name}.mod"))
}

fn read_mod(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| {
        panic!("fixture missing at {}: {e}", path.display());
    })
}

fn action_fixture(name: &str) -> String {
    read_mod(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/macro_action")
            .join(format!("{name}.mod")),
    )
}

#[test]
fn function_macro_expands_call_in_equation() {
    let src = action_fixture("function");
    let report = expand_report(&src);
    assert_eq!(report.n_equations, 1);
    assert!(
        report.effective_text.contains("y = 2"),
        "{}",
        report.effective_text
    );
    assert_eq!(parse(&src).equations[0].rhs.trim(), "2");
}

#[test]
fn scalar_when_filters_out_false_iteration() {
    let src = action_fixture("scalar_when");
    let report = expand_report(&src);
    assert_eq!(report.n_equations, 2, "{}", report.effective_text);
    assert!(
        !report.effective_text.contains("y_2"),
        "{}",
        report.effective_text
    );
    assert_eq!(report.origins.len(), 2);
    assert!(report.origins.iter().all(|origin| origin.loop_copy));
    assert_eq!(
        report
            .origins
            .iter()
            .map(|origin| origin
                .origin_frames
                .last()
                .unwrap()
                .value
                .as_deref()
                .unwrap())
            .collect::<Vec<_>>(),
        ["1", "3"]
    );
    let diags = analyze(&parse(&src));
    assert!(
        !diags.iter().any(|d| d.code == "E020" || d.code == "W013"),
        "{diags:?}"
    );
}

#[test]
fn tuple_loop_binds_each_element_and_keeps_origins() {
    let src = action_fixture("tuple");
    let report = expand_report(&src);
    assert_eq!(report.n_equations, 2, "{}", report.effective_text);
    assert!(
        report.effective_text.contains("y_1 = 2"),
        "{}",
        report.effective_text
    );
    assert!(
        report.effective_text.contains("y_3 = 4"),
        "{}",
        report.effective_text
    );
    assert_eq!(report.origins.len(), 2);
    assert!(report.origins.iter().all(|origin| origin.loop_copy));
    assert_eq!(
        report
            .origins
            .iter()
            .map(|origin| {
                let frame = origin.origin_frames.last().unwrap();
                (
                    frame.variable.as_deref().unwrap(),
                    frame.value.as_deref().unwrap(),
                )
            })
            .collect::<Vec<_>>(),
        [("(i,j)", "(1,2)"), ("(i,j)", "(3,4)")]
    );
    let diags = analyze(&parse(&src));
    assert!(
        !diags.iter().any(|d| d.code == "E020" || d.code == "W013"),
        "{diags:?}"
    );
}

#[test]
fn quoted_macro_identifier_reports_unquoted_name() {
    let src = action_fixture("quoted_name");
    let diags = analyze(&parse(&src));
    let unknown = diags.iter().find(|d| d.code == "E020").expect("E020");
    assert!(unknown.message.contains("zz"), "{unknown:?}");
    assert!(!unknown.message.contains("\"zz\""), "{unknown:?}");
}

#[test]
fn quoted_macro_expression_is_retokenized_before_model_parsing() {
    for fixture in ["expression_fragment", "expression_whitespace"] {
        let src = action_fixture(fixture);
        let report = expand_report(&src);
        assert!(report.complete, "{fixture}: {}", report.effective_text);
        assert_eq!(report.n_equations, 1, "{fixture}");
        assert!(
            report.effective_text.contains("y = y+1"),
            "{fixture}: {}",
            report.effective_text
        );
        let written = &src[report.origins[0].written_span.start as usize
            ..report.origins[0].written_span.end as usize];
        assert!(written.contains("@{rhs}"), "{fixture}: {written:?}");
        assert!(!analyze(&parse(&src)).iter().any(|diag| diag.code == "E020"));
    }
}

#[test]
fn interpolation_respects_operator_and_identifier_boundaries() {
    let expression = action_fixture("expression_boundary");
    let report = expand_report(&expression);
    assert!(report.complete, "{}", report.effective_text);
    assert!(
        report.effective_text.contains("y = x1+z"),
        "{}",
        report.effective_text
    );
    assert!(!analyze(&parse(&expression))
        .iter()
        .any(|diag| diag.code == "E020"));

    let right = action_fixture("expression_right_boundary");
    let right_report = expand_report(&right);
    assert!(right_report.complete);
    assert!(
        right_report.effective_text.contains("z = y+1"),
        "{}",
        right_report.effective_text
    );
    assert!(!analyze(&parse(&right))
        .iter()
        .any(|diag| diag.code == "E020"));

    let names = action_fixture("identifier_whitespace");
    let model = parse(&names);
    assert_eq!(model.endogenous.len(), 4, "{:?}", model.endogenous);
    for name in ["x", "y", "z"] {
        assert!(model
            .endogenous
            .iter()
            .any(|decl| model.name(decl.name) == name));
    }
    assert!(!model
        .endogenous
        .iter()
        .any(|decl| matches!(model.name(decl.name), "xy" | "xz")));
    assert!(!analyze(&model).iter().any(|diag| diag.code == "E020"));
}

#[test]
fn context_sensitive_replacement_is_explicitly_incomplete() {
    let source = "@#define rhs=\"y/*note*/+1\"\nvar y; model; y=@{rhs}; end;";
    let report = expand_report(source);
    assert!(!report.complete);
    assert_eq!(report.n_equations, 0);
    assert!(report.effective_text.contains("@{rhs}"));
    let diagnostics = analyze(&parse(source));
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].code, "I211");
}

#[test]
fn undefined_bare_define_reports_macro_error_at_directive() {
    let src = action_fixture("unknown_value");
    let diags = analyze(&parse(&src));
    let unknown = diags
        .iter()
        .find(|d| d.message == "Unknown variable zz")
        .expect("macro error");
    assert_eq!(unknown.code, "E063");
    assert_eq!(unknown.span.start, src.find("@#define").unwrap() as u32);
    assert!(!diags.iter().any(|d| d.code == "E020"), "{diags:?}");
    assert!(!diags.iter().any(|d| d.code == "I211"), "{diags:?}");
}

#[test]
fn valid_unsupported_macro_stays_explicitly_incomplete() {
    let src = action_fixture("unsupported_builtin");
    let report = expand_report(&src);
    assert!(!report.complete);
    assert_eq!(report.n_equations, 0);
    assert!(
        report.effective_text.contains("@{n}"),
        "{}",
        report.effective_text
    );
    assert_eq!(analyze(&parse(&src))[0].code, "I211");
    let public = dygnosis::mcp::dynare_expand(&src, None, None);
    assert_eq!(public["status"], "incomplete");
    assert_eq!(public["n_equations"], 0);
    assert_eq!(
        dygnosis::mcp::dynare_model_info(&src, None, None)["status"],
        "incomplete"
    );
    assert_eq!(
        dygnosis::mcp::dynare_equations(&src, None, None, None, None)["status"],
        "incomplete"
    );
    assert_eq!(
        dygnosis::mcp::dynare_compare_models(
            &src,
            "var y; model; y=0; end;",
            None,
            None,
            None,
            None,
            None
        )["status"],
        "incomplete"
    );
    let extraction = dygnosis::mcp::dynare_extract(
        &src,
        None,
        None,
        &["eq".to_string()],
        &std::collections::HashMap::new(),
        None,
    )
    .expect("extract result");
    assert_eq!(extraction["status"], "unsupported_context");
}

#[test]
fn incomplete_macro_analysis_is_visible_in_cli_and_mcp_diagnosis() {
    let src = action_fixture("unsupported_builtin");
    let path = "tests/fixtures/macro_action/unsupported_builtin.mod";
    let set = dygnosis::check_file_with_origins(&src, path);
    let note = set
        .diagnostics
        .iter()
        .find(|diag| diag.code == "I211")
        .expect("incomplete-analysis Information");
    assert_eq!(note.span.start, src.find("@#define").unwrap() as u32);
    let cli = dygnosis::format_check_lines_with_origins(path, &set, &src);
    assert!(cli.contains("INFO [I211]"), "{cli}");
    assert!(!cli.contains("No issues found"), "{cli}");
    let mcp = dygnosis::mcp::dynare_diagnose(&src, None, None);
    let item = mcp
        .iter()
        .find(|diag| diag.code == "I211")
        .expect("MCP I211");
    assert_eq!((item.line, item.column), (2, 1));
}

#[test]
fn recursive_macro_function_and_large_loop_do_not_claim_complete_views() {
    for src in [
        "@#define f(x)=f(x)\nvar y; model; y=@{f(1)}; end;",
        "@#define nums=1:10001\nvar y; model; @#for i in nums\ny=0;\n@#endfor\nend;",
    ] {
        let report = expand_report(src);
        assert!(!report.complete, "{}", report.effective_text);
        assert_eq!(report.n_equations, 0);
        assert_eq!(analyze(&parse(src))[0].code, "I211");
    }
}

#[test]
fn function_macro_can_choose_a_conditional_branch() {
    let src = "@#define f(x)=x+1\nvar y; model;\n@#if f(1)==2\ny=1;\n@#else\ny=0;\n@#endif\nend;";
    let report = expand_report(src);
    assert!(report.complete);
    assert_eq!(report.n_equations, 1);
    assert!(report.effective_text.contains("y = 1"));
    assert!(!report.effective_text.contains("y = 0"));
}

#[test]
fn real_and_mixed_macro_numbers_keep_valid_arithmetic_and_comparisons() {
    for (expression, expected) in [("0.5+0.5", "1"), ("0.5+1", "1.5"), ("2*0.5", "1")] {
        let src = format!("@#define x={expression}\nvar y; model; y=@{{x}}; end;");
        let report = expand_report(&src);
        assert!(report.complete, "{expression}: {}", report.effective_text);
        assert_eq!(parse(&src).equations[0].rhs.trim(), expected);
        assert!(!analyze(&parse(&src)).iter().any(|d| d.code == "E285"));
    }
    let comparison = "var y; model;\n@#if 0.5 < 1\ny=1;\n@#else\ny=0;\n@#endif\nend;";
    let report = expand_report(comparison);
    assert!(report.complete);
    assert!(report.effective_text.contains("y = 1"));
    assert!(!report.effective_text.contains("y = 0"));
}

#[test]
fn defined_is_a_macro_builtin_even_when_its_variable_is_absent() {
    let absent = "var y; model;\n@#if defined(X)\ny=0;\n@#else\ny=1;\n@#endif\nend;";
    let present = format!("@#define X\n{absent}");
    for (source, expected) in [(absent, "y = 1"), (present.as_str(), "y = 0")] {
        let report = expand_report(source);
        assert!(report.complete, "{}", report.effective_text);
        assert!(
            report.effective_text.contains(expected),
            "{}",
            report.effective_text
        );
        assert!(!analyze(&parse(source)).iter().any(|d| d.code == "E063"));
    }
}

#[test]
fn recognized_unevaluated_builtins_and_casts_do_not_claim_unknown_function() {
    for src in [
        "var y; model;\n@#if isempty([1])\ny=0;\n@#else\ny=1;\n@#endif\nend;".to_string(),
        "@#define x=(real) 1\nvar y; model; y=@{x}; end;".to_string(),
    ] {
        let report = expand_report(&src);
        assert!(!report.complete, "{}", report.effective_text);
        let diags = analyze(&parse(&src));
        assert_eq!(diags[0].code, "I211", "{diags:?}");
    }
}

#[test]
fn valid_array_and_string_operators_stay_incomplete_without_type_errors() {
    for expression in ["[1]+[2]", "\"a\"<\"b\""] {
        let source = format!("@#define x={expression}\nvar y; model; y=1; end;");
        let report = expand_report(&source);
        assert!(!report.complete, "{expression}: {}", report.effective_text);
        let diagnostics = analyze(&parse(&source));
        assert_eq!(diagnostics.len(), 1, "{expression}: {diagnostics:?}");
        assert_eq!(diagnostics[0].code, "I211");
    }
}

#[test]
fn unused_malformed_function_refuses_before_model_analysis() {
    let source = "@#define f(x)=x+\nvar y; model; y=1; end;";
    let diags = analyze(&parse(source));
    assert_eq!(diags.len(), 1, "{diags:?}");
    assert_eq!(diags[0].code, "E062");
    assert_eq!(diags[0].message, "syntax error, unexpected EOL");
    assert_eq!(diags[0].span.start, 0);

    let valid = "@#define f(x)=x+g\n@#define g=1\nvar y; model; y=@{f(1)}; end;";
    assert_eq!(parse(valid).equations[0].rhs.trim(), "2");
}

#[test]
fn valid_unsupported_conditional_retains_source_without_false_error() {
    let src = "var y; model;\n@#if length([1])\ny=1;\n@#else\ny=0;\n@#endif\nend;";
    let report = expand_report(src);
    assert!(!report.complete);
    assert_eq!(report.n_equations, 0);
    assert!(report.effective_text.contains("@#if length([1])"));
    let note = analyze(&parse(src));
    assert_eq!(note.len(), 1, "{note:?}");
    assert_eq!(note[0].code, "I211");
    assert_eq!(note[0].span.start, src.find("@#if").unwrap() as u32);
}

#[test]
fn define_line_is_one_macro_dir() {
    let ks = kinds("@#define ZLB = 0\n");
    assert_eq!(ks, [TokenKind::MacroDir, TokenKind::Eof]);
}

#[test]
fn interpolation_is_macro_interp_token() {
    assert_eq!(
        kinds("EXPECTATION(-@{lag})"),
        [
            TokenKind::Ident,
            TokenKind::LParen,
            TokenKind::Minus,
            TokenKind::MacroInterp,
            TokenKind::RParen,
            TokenKind::Eof,
        ]
    );
}

#[test]
fn commented_directive_is_not_macro_dir() {
    let ks = kinds("// @#define X = 1\n");
    assert!(!ks.contains(&TokenKind::MacroDir));
    assert_eq!(ks, [TokenKind::Eof]);
}

#[test]
fn ifndef_define_if_else_keeps_else_branch() {
    let src = "\
@#ifndef ZLB
@#define ZLB = 0
@#endif
@#if ZLB
a=1;
@#else
a=0;
@#endif
";
    let tokens = expanded(src);
    assert!(!tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::MacroDir | TokenKind::MacroInterp)));
    assert_eq!(
        tokens.iter().map(|t| t.kind).collect::<Vec<_>>(),
        [
            TokenKind::Ident,
            TokenKind::Eq,
            TokenKind::Number,
            TokenKind::Semi,
            TokenKind::Eof,
        ]
    );
    assert_eq!(lexemes(src, &tokens), ["a", "=", "0", ";"]);
}

#[test]
fn for_unrolls_expectation_lags_one_through_sixteen() {
    let src = "\
@#define lags = 1:16
@#for lag in lags
+EXPECTATION(-@{lag})(z)
@#endfor
";
    let tokens = expanded(src);
    assert!(!tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::MacroDir | TokenKind::MacroInterp)));
    let expectation_count = tokens
        .iter()
        .filter(|t| t.kind == TokenKind::Ident && t.text(src).eq_ignore_ascii_case("EXPECTATION"))
        .count();
    assert_eq!(expectation_count, 16);
    let numbers: Vec<&str> = tokens
        .iter()
        .filter(|t| t.kind == TokenKind::Number)
        .map(|t| t.text(src))
        .collect();
    let expected: Vec<String> = (1..=16).map(|n| n.to_string()).collect();
    assert_eq!(
        numbers,
        expected.iter().map(String::as_str).collect::<Vec<_>>()
    );
    assert!(!numbers.contains(&"@{lag}"));
}

#[test]
fn identifier_at_end_of_file_keeps_the_eof_token() {
    let src = "var y";
    let tokens = expanded(src);
    assert!(
        tokens.last().is_some_and(|tok| tok.kind == TokenKind::Eof),
        "{:?}",
        tokens.iter().map(|tok| tok.kind).collect::<Vec<_>>()
    );
    let model = parse(src);
    assert!(model
        .endogenous
        .iter()
        .any(|decl| model.name(decl.name) == "y"));

    let glued = "\
@#define i = 1
var x@{i}";
    let model = parse(glued);
    assert!(model
        .endogenous
        .iter()
        .any(|decl| model.name(decl.name) == "x1"));
}

#[test]
fn adjacent_interpolation_becomes_one_identifier() {
    let src = "\
@#define is = 1:2
@#for i in is
var x@{i};
@#endfor
";
    let tokens = expanded(src);
    let names: Vec<&str> = tokens
        .iter()
        .filter(|tok| tok.kind == TokenKind::Ident && tok.text(src).starts_with('x'))
        .map(|tok| tok.text(src))
        .collect();
    assert_eq!(names, ["x1", "x2"]);
}

#[test]
fn whole_name_substitution_stays_one_identifier() {
    let src = "\
@#define a = \"beta\"
var @{a};
";
    let tokens = expanded(src);
    assert!(tokens.iter().any(|tok| tok.text(src) == "beta"));
}

#[test]
fn empty_and_plain_source_are_identity() {
    let empty = "";
    assert_eq!(kinds(empty), expanded_kinds(empty));

    let plain = "var y;\nmodel;\ny = 0;\nend;\n";
    assert_eq!(kinds(plain), expanded_kinds(plain));
    assert!(!expanded(plain)
        .iter()
        .any(|t| matches!(t.kind, TokenKind::MacroDir | TokenKind::MacroInterp)));
}

#[test]
fn parse_zlb_shaped_f17_drops_inactive_qe_rule() {
    let src = "\
@#define QE = 0
model;
[name='F17 QE rule']
@#if QE
qe = rho_qe*qe(-1) + phi_qe*(Y_ss - Y)/Y_ss;
@#else
qe = 0;
@#endif
end;
";
    let model = parse(src);
    assert_eq!(model.equations.len(), 1);
    let text = &model.equations[0].text;
    assert!(text.contains("qe = 0"), "text was {text:?}");
    assert!(!text.contains("rho_qe"), "text was {text:?}");
}

#[test]
fn parse_us_re09_shaped_phillips_unrolls_lags() {
    let src = "\
@#define lags = 1:16
model;
p = lambda*( + z +
 @#for lag in lags
   +EXPECTATION(-@{lag})(z)*((1-lambda)^(@{lag}))
 @#endfor
);
end;
";
    let model = parse(src);
    assert_eq!(model.equations.len(), 1);
    let text = &model.equations[0].text;
    assert!(text.contains("EXPECTATION(-1)"), "text was {text:?}");
    assert!(text.contains("EXPECTATION(-16)"), "text was {text:?}");
}

#[test]
fn zlb_qe_f16_name_and_f17_body() {
    let model = parse(&read_mod(&copilot_archive("zlb_qe")));
    let f16 = model
        .equations
        .iter()
        .find(|e| e.name == "F16 Taylor rule (no ZLB)")
        .expect("F16 no-ZLB name");
    assert!(
        !f16.name.contains("ZLB binds") && f16.name.contains("no ZLB"),
        "F16 name {}",
        f16.name
    );
    let f17 = model
        .equations
        .iter()
        .find(|e| e.name == "F17 QE rule")
        .expect("F17 name");
    assert!(f17.text.contains("qe = 0"), "F17 text {}", f17.text);
    assert!(!f17.text.contains("rho_qe"), "F17 text {}", f17.text);
}

#[test]
fn us_re09_rep_unrolls_expectation_minus_sixteen() {
    let model = parse(&read_mod(&copilot_example("US_RE09_rep")));
    assert!(
        model
            .equations
            .iter()
            .any(|e| e.text.contains("EXPECTATION(-16)")),
        "no equation contained EXPECTATION(-16): {:?}",
        model
            .equations
            .iter()
            .map(|e| e.text.as_str())
            .collect::<Vec<_>>()
    );
}
