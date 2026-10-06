use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use dygnosis::diagnostic::analyze;
use dygnosis::expand::expand_report;
use dygnosis::lexer::{tokenize, Token, TokenKind};
use dygnosis::macro_expand::expand_macros;
use dygnosis::parse;
use dygnosis::span::{LineIndex, Position};
use dygnosis::JsonStage;
use serde_json::{json, Value};
use tower_lsp::lsp_types::*;
use tower_lsp::LanguageServer;

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
fn range_bound_arithmetic_binds_tighter_than_colon() {
    let src = action_fixture("range_arithmetic");
    let report = expand_report(&src);
    assert_eq!(report.n_equations, 2, "{}", report.effective_text);
    assert!(
        !report.effective_text.contains("y_3"),
        "{}",
        report.effective_text
    );
    let diags = analyze(&parse(&src));
    assert!(
        !diags
            .iter()
            .any(|d| matches!(d.code.as_str(), "E285" | "I211" | "W013")),
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
fn incomplete_macro_analysis_is_visible_in_formatted_lines_and_mcp_diagnosis() {
    let src = action_fixture("unsupported_builtin");
    let path = "tests/fixtures/macro_action/unsupported_builtin.mod";
    let set = dygnosis::check_file_with_origins(&src, path);
    let note = set
        .diagnostics
        .iter()
        .find(|diag| diag.code == "I211")
        .expect("incomplete-analysis Information");
    assert_eq!(note.span.start, src.find("@#define").unwrap() as u32);
    let printed = dygnosis::format_check_lines_with_origins(path, &set, &src);
    assert!(printed.contains("INFO [I211]"), "{printed}");
    assert!(!printed.contains("No issues found"), "{printed}");
    let mcp = dygnosis::mcp::dynare_diagnose(&src, None, None);
    let item = mcp
        .iter()
        .find(|diag| diag.code == "I211")
        .expect("MCP I211");
    assert_eq!((item.line, item.column), (2, 1));
}

#[test]
fn incomplete_macro_with_load_file_withholds_untrusted_workspace_facts() {
    let dir = (0..1024)
        .map(|n| {
            std::env::temp_dir().join(format!(
                "dygnosis_incomplete_load_{}_{}",
                std::process::id(),
                n
            ))
        })
        .find(|path| std::fs::create_dir(path).is_ok())
        .expect("unique temporary workspace");
    let root = dir.join("case.mod");
    let values = dir.join("values.txt");
    let root_key = root.to_string_lossy().into_owned();
    let values_key = values.to_string_lossy().into_owned();
    let incomplete = "@#define n = length([1])\nparameters p_@{n};\nvar y;\nmodel; y=p_@{n}*y(-1); end;\nload_params_and_steady_state('values.txt');\n";
    let complete = "parameters p_1; p_1=0.5;\nvar y;\nmodel; y=p_1*y(-1); end;\nload_params_and_steady_state('values.txt');\n";

    let disk_codes = |source: &str| {
        std::fs::write(&root, source).expect("write model");
        let body =
            dygnosis::dynare_workspace_diagnose(None, None, Some(std::slice::from_ref(&root_key)))
                .expect("path diagnosis");
        let codes = body["roots"][0]["diagnostics"]
            .as_array()
            .expect("diagnostics")
            .iter()
            .map(|diag| {
                (
                    diag["code"].as_str().unwrap().to_string(),
                    diag["message"].as_str().unwrap().to_string(),
                )
            })
            .collect::<Vec<_>>();
        (body["summary"]["errors"].as_u64().unwrap(), codes)
    };
    let mapped = |source: &str, companion: Option<&str>| {
        let mut files = std::collections::HashMap::from([(root_key.clone(), source.to_string())]);
        if let Some(text) = companion {
            files.insert(values_key.clone(), text.to_string());
        }
        dygnosis::mcp::dynare_diagnose(source, Some(&root_key), Some(&files))
    };

    std::fs::write(&values, "p_1 0.5\n").expect("write parameter values");
    let (incomplete_errors, incomplete_codes) = disk_codes(incomplete);
    assert_eq!(incomplete_errors, 0, "{incomplete_codes:?}");
    assert!(
        incomplete_codes.iter().any(|(code, _)| code == "I211"),
        "{incomplete_codes:?}"
    );
    assert!(
        incomplete_codes.iter().all(|(code, _)| code != "W204"),
        "{incomplete_codes:?}"
    );
    let mapped_incomplete = mapped(incomplete, Some("p_1 0.5\n"));
    assert_eq!(mapped_incomplete.len(), 1, "{mapped_incomplete:?}");
    assert_eq!(mapped_incomplete[0].code, "I211");

    // The pinned preprocessor skips this false branch. Until `length` can be
    // evaluated here, the workspace must not claim its file is missing.
    let uncertain_branch = "@#if length([])\nload_params_and_steady_state('missing_values.txt');\n@#endif\nvar y; model; y=0; end;\n";
    let (branch_errors, branch_codes) = disk_codes(uncertain_branch);
    assert_eq!(branch_errors, 0, "{branch_codes:?}");
    assert!(
        branch_codes.iter().any(|(code, _)| code == "I211"),
        "{branch_codes:?}"
    );
    assert!(
        branch_codes.iter().all(|(code, _)| code != "E306"),
        "{branch_codes:?}"
    );
    let mapped_branch = mapped(uncertain_branch, None);
    assert_eq!(mapped_branch.len(), 1, "{mapped_branch:?}");
    assert_eq!(mapped_branch[0].code, "I211");

    std::fs::write(&values, "ghost 0.5\n").expect("write unknown name");
    let (_ghost_errors, ghost_codes) = disk_codes(complete);
    assert!(
        ghost_codes
            .iter()
            .any(|(code, message)| code == "W204" && message.contains("Unknown symbol ghost")),
        "{ghost_codes:?}"
    );
    assert!(mapped(complete, Some("ghost 0.5\n"))
        .iter()
        .any(|item| item.code == "W204"));

    let missing = complete.replace("values.txt", "missing_values.txt");
    let (_missing_errors, missing_codes) = disk_codes(&missing);
    assert!(
        missing_codes.iter().any(|(code, message)| {
            code == "E306" && message.contains("Can't open missing_values.txt")
        }),
        "{missing_codes:?}"
    );
    assert!(mapped(&missing, None)
        .iter()
        .any(|item| item.code == "E306"));
    std::fs::remove_dir_all(dir).expect("remove temporary workspace");
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
fn quoted_copy(source: &str) -> String {
    let mut definitions = String::new();
    let mut expanded = String::new();
    let mut cursor = 0;
    for (number, token) in tokenize(source)
        .into_iter()
        .filter(|token| token.kind == TokenKind::String)
        .enumerate()
    {
        let text = token.text(source);
        assert!(text.starts_with('\'') && text.ends_with('\''));
        let value = &text[1..text.len() - 1];
        assert!(!value.contains('"'));
        definitions.push_str(&format!("@#define quoted{number}=\"{value}\"\n"));
        expanded.push_str(&source[cursor..token.span.start as usize]);
        expanded.push_str(&format!("'@{{quoted{number}}}'"));
        cursor = token.span.end as usize;
    }
    assert_ne!(cursor, 0, "audit source has no quoted consumer");
    expanded.push_str(&source[cursor..]);
    definitions + &expanded
}

fn audit_fixture(file: &str) -> String {
    std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(file),
    )
    .unwrap()
}

fn pinned_binary() -> Option<PathBuf> {
    let pinned = PathBuf::from("C:/dynare/7.2/preprocessor/dynare-preprocessor.exe");
    pinned.is_file().then_some(pinned).or_else(|| {
        dygnosis::find_preprocessor(None).filter(|path| {
            path.components()
                .any(|part| part.as_os_str().to_string_lossy() == "7.2")
        })
    })
}

#[test]
fn quoted_consumer_reach_audit_keeps_shipped_fire_and_quiet_cases() {
    let binary = pinned_binary();
    if binary.is_none() {
        eprintln!("SKIP official quoted-value reach audit: Dynare 7.2 is absent");
    }
    let fires = [
        (
            "d_block/e256_tag_twice.mod",
            "E256",
            "Tag 'name' cannot be used twice",
            JsonStage::Check,
        ),
        (
            "d_block/e257_default_eq_tag.mod",
            "E257",
            "Error creating default equation tag",
            JsonStage::Transform,
        ),
        (
            "d_block/e262_mcp_lhs_not_var.mod",
            "E262",
            "mcp' tag is not a variable",
            JsonStage::Check,
        ),
        (
            "d_hank/e262_mcp_unknown.mod",
            "E262",
            "mcp' tag is not a variable",
            JsonStage::Check,
        ),
        (
            "d_block/e263_mcp_lhs_not_endo.mod",
            "E263",
            "not an endogenous variable",
            JsonStage::Check,
        ),
        (
            "d_block/e264_mcp_rhs_not_const.mod",
            "E264",
            "should be a constant",
            JsonStage::Check,
        ),
        (
            "d_block/e265_mcp_no_inequality.mod",
            "E265",
            "does not contain an inequality",
            JsonStage::Check,
        ),
        (
            "d_hank/e479_mcp.mod",
            "E479",
            "'mcp' tags are not allowed",
            JsonStage::Check,
        ),
        (
            "occbin/e172_missing_regime.mod",
            "E172",
            "is not defined",
            JsonStage::Check,
        ),
        (
            "occbin/e173_bind_no_name.mod",
            "E173",
            "must have a 'name' tag",
            JsonStage::Check,
        ),
        (
            "occbin/e175_no_equation.mod",
            "E175",
            "No equation has been declared",
            JsonStage::Check,
        ),
        (
            "occbin/e176_bind_and_relax.mod",
            "E176",
            "both in the 'bind' and 'relax' tags",
            JsonStage::Check,
        ),
        (
            "occbin/e177_regime_dup.mod",
            "E177",
            "has already been declared",
            JsonStage::Check,
        ),
        (
            "occbin/e184_dup_clause.mod",
            "E184",
            "clause is declared multiple times",
            JsonStage::Check,
        ),
        (
            "occbin/e185_bad_name.mod",
            "E185",
            "unauthorized characters",
            JsonStage::Check,
        ),
        (
            "occbin/e185_name_used.mod",
            "E185",
            "is already used",
            JsonStage::Check,
        ),
        (
            "d_pac/e433_missing_tag.mod",
            "E433",
            "looking for equation tag Missing failed",
            JsonStage::Transform,
        ),
        (
            "d_surgery/e335_tag_not_found.mod",
            "E335",
            "were not found",
            JsonStage::Transform,
        ),
        (
            "d_surgery/e337_excluded_twice.mod",
            "E337",
            "excluded twice",
            JsonStage::Transform,
        ),
        (
            "d_surgery/e256_tag_twice_surgery.mod",
            "E256",
            "Tag 'name' cannot be used twice",
            JsonStage::Check,
        ),
    ];
    let mut cases: Vec<_> = fires
        .into_iter()
        .map(|(file, code, needle, stage)| (file, audit_fixture(file), code, needle, stage))
        .collect();
    cases.extend([
        (
            "long_name/type clash",
            "var y(long_name='Output'); varexo y; model; y=1; end;".into(),
            "E030",
            "declared twice",
            JsonStage::Check,
        ),
        (
            "long_name/same type",
            "var y(long_name='Output'); var y(long_name='Output'); model; y=y(-1); end;".into(),
            "W031",
            "declared twice",
            JsonStage::Check,
        ),
        (
            "eqtags/duplicate option",
            audit_fixture("d_pac/e271_var_option.mod").replace(
                "model_name=v,model_name=w,eqtags=['Y']",
                "model_name=v,eqtags=['Y'],eqtags=['Y']",
            ),
            "E271",
            "declared twice",
            JsonStage::Check,
        ),
        (
            "filename/duplicate option",
            audit_fixture("d_block/e271_option_twice.mod").replace(
                "dsge_var, dsge_var=0.5, datafile='d.csv'",
                "datafile='d.csv', datafile='d.csv'",
            ),
            "E271",
            "declared twice",
            JsonStage::Check,
        ),
        (
            "shock label/unknown member",
            audit_fixture("d_open/e058_shock_groups_undeclared.mod").replace("g =", "'group' ="),
            "E058",
            "Unknown symbol: zzz",
            JsonStage::Check,
        ),
        (
            "shock label/wrong type",
            audit_fixture("d_open/e333_shock_groups_not_exo.mod").replace("g =", "'group' ="),
            "E333",
            "should be an exogenous variable",
            JsonStage::Check,
        ),
        (
            "shock label/duplicate",
            audit_fixture("d_writer/w205_shock_groups_label_reused.mod").replace("g1 =", "'g1' ="),
            "W205",
            "has been reused",
            JsonStage::Write,
        ),
    ]);
    for (label, source, code, needle, stage) in cases {
        let quoted = quoted_copy(&source);
        let signature = |source: &str| {
            analyze(&parse(source))
                .into_iter()
                .map(|row| (row.code, row.severity))
                .collect::<Vec<_>>()
        };
        let expected = signature(&source);
        assert!(
            expected.iter().any(|row| row.0 == code),
            "{label}: {expected:?}"
        );
        assert_eq!(signature(&quoted), expected, "{label}");
        if let Some(binary) = &binary {
            let result =
                dygnosis::run_preprocessor(&quoted, binary, None, Duration::from_secs(30), stage);
            let output = result.raw_stdout + &result.raw_stderr;
            assert!(
                output.contains(needle),
                "{label}/{code}/{stage:?}: {output}"
            );
        }
        eprintln!("quoted reach FIRE {label}: {code} at {stage:?}");
    }
    let quiets = [
        ("tags/long_name/partition", "var y(long_name='Unknown label',sector='missing'); model; [name='eq1',mcp='y > 0',label='missing'] y=y(-1); end;", JsonStage::Transform),
        ("quoted eqtags", "var y; varexo e; model; [name='Y'] y=y(-1)+e; end; var_model(model_name=v,eqtags=['Y']);", JsonStage::Transform),
        ("surgery selector", "var y z; model; [name='Y'] y=y(-1); [name='Z'] z=z(-1); end; model_remove('Y');", JsonStage::Transform),
        ("filename option", "var y; varexo e; model; y=y(-1)+e; end; varobs y; estimation(datafile='d.csv');", JsonStage::Check),
        ("shock label", "var y; varexo e; model; y=y(-1)+e; end; shock_groups; 'missing' = e; end;", JsonStage::Write),
        ("OccBin names", "var y; model; [name='Y',bind='ELB'] y=0; [name='Y',relax='ELB'] y=y(-1); end; occbin_constraints; name 'ELB'; bind y<0; relax y>0; end;", JsonStage::Check),
    ];
    for (label, source, stage) in quiets {
        let quoted = quoted_copy(source);
        let diagnostics = analyze(&parse(&quoted));
        assert!(
            !diagnostics
                .iter()
                .any(|row| row.severity == dygnosis::Severity::Error),
            "{label}: {diagnostics:?}"
        );
        let quiet_codes = [
            "E020", "E030", "W031", "E058", "E172", "E173", "E175", "E176", "E177", "E184", "E185",
            "E256", "E257", "E262", "E263", "E264", "E265", "E271", "E333", "E335", "E337", "E433",
            "E479", "W205",
        ];
        assert!(
            !diagnostics
                .iter()
                .any(|row| quiet_codes.contains(&row.code.as_str())),
            "{label}: {diagnostics:?}"
        );
        assert!(expand_report(&quoted).complete, "{label}");
        if let Some(binary) = &binary {
            let result =
                dygnosis::run_preprocessor(&quoted, binary, None, Duration::from_secs(30), stage);
            assert!(
                result.success,
                "{label}/{stage:?}: {}{}",
                result.raw_stdout, result.raw_stderr
            );
            let output = result.raw_stdout + &result.raw_stderr;
            assert!(
                !output.contains("declared twice") && !output.contains("has been reused"),
                "{label}/{stage:?}: {output}"
            );
        }
        eprintln!("quoted reach QUIET {label} at {stage:?}");
    }
}

#[test]
fn quoted_tags_long_names_and_partition_values_expand_in_execution_order() {
    let source = "@#for j in 1:2\nvar y@{j}(long_name='Output @{j}😀',sector='s@{j}');\n@#endfor\nmodel;\n@#for j in 1:2\n[name='eq@{j}',mcp='y@{j} >= 0'] y@{j}=y@{j}(-1);\n@#endfor\nend;";
    let model = parse(source);
    assert!(!model.macro_incomplete());
    assert_eq!(
        model
            .endogenous
            .iter()
            .map(|row| row.long_name.as_deref())
            .collect::<Vec<_>>(),
        [Some("Output 1😀"), Some("Output 2😀")]
    );
    assert_eq!(
        model
            .equations
            .iter()
            .map(|row| row.name.as_str())
            .collect::<Vec<_>>(),
        ["eq1", "eq2"]
    );
    assert_eq!(model.equations[0].tag_map["mcp"], "y1 >= 0");
    let report = expand_report(source);
    assert!(report.complete && report.navigation_complete);
    assert!(report.effective_text.contains("sector = 's2'"));
    assert_eq!(report.n_equations, 2);
    let selected = dygnosis::dynare_equations(source, None, None, Some("eq1"), None);
    assert_eq!(selected["equations"].as_array().unwrap().len(), 1);
    assert_eq!(selected["equations"][0]["name"], "eq1");
    let rows = &report.navigation;
    assert_eq!(
        rows[0].equation.source.segments,
        rows[1].equation.source.segments
    );
    assert_ne!(rows[0].effective_span, rows[1].effective_span);
    assert_eq!(rows[0].macro_frames[0].value.as_deref(), Some("1"));
    assert_eq!(rows[1].macro_frames[0].value.as_deref(), Some("2"));
}

#[test]
fn quoted_substitution_keeps_delimiters_empty_values_and_literal_generated_macro_text() {
    let source = "@#define n=2\n@#define empty=\"\"\n@#define literal=\"@{missing}\"\nvar y(long_name='a;@{n},@{empty}!'); model; [name='eq@{n}',label='@{\"a}b\"}',literal='@{literal}'] y=1; end; verbatim; disp(\"v@{n}\"); end;";
    let model = parse(source);
    assert!(!model.macro_incomplete());
    assert_eq!(model.endogenous[0].long_name.as_deref(), Some("a;2,!"));
    assert_eq!(model.equations[0].tag_map["label"], "a}b");
    assert_eq!(model.equations[0].tag_map["literal"], "@{missing}");
    let report = expand_report(source);
    assert!(report.complete);
    assert!(report.effective_text.contains("disp(\"v2\")"));
    assert!(!analyze(&model).iter().any(|row| row.code == "E063"));
    let source = "@#define n=2\nvar y(long_name='@{empty}'); model; [name='eq@{n}'] y=1; end;";
    assert!(analyze(&parse(source))
        .iter()
        .any(|row| row.code == "E063" && row.message == "Unknown variable empty"));
}

#[test]
fn quoted_failures_keep_proven_macro_errors_and_unsafe_valid_values_incomplete() {
    let binary = pinned_binary();
    for (expression, code, message) in [
        ("missing", "E063", "Unknown variable missing"),
        ("missing(1)", "E063", "Unknown function missing"),
        (
            "1+\"text\"",
            "E285",
            "Type mismatch for operands of + operator",
        ),
    ] {
        for source in [
            format!("var y; model; [name='eq@{{{expression}}}'] y=1; end;"),
            format!("var y(long_name='Output @{{{expression}}}'); model; y=1; end;"),
            format!("var y; model; y=1; end; initval_file(filename='@{{{expression}}}.csv');"),
        ] {
            let diagnostics = analyze(&parse(&source));
            let diagnostic = diagnostics.iter().find(|row| row.code == code).unwrap();
            assert_eq!(diagnostic.message, message);
            assert_eq!(
                &source[diagnostic.span.start as usize..diagnostic.span.end as usize],
                format!("@{{{expression}}}")
            );
            assert!(!diagnostics.iter().any(|row| row.code == "I211"));
            assert!(!expand_report(&source).complete);
            if let Some(binary) = &binary {
                let result = dygnosis::run_preprocessor(
                    &source,
                    binary,
                    None,
                    Duration::from_secs(30),
                    JsonStage::Check,
                );
                assert!(!result.success);
                assert!((result.raw_stdout + &result.raw_stderr).contains(message));
            }
        }
    }
    for source in [
        "@#define v=\"left',sector='right\"\nvar y(long_name='@{v}'); model; y=1; end;",
        "var y; model; [name='eq@{length([1,2])}'] y=1; end;",
        "var y; model; [name='eq@{missing'] y=1; end;",
    ] {
        let report = expand_report(source);
        assert!(!report.complete, "{source}");
        assert_eq!(report.navigation, []);
        let diagnostics = analyze(&parse(source));
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert_eq!(diagnostics[0].code, "I211");
        assert_eq!(
            dygnosis::dynare_equations(source, None, None, None, None)["status"],
            "incomplete"
        );
    }
    let inactive = "@#if 0\nvar z(long_name='@{missing}');\n@#endif\nvar y; model; y=1; end;";
    assert!(expand_report(inactive).complete);
    assert!(!analyze(&parse(inactive))
        .iter()
        .any(|row| row.code == "E063"));

    let unsafe_valid =
        "@#define v=\"left',sector='right\"\nvar y(long_name='@{v}'); model; y=1; end;";
    if let Some(binary) = &binary {
        let result = dygnosis::run_preprocessor(
            unsafe_valid,
            binary,
            None,
            Duration::from_secs(30),
            JsonStage::Check,
        );
        assert!(result.success, "{}{}", result.raw_stdout, result.raw_stderr);
    }
    let unsafe_then_unknown =
        "@#define v=\"left',sector='right\"\nvar y(long_name='@{v}@{missing}'); model; y=1; end;";
    assert!(analyze(&parse(unsafe_then_unknown))
        .iter()
        .any(|row| row.code == "E063"));
}

fn lsp_slice<'a>(text: &'a str, range: &Value) -> &'a str {
    let index = LineIndex::new(text);
    let offset = |point: &Value| {
        index.offset_utf16(
            text,
            Position {
                line: point["line"].as_u64().unwrap() as u32,
                character: point["character"].as_u64().unwrap() as u32,
            },
        ) as usize
    };
    &text[offset(&range["start"])..offset(&range["end"])]
}

#[tokio::test]
async fn quoted_include_copies_keep_readable_ranges_frames_and_written_source_ownership() {
    let source = "var y1 y2; model;\r\n@#include \"body.inc\"\r\nend;";
    let body = "@#for j in 1:2\r\n[name='eq@{j}',label='😀@{j}'] y@{j}=1;\r\n@#endfor\r\n";
    let root = Url::parse("file:///C:/dygnosis-quoted/root.mod").unwrap();
    let include = Url::parse("file:///C:/dygnosis-quoted/body.inc").unwrap();
    let (service, _socket) = dygnosis::server::new_service();
    let backend = service.inner();
    for (uri, text) in [(&root, source), (&include, body)] {
        backend
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
    let command = |layout: bool| ExecuteCommandParams {
        command: "dynare/showEffectiveModel".into(),
        arguments: vec![if layout {
            json!({"root_uri":root,"layout":"readable"})
        } else {
            json!({"root_uri":root})
        }],
        work_done_progress_params: Default::default(),
    };
    let readable = backend
        .execute_command(command(true))
        .await
        .unwrap()
        .unwrap();
    let compact = backend
        .execute_command(command(false))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(readable["complete"], true, "{readable}");
    assert!(readable["effective_text"]
        .as_str()
        .unwrap()
        .contains("\n    [name = 'eq2', label = '😀2'] y2 = 1 ;"));
    let rows = readable["navigation"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    for (number, row) in rows.iter().enumerate() {
        assert_eq!(
            lsp_slice(
                readable["effective_text"].as_str().unwrap(),
                &row["effective_range"]
            ),
            format!(
                "[name = 'eq{}', label = '😀{}'] y{} = 1",
                number + 1,
                number + 1,
                number + 1
            )
        );
        assert_eq!(row["written_locations"][0]["uri"], include.as_str());
        assert!(lsp_slice(body, &row["written_locations"][0]["range"]).contains("name='eq@{j}'"));
        assert_eq!(row["macro_frames"][0]["value"], (number + 1).to_string());
        assert_eq!(
            row["written_locations"],
            compact["navigation"][number]["written_locations"]
        );
    }
    assert_eq!(rows[0]["written_locations"], rows[1]["written_locations"]);
    assert_eq!(
        backend
            .execute_command(command(true))
            .await
            .unwrap()
            .unwrap(),
        readable
    );
    let files = HashMap::from([
        (root.to_string(), source.to_string()),
        (include.to_string(), body.to_string()),
    ]);
    assert_eq!(
        dygnosis::dynare_equations(source, Some(root.as_str()), Some(&files), Some("eq1"), None)
            ["equations"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let broken_body = body.replace("eq@{j}", "eq@{missing}");
    let files = HashMap::from([
        (root.to_string(), source.to_string()),
        (include.to_string(), broken_body),
    ]);
    let diagnostics = dygnosis::dynare_diagnose(source, Some(root.as_str()), Some(&files));
    assert!(diagnostics
        .iter()
        .any(|row| row.code == "E063" && row.file.as_deref() == Some(include.as_str())));
}

fn membership_branch(condition: &str) -> String {
    format!(
        "var y;\n@#if ({condition})\ny_hit = 1;\n@#else\ny_miss = 1;\n@#endif\nmodel; y=1; end;"
    )
}

fn assert_membership_branch(condition: &str, expect_hit: bool) {
    let source = membership_branch(condition);
    let report = expand_report(&source);
    assert!(
        report.complete,
        "{condition}: incomplete {}",
        report.effective_text
    );
    let hit = report.effective_text.contains("y_hit");
    let miss = report.effective_text.contains("y_miss");
    assert_eq!(hit, expect_hit, "{condition}: {}", report.effective_text);
    assert_eq!(!miss, expect_hit, "{condition}: {}", report.effective_text);
    assert!(
        !analyze(&parse(&source))
            .iter()
            .any(|row| row.code == "I211"),
        "{condition}: unexpected I211"
    );
}

fn assert_membership_incomplete(condition: &str) {
    let source = membership_branch(condition);
    let report = expand_report(&source);
    assert!(
        !report.complete,
        "{condition}: expected incomplete {}",
        report.effective_text
    );
    let diagnostics = analyze(&parse(&source));
    assert!(
        diagnostics.iter().any(|row| row.code == "I211"),
        "{condition}: expected I211, got {diagnostics:?}"
    );
    assert!(
        !diagnostics.iter().any(|row| row.code == "E285"),
        "{condition}: unexpected E285 {diagnostics:?}"
    );
}

#[test]
fn macro_membership_hits_misses_types_and_nesting() {
    assert_membership_branch(r#""8" in ["8"]"#, true);
    assert_membership_branch(r#""7" in ["8"]"#, false);
    assert_membership_branch("1 in []", false);
    assert_membership_branch("1 in [1]", true);
    assert_membership_branch(r#""a" in [1, "a", true]"#, true);
    assert_membership_branch("true in [1]", false);
    assert_membership_branch(r#""1" in [1]"#, false);
    assert_membership_branch("1 in [1.0]", true);
    assert_membership_branch("true in [true, false]", true);
    assert_membership_branch("[1] in [[1], [2]]", true);
    assert_membership_branch("2 in (1,2)", true);
    assert_membership_branch("3 in (1,2)", false);
    // A still-Range right operand is incomplete until range materialization.
    assert_membership_incomplete("1 in 1:3");
    assert_membership_incomplete("0 in 1:3");
    assert_membership_incomplete("1.0 in 1:3");
    assert_membership_branch("1 in [1,2,3]", true);
    assert_membership_branch("1 in [1:3]", false);
    assert_membership_branch("(1:3) in [1:3]", true);
    assert_membership_branch("(1:3) in [[1,2,3]]", false);
    assert_membership_branch("(1:3) in [1,2,3]", false);
    assert_membership_branch("1+1 in [2]", true);
    assert_membership_branch("1 in [1] == true", true);
    assert_membership_branch("false == 1 in [1]", false);
    assert_membership_branch(r#"("8") in (["8"])"#, true);

    let word = "@#define inside = 1\nvar y;\n@#if (inside)\ny_hit = 1;\n@#else\ny_miss = 1;\n@#endif\nmodel; y=1; end;";
    let report = expand_report(word);
    assert!(report.complete);
    assert!(report.effective_text.contains("y_hit"));
    assert!(!analyze(&parse(word)).iter().any(|row| row.code == "I211"));
}

#[test]
fn macro_membership_in_define_interpolation_function_and_for_collection() {
    let define = "@#define possible_signals = [\"0\", \"1\", \"8\"]\n@#define has8 = \"8\" in possible_signals\nvar y;\n@#if (has8)\ny_hit = 1;\n@#else\ny_miss = 1;\n@#endif\nmodel; y=1; end;";
    assert!(expand_report(define).effective_text.contains("y_hit"));

    let interp = "@#define a=[\"8\"]\nvar y; model; y=@{\"8\" in a}; end;";
    let report = expand_report(interp);
    assert!(report.complete, "{}", report.effective_text);
    assert_eq!(parse(interp).equations[0].rhs.trim(), "true");

    let function = "@#define f(x) = x in [1,2]\nvar y;\n@#if (f(2))\ny_hit = 1;\n@#else\ny_miss = 1;\n@#endif\nmodel; y=1; end;";
    assert!(expand_report(function).effective_text.contains("y_hit"));

    let for_coll = "@#define xs = [1,2]\nvar y; model;\n@#for j in [1 in xs, 3]\ny=@{j};\n@#endfor\nend;";
    let report = expand_report(for_coll);
    assert!(report.complete, "{}", report.effective_text);
    assert!(report.effective_text.contains("y = true"));
    assert!(report.effective_text.contains("y = 3"));

    let plain_for = "var y; model;\n@#for j in 1:2\ny@{j}=1;\n@#endfor\nend;";
    let report = expand_report(plain_for);
    assert!(report.complete, "{}", report.effective_text);
    assert!(report.effective_text.contains("y1 = 1"));
    assert!(report.effective_text.contains("y2 = 1"));
    assert!(!analyze(&parse(plain_for))
        .iter()
        .any(|row| matches!(row.code.as_str(), "I211" | "E062" | "E285")));
}

#[test]
fn macro_membership_possible_signals_style_takes_true_branch_without_i211() {
    let source = r#"
@#define possible_signals = ["0", "1", "2", "3", "4", "5", "6", "7", "8"]
var y;
@#if ("8" in possible_signals)
y_eight = 1;
@#else
y_no_eight = 1;
@#endif
@#if ("9" in possible_signals)
y_nine = 1;
@#else
y_no_nine = 1;
@#endif
model; y=1; end;
"#;
    let report = expand_report(source);
    assert!(report.complete, "{}", report.effective_text);
    assert!(report.effective_text.contains("y_eight"));
    assert!(!report.effective_text.contains("y_no_eight"));
    assert!(report.effective_text.contains("y_no_nine"));
    assert!(!report.effective_text.contains("y_nine"));
    assert!(!analyze(&parse(source))
        .iter()
        .any(|row| row.code == "I211"));
}

#[test]
fn macro_membership_refusals_keep_pinned_sentences() {
    let string_right = membership_branch(r#"1 in "abc""#);
    let diagnostics = analyze(&parse(&string_right));
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0].code, "E285");
    assert_eq!(
        diagnostics[0].message,
        "Second argument of `in` operator must be an array"
    );
    assert!(!expand_report(&string_right).complete);

    let int_right = membership_branch("1 in 2");
    let diagnostics = analyze(&parse(&int_right));
    assert_eq!(diagnostics[0].code, "E285");
    assert_eq!(
        diagnostics[0].message,
        "Second argument of `in` operator must be an array"
    );

    let chain = membership_branch("1 in [1] in [true]");
    let diagnostics = analyze(&parse(&chain));
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0].code, "E062");
    assert_eq!(diagnostics[0].message, "syntax error, unexpected IN");
    assert!(!expand_report(&chain).complete);

    let unknown = membership_branch("1 in missing");
    let diagnostics = analyze(&parse(&unknown));
    assert!(
        diagnostics
            .iter()
            .any(|row| row.code == "E063" && row.message == "Unknown variable missing"),
        "{diagnostics:?}"
    );
    assert!(!diagnostics.iter().any(|row| row.code == "I211"));

    let huge = membership_branch("1 in 1:10001");
    let diagnostics = analyze(&parse(&huge));
    assert_eq!(diagnostics[0].code, "I211");
    assert!(!expand_report(&huge).complete);
}

#[test]
fn macro_membership_honesty_and_reach_audit() {
    let Some(binary) = pinned_binary() else {
        eprintln!("SKIP membership honesty: Dynare 7.2 is absent");
        return;
    };

    // Dynare materializes `1:3` to an array before `contains`, so `1 in 1:3`
    // hits there. This evaluator leaves a still-Range right operand incomplete.
    for (condition, expect_hit) in [
        (r#""8" in ["8"]"#, true),
        (r#""7" in ["8"]"#, false),
        ("2 in (1,2)", true),
        ("1 in 1:3", true),
        ("1 in [1:3]", false),
        ("1+1 in [2]", true),
    ] {
        let source = membership_branch(condition);
        let stem = condition
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
            .collect::<String>();
        let mod_path = std::env::temp_dir().join(format!("dyg-membership-{stem}.mod"));
        let out = std::env::temp_dir().join(format!("dyg-membership-{stem}.out"));
        std::fs::write(&mod_path, &source).unwrap();
        let _ = std::fs::remove_file(&out);
        let output = std::process::Command::new(&binary)
            .arg(&mod_path)
            .arg("onlymacro")
            .arg(format!("savemacro={}", out.display()))
            .output()
            .expect("run preprocessor");
        assert!(
            output.status.success(),
            "{condition}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let saved = std::fs::read_to_string(&out).unwrap_or_default();
        assert_eq!(saved.contains("y_hit"), expect_hit, "{condition}: {saved}");
        assert_eq!(saved.contains("y_miss"), !expect_hit, "{condition}: {saved}");
        let _ = std::fs::remove_file(&mod_path);
        let _ = std::fs::remove_file(&out);
    }

    let cases = [
        (
            "string-right",
            membership_branch(r#"1 in "abc""#),
            "E285",
            "Second argument of `in` operator must be an array",
            true,
        ),
        (
            "chain",
            membership_branch("1 in [1] in [true]"),
            "E062",
            "syntax error, unexpected IN",
            true,
        ),
        (
            "unknown",
            membership_branch("1 in missing"),
            "E063",
            "Unknown variable missing",
            true,
        ),
        (
            "hit-quiet",
            membership_branch(r#""8" in ["8"]"#),
            "E285",
            "Second argument of `in` operator must be an array",
            false,
        ),
    ];
    for (label, source, code, needle, should_fire) in cases {
        let ours = analyze(&parse(&source));
        let ours_hit = ours
            .iter()
            .any(|row| row.code == code && row.message.contains(needle));
        assert_eq!(ours_hit, should_fire, "{label}: {ours:?}");

        let result = dygnosis::run_preprocessor(
            &source,
            &binary,
            None,
            Duration::from_secs(30),
            JsonStage::Check,
        );
        let text = result.raw_stdout.clone() + &result.raw_stderr;
        if should_fire {
            assert!(!result.success, "{label}: expected refuse");
            assert!(text.contains(needle), "{label}: {text}");
        } else {
            assert!(result.success, "{label}: {text}");
            assert!(!text.contains(needle), "{label}: {text}");
        }
    }

    // Reach audit: membership entry points vs shipped generic codes.
    let surfaces = [
        (
            "@#if",
            "@#define a=[1]\nvar y;\n@#if (MISSING in a)\ny=1;\n@#endif\nmodel; y=1; end;",
            "E063",
            "Unknown variable MISSING",
        ),
        (
            "@#define",
            "@#define x = 1 in \"abc\"\nvar y; model; y=1; end;",
            "E285",
            "Second argument of `in` operator must be an array",
        ),
        (
            "@{}",
            "var y; model; y=@{1 in \"abc\"}; end;",
            "E285",
            "Second argument of `in` operator must be an array",
        ),
        (
            "function-body",
            "@#define f(x)=x in \"abc\"\nvar y; model; y=@{f(1)}; end;",
            "E285",
            "Second argument of `in` operator must be an array",
        ),
        (
            "@#for-collection",
            "var y; model;\n@#for j in (1 in \"abc\")\ny=1;\n@#endfor\nend;",
            "E285",
            "Second argument of `in` operator must be an array",
        ),
        (
            "@#if-chain",
            "var y;\n@#if (1 in [1] in [true])\ny=1;\n@#endif\nmodel; y=1; end;",
            "E062",
            "syntax error, unexpected IN",
        ),
    ];
    for (surface, source, code, needle) in surfaces {
        let diagnostics = analyze(&parse(source));
        assert!(
            diagnostics
                .iter()
                .any(|row| row.code == code && row.message.contains(needle)),
            "{surface}: expected {code} ({needle}); got {diagnostics:?}"
        );
        let result = dygnosis::run_preprocessor(
            source,
            &binary,
            None,
            Duration::from_secs(30),
            JsonStage::Check,
        );
        let text = result.raw_stdout + &result.raw_stderr;
        assert!(!result.success, "{surface}: Dynare should refuse");
        assert!(
            text.contains(needle),
            "{surface}: Dynare text missing {needle}: {text}"
        );
    }
}
