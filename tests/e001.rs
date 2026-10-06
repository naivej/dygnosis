use std::path::PathBuf;

use dygnosis::lexer::{tokenize, TokenKind};
use dygnosis::span::LineIndex;
use dygnosis::{analyze, auto_fix, check_parse, has_structural_error, parse, Severity, TextEdit};

const ARCHIVES: &[&str] = &[
    "trend_rbc_gov_inv",
    "sims_wu_2019",
    "govt_rbc_irf_matching",
    "lk2024",
];

#[derive(Debug, Clone, PartialEq, Eq)]
struct Diag {
    code: String,
    severity: i32,
    message: String,
    start_line: u32,
    start_char: u32,
    end_line: u32,
    end_char: u32,
    fix: Option<TextEdit>,
}

fn copilot_mod(archive_dir: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(".agents/skills/dynare-copilot/references/model-archive")
        .join(archive_dir)
        .join(format!("{archive_dir}.mod"))
}

fn read_mod(archive_dir: &str) -> String {
    let path = copilot_mod(archive_dir);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("fixture missing at {}: {e}", path.display()))
        .replace("\r\n", "\n")
}

fn check_mod(rel: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(rel);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("fixture missing at {}: {e}", path.display()))
        .replace("\r\n", "\n")
}

fn rust_e001(text: &str) -> Vec<Diag> {
    let model = parse(text);
    let index = LineIndex::new(&model.source);
    check_parse(&model)
        .into_iter()
        .map(|d| {
            let start = index.position(&model.source, d.span.start);
            let end = index.position(&model.source, d.span.end);
            Diag {
                code: d.code,
                severity: d.severity as i32,
                message: d.message,
                start_line: start.line,
                start_char: start.character,
                end_line: end.line,
                end_char: end.character,
                fix: d.fix,
            }
        })
        .collect()
}

fn missing_semi_only(rows: Vec<Diag>) -> Vec<Diag> {
    rows.into_iter()
        .filter(|d| !d.message.contains("merged due to a missing semicolon"))
        .collect()
}

fn range_of(text: &str, needle: &str) -> (u32, u32, u32, u32) {
    let start = text
        .find(needle)
        .unwrap_or_else(|| panic!("missing {needle:?}"));
    assert_eq!(
        text.matches(needle).count(),
        1,
        "needle {needle:?} must occur once"
    );
    let end = (start + needle.len()) as u32;
    let index = LineIndex::new(text);
    let s = index.position(text, start as u32);
    let e = index.position(text, end);
    (s.line, s.character, e.line, e.character)
}

fn range_in(text: &str, context: &str, inner: &str) -> (u32, u32, u32, u32) {
    let at = text
        .find(context)
        .unwrap_or_else(|| panic!("missing context {context:?}"));
    let rel = text[at..at + context.len()]
        .find(inner)
        .unwrap_or_else(|| panic!("missing {inner:?} in {context:?}"));
    let start = (at + rel) as u32;
    let end = start + inner.len() as u32;
    let index = LineIndex::new(text);
    let s = index.position(text, start);
    let e = index.position(text, end);
    (s.line, s.character, e.line, e.character)
}

fn assert_span(text: &str, d: &Diag, needle: &str) {
    assert_eq!(
        (d.start_line, d.start_char, d.end_line, d.end_char),
        range_of(text, needle),
        "span should be {needle:?}, message={}",
        d.message
    );
}

fn assert_span_in(text: &str, d: &Diag, context: &str, inner: &str) {
    assert_eq!(
        (d.start_line, d.start_char, d.end_line, d.end_char),
        range_in(text, context, inner),
        "span should be {inner:?} in {context:?}, message={}",
        d.message
    );
}

fn assert_fire(rel: &str, msg_sub: &str, needle: &str) {
    let text = check_mod(rel);
    let got = rust_e001(&text);
    assert_eq!(got.len(), 1, "{rel}: {got:?}");
    assert_eq!(got[0].code, "E001");
    assert_eq!(got[0].severity, 1);
    assert!(
        got[0].message.contains(msg_sub),
        "{rel}: missing {msg_sub:?} in {}",
        got[0].message
    );
    assert_span(&text, &got[0], needle);
}

#[test]
fn e001_clean_archive_files_are_empty() {
    for name in ARCHIVES {
        let text = read_mod(name);
        let rust = rust_e001(&text);
        assert!(rust.is_empty(), "{name}: expected no E001, got {rust:?}");
        let model = parse(&text);
        assert!(
            !has_structural_error(&model),
            "{name} should not have structural errors"
        );
    }
}

#[test]
fn e001_delete_model_end() {
    let text = check_mod("e001/delete_model_end.mod");
    let got = rust_e001(&text);
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].code, "E001");
    assert!(got[0].message.contains("Missing 'end;' for 'model'"));
    assert_span(&text, &got[0], "model;");
    assert!(has_structural_error(&parse(&text)));
}

#[test]
fn e001_delete_ss_end() {
    assert_fire(
        "e001/delete_ss_end.mod",
        "Missing 'end;' for 'steady_state_model'",
        "steady_state_model;",
    );
}

#[test]
fn e001_delete_shocks_end() {
    assert_fire(
        "e001/delete_shocks_end.mod",
        "Missing 'end;' for 'shocks'",
        "shocks;",
    );
}

#[test]
fn e001_delete_occbin_end() {
    assert_fire(
        "e001/delete_occbin_end.mod",
        "Missing 'end;' for 'occbin_constraints'",
        "occbin_constraints;",
    );
}

#[test]
fn e001_delete_matched_moments_end() {
    assert_fire(
        "e001/delete_matched_moments_end.mod",
        "Missing 'end;' for 'matched_moments'",
        "matched_moments;",
    );
}

#[test]
fn e001_strip_var_semicolon() {
    assert_fire(
        "e001/strip_var_semi.mod",
        "Declaration 'var' appears to be missing its terminating semicolon",
        "var ",
    );
}

#[test]
fn e001_strip_betta_semicolon() {
    let text = check_mod("e001/strip_betta_semi.mod");
    let got = missing_semi_only(rust_e001(&text));
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].code, "E001");
    assert!(got[0]
        .message
        .contains("Parameter assignment 'betta' is missing its terminating semicolon"));
    assert_span(&text, &got[0], "betta = 0.99");
}

#[test]
fn e001_strip_last_model_equation_semicolon() {
    assert_fire(
        "e001/strip_last_eq.mod",
        "Statement in 'model' block is missing its terminating semicolon",
        "y = rho * y(-1) + e",
    );
}

#[test]
fn e001_join_two_model_equations() {
    let text = check_mod("e001/join_two_eqs.mod");
    let got = rust_e001(&text);
    assert_eq!(got.len(), 1);
    assert!(got[0].message.contains("merged due to a missing semicolon"));
    assert_span(&text, &got[0], "y = rho * y(-1) + e\nc = betta * c(+1)");
}

#[test]
fn e001_join_two_param_assignments() {
    let text = check_mod("e001/join_two_params.mod");
    let got = missing_semi_only(rust_e001(&text));
    assert_eq!(got.len(), 1);
    assert!(got[0]
        .message
        .contains("Parameter assignment 'betta' is missing its terminating semicolon"));
    assert_span(&text, &got[0], "betta = 0.99");
}

#[test]
fn e001_cmd_option_commas_is_quiet() {
    let text = check_mod("e001/cmd_option_commas.mod");
    let got = rust_e001(&text);
    assert!(
        got.is_empty(),
        "catalog and 0.1 option-list commas must not be E001, got {got:?}"
    );
    assert!(!has_structural_error(&parse(&text)));
    assert_eq!(
        auto_fix(&text),
        text,
        "auto_fix must be identity on option lists"
    );
}

#[test]
fn e001_assign_before_cmd_still_missing_semi() {
    let text = check_mod("e001/assign_before_cmd.mod");
    let got = missing_semi_only(rust_e001(&text));
    assert_eq!(got.len(), 1, "assign_before_cmd: {got:?}");
    assert_eq!(got[0].code, "E001");
    assert!(got[0]
        .message
        .contains("Parameter assignment 'scale' is missing its terminating semicolon"));
    assert_span(&text, &got[0], "scale = 1");
}

#[test]
fn e001_cmd_name_lhs_still_missing_semi() {
    let text = check_mod("e001/cmd_name_lhs.mod");
    let got = missing_semi_only(rust_e001(&text));
    assert_eq!(got.len(), 1, "cmd_name_lhs: {got:?}");
    assert_eq!(got[0].code, "E001");
    assert!(got[0]
        .message
        .contains("Parameter assignment 'data' is missing its terminating semicolon"));
    assert_span(&text, &got[0], "data = 0.50");
}

#[test]
fn e001_var_cmd_name_is_not_reserved() {
    let text = check_mod("e001/var_cmd_name.mod");
    let got = rust_e001(&text);
    assert!(
        got.is_empty(),
        "var method_of_moments must not gain reserved E001, got {got:?}"
    );
}

#[test]
fn e001_shocks_var_missing_semicolon() {
    assert_fire(
        "e001/shocks_var_no_semi.mod",
        "Missing semicolon in shocks block before 'stderr'",
        "var e ",
    );
}

#[test]
fn e001_keyword_typo_mdoel() {
    let text = check_mod("e001/typo_mdoel.mod");
    let got = rust_e001(&text);
    assert_eq!(got.len(), 1);
    assert!(got[0]
        .message
        .contains("Possible misspelling of 'model' keyword: 'mdoel'"));
    assert_span_in(&text, &got[0], "mdoel;", "mdoel");
}

#[test]
fn e001_variable_inside_model_is_not_var_typo() {
    let text = check_mod("e001/variable_ident.mod");
    let rust = rust_e001(&text);
    assert!(
        rust.iter()
            .all(|d| !d.message.contains("Possible misspelling of 'var'")),
        "inside-block `variable` must not be a var typo: {rust:?}"
    );
}

#[test]
fn e001_unbalanced_paren() {
    assert_fire(
        "e001/unbalanced_paren.mod",
        "Unbalanced parentheses in equation: unmatched ')'",
        "y = rho * y(-1) + e))",
    );
}

#[test]
fn e001_invalid_ident_c_minus_x() {
    assert_fire(
        "e001/invalid_ident.mod",
        "Invalid Dynare identifier 'c-x'",
        "c-x",
    );
}

fn invalid_ident_messages(text: &str) -> Vec<String> {
    rust_e001(text)
        .into_iter()
        .filter(|d| d.message.contains("Invalid Dynare identifier"))
        .map(|d| d.message)
        .collect()
}

#[test]
fn incomplete_expansion_keeps_decidable_invalid_ident() {
    let text = check_mod("e001/macro_decl_incomplete_illegal.mod");
    let model = parse(&text);
    assert!(
        model.macro_incomplete(),
        "undefined @#if FOO must leave expansion incomplete"
    );
    assert!(
        model
            .endogenous
            .iter()
            .any(|d| model.name(d.name) == "bad" || model.name(d.name) == "name"),
        "parser still records active decls for bad-name tokens: {:?}",
        model
            .endogenous
            .iter()
            .map(|d| model.name(d.name))
            .collect::<Vec<_>>()
    );
    assert!(
        !model
            .endogenous
            .iter()
            .any(|d| model.name(d.name) == "x" || model.name(d.name) == "FOO"),
        "incomplete @#if must not invent names: {:?}",
        model
            .endogenous
            .iter()
            .map(|d| model.name(d.name))
            .collect::<Vec<_>>()
    );
    let invalid: Vec<_> = rust_e001(&text)
        .into_iter()
        .filter(|d| d.message.contains("Invalid Dynare identifier"))
        .collect();
    assert_eq!(invalid.len(), 1, "{invalid:?}");
    assert_eq!(invalid[0].code, "E001");
    assert!(
        invalid[0]
            .message
            .contains("Invalid Dynare identifier 'bad-name'"),
        "{invalid:?}"
    );
    assert_span(&text, &invalid[0], "bad-name");
}

#[test]
fn macro_directive_in_declaration_names() {
    let active = check_mod("e001/macro_decl_active_illegal.mod");
    let active_diags: Vec<_> = rust_e001(&active)
        .into_iter()
        .filter(|d| d.message.contains("Invalid Dynare identifier"))
        .collect();
    assert_eq!(active_diags.len(), 1, "{active_diags:?}");
    assert!(
        active_diags[0]
            .message
            .contains("Invalid Dynare identifier 'bad-name'"),
        "{active_diags:?}"
    );
    assert_span(&active, &active_diags[0], "bad-name");

    for rel in [
        "e001/macro_decl_inactive_illegal.mod",
        "e001/macro_decl_elseif_nested.mod",
        "e001/macro_decl_metadata.mod",
        "e001/macro_decl_money0.mod",
        "e001/macro_decl_money1.mod",
        "e001/macro_decl_varexo_det.mod",
        "e001/macro_decl_separate_inactive.mod",
    ] {
        let text = check_mod(rel);
        let msgs = invalid_ident_messages(&text);
        assert!(msgs.is_empty(), "{rel}: {msgs:?}");
        assert!(
            analyze(&parse(&text))
                .iter()
                .all(|d| !d.message.contains("Invalid Dynare identifier")),
            "{rel}"
        );
    }

    let for_include = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/e001/macro_decl_for_include.mod");
    let for_text = std::fs::read_to_string(&for_include)
        .unwrap()
        .replace("\r\n", "\n");
    let for_path = for_include.to_str().unwrap();
    assert!(invalid_ident_messages(&for_text).is_empty(), "{for_text}");
    let for_diags = dygnosis::check_file(&for_text, for_path);
    assert!(
        for_diags
            .iter()
            .all(|d| !d.message.contains("Invalid Dynare identifier")),
        "{for_diags:?}"
    );
    assert!(
        for_diags.iter().all(|d| d.code != "E061"),
        "resolved include must not be missing: {for_diags:?}"
    );
    let model = parse(&for_text);
    assert!(
        model.endogenous.iter().any(|d| model.name(d.name) == "y_1"),
        "loop-generated decl missing: {:?}",
        model
            .endogenous
            .iter()
            .map(|d| model.name(d.name))
            .collect::<Vec<_>>()
    );
    assert!(
        !model
            .endogenous
            .iter()
            .any(|d| model.name(d.name) == "bad" || model.name(d.name) == "name"),
        "empty @#for must not activate bad-name"
    );

    let malformed = check_mod("e001/macro_decl_malformed.mod");
    let malformed_diags = analyze(&parse(&malformed));
    assert!(
        malformed_diags.iter().any(|d| d.code == "E062"),
        "{malformed_diags:?}"
    );
    assert!(
        malformed_diags
            .iter()
            .all(|d| !d.message.contains("Invalid Dynare identifier")),
        "{malformed_diags:?}"
    );
}

#[test]
fn macro_decl_reach_audit() {
    let dup = analyze(&parse(&check_mod("e001/macro_decl_reach_dup.mod")));
    assert!(
        dup.iter().any(|d| d.code == "W031"),
        "duplicate active name must reach W031: {dup:?}"
    );
    assert!(
        dup.iter()
            .all(|d| !d.message.contains("Invalid Dynare identifier")),
        "{dup:?}"
    );

    let typed = analyze(&parse(&check_mod("e001/macro_decl_reach_type.mod")));
    assert!(
        typed.iter().any(|d| d.code == "E030"),
        "wrong-kind redeclaration must reach E030: {typed:?}"
    );

    let unknown = analyze(&parse(&check_mod("e001/macro_decl_reach_unknown.mod")));
    assert!(
        unknown
            .iter()
            .any(|d| d.code == "E020" && d.message.contains('z')),
        "unknown equation symbol must reach E020: {unknown:?}"
    );

    let list = analyze(&parse(&check_mod("e001/macro_decl_reach_list.mod")));
    assert!(
        list.iter()
            .any(|d| d.code == "E240" && d.message.contains('p')),
        "wrong-type command list must reach E240: {list:?}"
    );
    assert!(
        list.iter()
            .any(|d| d.code == "E239" && d.message.contains('z')),
        "undeclared command-list name must reach E239: {list:?}"
    );
    assert!(
        list.iter().all(|d| {
            !(d.code == "E239" && d.message.contains("Variable x ")
                || d.code == "E240" && d.message.contains("Variable x "))
        }),
        "active macro-declared x must be usable in rplot: {list:?}"
    );
}

#[test]
fn e001_non_ascii_trailing_declaration() {
    assert_fire(
        "e001/non_ascii_trailing.mod",
        "character unrecognized by lexer",
        "é",
    );
}

#[test]
fn e001_non_ascii_equation() {
    assert_fire(
        "e001/non_ascii_equation.mod",
        "character unrecognized by lexer",
        "é",
    );
}

#[test]
fn e001_non_ascii_shock() {
    assert_fire(
        "e001/non_ascii_shock.mod",
        "character unrecognized by lexer",
        "é",
    );
}

#[test]
fn e001_non_ascii_identifier() {
    assert_fire(
        "e001/non_ascii_ident.mod",
        "character unrecognized by lexer",
        "日本",
    );
}

fn lex_e001(text: &str) -> bool {
    rust_e001(text)
        .iter()
        .any(|diag| diag.message.contains("character unrecognized by lexer"))
}

#[test]
fn e001_conditional_chains_keep_only_the_selected_branch() {
    let quiet = [
        "\
@#if 0
var café;
@#elseif 1
var y;
@#elseif 1
var café;
@#else
var café;
@#endif
",
        "\
@#if 1
var y;
@#elseif 0
var café;
@#else
var café;
@#endif
",
        "\
@#if 0
var café;
@#elseif 0
var café;
@#elseif 1
var y;
@#endif
",
        "\
@#if 1
@#if 0
var café;
@#else
var y;
@#endif
@#else
var café;
@#endif
",
        "\
@#define NAME = 1
@#ifndef NAME
var café;
@#else
var y;
@#endif
",
        "\
@#ifdef MISSING
var café;
@#else
var y;
@#endif
",
    ];
    for text in quiet {
        assert!(!lex_e001(text), "{text}");
        let model = parse(text);
        assert!(
            model
                .endogenous
                .iter()
                .any(|decl| model.name(decl.name) == "y"),
            "{text}"
        );
        assert!(
            model
                .endogenous
                .iter()
                .all(|decl| !model.name(decl.name).contains('c')),
            "{text}"
        );
    }

    let selected = [
        "\
@#if 0
var y;
@#elseif 1
var café;
@#else
var y;
@#endif
",
        "\
@#if 1
@#if 1
var café;
@#endif
@#endif
",
        "\
@#define NAME = 1
@#ifdef NAME
var café;
@#endif
",
        "\
@#ifndef MISSING
var café;
@#endif
",
    ];
    for text in selected {
        assert!(lex_e001(text), "{text}");
    }
}

#[test]
fn e001_native_matlab_and_inactive_macro_stay_quiet() {
    let native = check_mod("e001/native_matlab_quiet.mod");
    let inactive = check_mod("e001/inactive_macro_quiet.mod");
    for text in [native.as_str(), inactive.as_str()] {
        let got = rust_e001(text);
        assert!(
            got.iter()
                .all(|diag| !diag.message.contains("character unrecognized by lexer")),
            "{text:?} -> {got:?}"
        );
    }
    let same_line = "\
var y;
model;
y=0;
end;
native_value = 1; café;
";
    let got = rust_e001(same_line);
    assert!(
        got.iter()
            .all(|diag| !diag.message.contains("character unrecognized by lexer")),
        "{got:?}"
    );
    let later = "\
var y;
model;
y=0;
end;
stoch_simul; var café;
";
    let got = rust_e001(later);
    assert!(
        got.iter()
            .any(|diag| diag.message.contains("character unrecognized by lexer")),
        "{got:?}"
    );
    let active = "\
@#if 1
var café;
@#endif
var y;
model;
y = 0;
end;
";
    let got = rust_e001(active);
    assert!(
        got.iter()
            .any(|diag| diag.message.contains("character unrecognized by lexer")),
        "{got:?}"
    );
}

#[test]
fn e001_unicode_display_and_complementarity_stay_quiet() {
    let text = check_mod("e001/unicode_display_quiet.mod");
    let got = rust_e001(&text);
    assert!(
        got.is_empty(),
        "unicode comments, long_name, TeX, and equation names must stay quiet, got {got:?}"
    );
    for rel in ["occbin/perp.mod", "p_hank/quiet_perp_spellings.mod"] {
        let body = check_mod(rel);
        let diags = rust_e001(&body);
        assert!(
            diags
                .iter()
                .all(|d| !d.message.contains("character unrecognized by lexer")),
            "{rel} must keep the complementarity operator, got {diags:?}"
        );
    }
    let kinds = tokenize("y = 1 ⟂ x _|_ z;");
    let perp = kinds
        .iter()
        .filter(|t| t.kind == TokenKind::Perpendicular)
        .count();
    assert_eq!(perp, 2, "both complementarity spellings stay tokens");
}

#[test]
fn e001_reserved_parameters_log() {
    let text = check_mod("e001/reserved_log.mod");
    let got = rust_e001(&text);
    assert_eq!(got.len(), 1);
    assert!(got[0].message.contains("Invalid Dynare identifier 'log'"));
    assert_span_in(&text, &got[0], "parameters log;", "log");
}

#[test]
fn e001_cascade_missing_model_end() {
    let text = check_mod("e001/delete_model_end.mod");
    let model = parse(&text);
    assert!(has_structural_error(&model));
    let rust = rust_e001(&text);
    assert_eq!(rust.len(), 1);
    assert_eq!(rust[0].code, "E001");

    let all = analyze(&model);
    assert!(all.iter().any(|d| d.code == "E001"));
    assert!(
        all.iter().all(|d| d.code == "E001"),
        "analyze() skips later families when E001 is present, got {:?}",
        all.iter().map(|d| &d.code).collect::<Vec<_>>()
    );
    for skip in ["W013", "E020", "E030", "W054", "W010"] {
        assert!(
            !all.iter().any(|d| d.code == skip),
            "analyze() cascade should suppress {skip}, got {:?}",
            all.iter().map(|d| &d.code).collect::<Vec<_>>()
        );
    }
}

// --- A declared name whose spelling is also a block opener (0.6.0 slice 06) ---
//
// The lexer scopes every opener rule to `INITIAL`, so a word inside a body reads as
// an identifier and the symbol table resolves it. `var y shocks;` therefore declares
// a second endogenous, and the file parses. The fire rows below are the shapes 7.1
// refuses, and they keep their code.

/// `var y shocks;` used in the model and in `initval`: 7.1 accepts this file.
#[test]
fn e001_opener_var_used_is_quiet() {
    for rel in [
        "e001/opener_var_used.mod",
        "e001/opener_var_used_matched_irfs.mod",
        "e001/opener_var_name_first.mod",
        "e001/opener_var_in_own_block.mod",
        "e001/opener_var_late_names.mod",
        "e001/opener_var_skipped_block.mod",
        "e001/opener_var_declared_bare_row.mod",
        "e001/opener_var_declared_bare_row_first.mod",
    ] {
        let text = check_mod(rel);
        let got = rust_e001(&text);
        assert!(got.is_empty(), "{rel}: expected no E001, got {got:?}");
        assert!(!has_structural_error(&parse(&text)));
    }
}

/// The `declared_before` clause on its own, with no assignment row to protect it: a
/// **bare** declared opener-named row inside the model body is an equation with a
/// zero right-hand side, which 7.1 accepts. Every other quiet lock in this file uses
/// a `{name} = 0.1*y;` row, so only these two fail if the clause goes.
#[test]
fn e001_opener_var_declared_bare_row_is_quiet() {
    for rel in [
        "e001/opener_var_declared_bare_row.mod",
        "e001/opener_var_declared_bare_row_first.mod",
    ] {
        let text = check_mod(rel);
        let model = parse(&text);
        let all = analyze(&model);
        let codes: Vec<&str> = all.iter().map(|d| d.code.as_str()).collect();
        assert!(
            !codes.contains(&"E001") && !codes.contains(&"E020"),
            "{rel}: a bare declared opener-named row must be quiet, got {codes:?}"
        );
        // The row is stored as its own equation, not skipped as a block opener.
        assert!(
            model.equations.iter().any(|eq| eq.text.trim() == "shocks"),
            "{rel}: the bare row should be stored as an equation, got {:?}",
            model
                .equations
                .iter()
                .map(|eq| eq.text.as_str())
                .collect::<Vec<_>>()
        );
    }
}

/// The opener-named declaration is legal syntax, but its unused endogenous
/// name reaches the later transform refusal.
#[test]
fn e001_opener_var_decl_only_reaches_transform_without_e001() {
    let text = check_mod("e001/opener_var_decl_only.mod");
    let got = rust_e001(&text);
    assert!(got.is_empty(), "expected no E001, got {got:?}");
    let all = analyze(&parse(&text));
    assert!(all.iter().any(|d| d.code == "E186"), "{all:?}");
    assert!(
        all.iter().all(|d| d.code != "W013" && d.code != "W020"),
        "{all:?}"
    );
}

/// A bare opener word inside a real `shocks` body is a row 7.1 refuses, declared or
/// not: that block's rows are `var` / `corr` / `skew` statements.
#[test]
fn e001_opener_var_bare_row_in_own_block() {
    let text = check_mod("e001/opener_var_bare_row_in_own_block.mod");
    let got = rust_e001(&text);
    assert_eq!(got.len(), 1, "{got:?}");
    assert_eq!(got[0].code, "E001");
    assert!(got[0].message.contains("Missing 'end;' for 'shocks'"));
    assert_span_in(&text, &got[0], "\nshocks;\nvar e;\n", "shocks;");
}

/// An undeclared opener-shaped name reports the neighbouring **E020**, the code
/// 7.1's own `Unknown symbol` sentence maps to.
#[test]
fn e001_opener_var_undeclared_use_is_e020() {
    for rel in [
        "e001/opener_var_undeclared_use.mod",
        "e001/opener_var_undeclared_bare_row.mod",
    ] {
        let text = check_mod(rel);
        let all = analyze(&parse(&text));
        let codes: Vec<&str> = all.iter().map(|d| d.code.as_str()).collect();
        assert!(
            codes.contains(&"E020"),
            "{rel}: expected E020, got {codes:?}"
        );
        assert!(
            !codes.contains(&"E001"),
            "{rel}: E001 is the wrong code here, got {codes:?}"
        );
    }
}

/// A declaration written after the model block does not help the equation: their
/// parser registers a name when it reads the declaration.
#[test]
fn e001_opener_var_declared_after_still_refuses() {
    let text = check_mod("e001/opener_var_declared_after.mod");
    let all = analyze(&parse(&text));
    let codes: Vec<&str> = all.iter().map(|d| d.code.as_str()).collect();
    assert!(codes.contains(&"E020"), "expected E020, got {codes:?}");
}

/// The control set: a genuinely missing `;` or `end;` keeps its **E001**.
#[test]
fn e001_opener_var_controls_still_fire() {
    for rel in [
        "e001/opener_var_decl_semi_missing.mod",
        "e001/opener_var_reserved_control.mod",
        "e001/opener_var_statement_scoped.mod",
    ] {
        let text = check_mod(rel);
        let got = rust_e001(&text);
        assert_eq!(got.len(), 1, "{rel}: {got:?}");
        assert_eq!(got[0].code, "E001");
        assert!(
            got[0]
                .message
                .contains("Declaration 'var' appears to be missing its terminating semicolon"),
            "{rel}: {}",
            got[0].message
        );
    }
    assert_fire(
        "e001/opener_var_missing_end_control.mod",
        "Missing 'end;' for 'model'",
        "model;",
    );
}

/// An `end` with no `;` still ends the body: their lexer returns to `INITIAL` on the
/// `end` word itself, so the next opener is an opener token again and 7.1 refuses
/// `unexpected INITVAL, expecting ';'`. The source strings below are one shape each;
/// the fixture is the same file on disk.
#[test]
fn e001_opener_var_end_without_semi_before_a_block() {
    assert_fire(
        "e001/opener_var_end_no_semi.mod",
        "Missing 'end;' for 'model'",
        "model;",
    );
    for (name, tail) in [
        ("shocks", "shocks;\nvar e; stderr 0.01;\nend;\n"),
        ("initval", "initval;\ny = 0;\nend;\n"),
    ] {
        let src = format!(
            "var y;\nvarexo e;\nparameters rho;\nrho = 0.5;\n\
             model;\ny = rho*y(-1)+e;\nend\n{tail}"
        );
        let text = src.clone();
        let got = rust_e001(&text);
        assert_eq!(got.len(), 1, "{name}: {got:?}");
        assert_eq!(got[0].code, "E001");
        assert!(
            got[0].message.contains("Missing 'end;' for 'model' block"),
            "{name}: {}",
            got[0].message
        );
        assert!(has_structural_error(&parse(&text)));
    }
}

/// `var y end;` declares a name. 7.1 accepts the declaration at check. `end` is
/// not a reserved identifier; a use inside a block is a different refuse.
#[test]
fn declared_end_is_not_a_reserved_identifier() {
    let text = check_mod("e001/opener_var_reserved_ident.mod");
    let got = rust_e001(&text);
    assert!(
        got.iter()
            .all(|d| !d.message.contains("Invalid Dynare identifier")),
        "{got:?}"
    );
}

/// Auto-fix is the identity on the quiet files and on the reserved-identifier one,
/// so the false **E001** this slice closes never drove an insertion, and **06b**'s
/// fixture does not gain one either.
#[test]
fn e001_opener_var_quiet_files_are_noop_for_auto_fix() {
    for rel in [
        "e001/opener_var_used.mod",
        "e001/opener_var_used_matched_irfs.mod",
        "e001/opener_var_name_first.mod",
        "e001/opener_var_in_own_block.mod",
        "e001/opener_var_late_names.mod",
        "e001/opener_var_skipped_block.mod",
        "e001/opener_var_reserved_ident.mod",
    ] {
        let text = check_mod(rel);
        assert_eq!(auto_fix(&text), text, "{rel} must be an auto_fix no-op");
    }
}

/// The 13 `<INITIAL>`-only command words are legal names. The five words that
/// also have a `<DYNARE_STATEMENT>` rule still refuse.
#[test]
fn initial_command_names_used_in_the_model_are_quiet() {
    for name in [
        "calib_smoother",
        "check",
        "dynasave",
        "dynatype",
        "estimated_params",
        "estimation",
        "model_diagnostics",
        "model_info",
        "osr",
        "perfect_foresight_setup",
        "perfect_foresight_solver",
        "resid",
        "steady",
    ] {
        let src = format!(
            "var y {name};\nvarexo e;\nparameters rho;\nrho = 0.95;\n\
             model;\ny = rho*y(-1) + e + {name};\n{name} = 0.1*y;\nend;\n\
             initval;\ny = 0;\n{name} = 0;\nend;\n\
             shocks;\nvar e; stderr 0.01;\nend;\nstoch_simul(order = 1, nograph);\n"
        );
        let errors: Vec<_> = analyze(&parse(&src))
            .into_iter()
            .filter(|d| d.severity == Severity::Error)
            .map(|d| format!("{} {}", d.code, d.message))
            .collect();
        assert!(errors.is_empty(), "{name} is a legal name, got {errors:?}");
    }
}

#[test]
fn statement_scoped_command_names_still_refuse() {
    for name in [
        "forecast",
        "identification",
        "simul",
        "stoch_simul",
        "varobs",
    ] {
        let src = format!(
            "var y {name};\nvarexo e;\nparameters rho;\nrho = 0.95;\n\
             model;\ny = rho*y(-1) + e;\nend;\n"
        );
        let got = rust_e001(&src);
        assert!(
            got.iter().any(|d| d.code == "E001"),
            "{name}: 7.1 refuses this declaration, got {got:?}"
        );
    }
}

/// `end = 0;` inside `initval` is the closer, not a row. 7.1 refuses it.
#[test]
fn end_assignment_inside_initval_is_their_syntax_error() {
    let src = "\
var y end;
varexo e;
parameters rho;
rho = 0.9;
model;
y = rho*y(-1) + e;
end;
initval;
y = 0;
end = 0;
end;
shocks;
var e; stderr 0.01;
end;
stoch_simul(order=1, irf=0, nograph);
";
    let got = rust_e001(src);
    let hit = got
        .iter()
        .find(|d| d.message.contains("unexpected IDENTIFIER"))
        .unwrap_or_else(|| panic!("{got:?}"));
    assert_eq!(
        hit.message,
        "syntax error, unexpected IDENTIFIER, expecting ';'"
    );
}

#[test]
fn empty_aggregate_model_refuses_on_end_without_transform_or_writing_notes() {
    for source in [
        "var y c; varexo e; model; end;",
        "var y; model(linear); /* empty */\r\nend;",
        "var y; model; y=0; end; model; end;",
    ] {
        let diagnostics = analyze(&parse(source));
        let errors: Vec<_> = diagnostics
            .iter()
            .filter(|diag| diag.code == "E001")
            .collect();
        assert_eq!(errors.len(), 1, "{source}: {diagnostics:?}");
        assert_eq!(errors[0].message, "syntax error, unexpected END");
        assert_eq!(errors[0].severity, Severity::Error);
        assert_eq!(errors[0].span.start as usize, source.rfind("end").unwrap());
        assert_eq!(
            &source[errors[0].span.start as usize..errors[0].span.end as usize],
            "end"
        );
        assert!(
            diagnostics
                .iter()
                .all(|diag| !matches!(diag.code.as_str(), "E186" | "I209")),
            "{diagnostics:?}"
        );
    }
    for source in [
        "var y;",
        "var y; model; y=0; end;",
        "model; # local=1; end;",
    ] {
        assert!(
            rust_e001(source).is_empty(),
            "{source}: {:?}",
            rust_e001(source)
        );
    }
}

#[test]
fn empty_aggregate_model_agrees_with_pinned_parse_refusal() {
    let pp = PathBuf::from("C:/dynare/7.2/preprocessor/dynare-preprocessor.exe");
    if !pp.is_file() {
        eprintln!("skipping honesty: Dynare 7.2 is absent");
        return;
    }
    for source in [
        "var y c; varexo e; model; end;",
        "var y; model(linear); /* empty */\r\nend;",
        "var y; model; y=0; end; model; end;",
    ] {
        let official = dygnosis::run_preprocessor(
            source,
            &pp,
            None,
            std::time::Duration::from_secs(30),
            dygnosis::JsonStage::Check,
        );
        assert!(
            !official.success,
            "{source}: {}{}",
            official.raw_stdout, official.raw_stderr
        );
        assert!(
            format!("{}{}", official.raw_stdout, official.raw_stderr)
                .contains("syntax error, unexpected END"),
            "{source}: {}{}",
            official.raw_stdout,
            official.raw_stderr
        );
    }
}

/// `end` inside an equation is the closer token. 7.1: `syntax error, unexpected END`.
#[test]
fn end_inside_an_equation_is_unexpected_end() {
    for src in [
        "var y end;\nvarexo e;\nparameters rho;\nrho = 0.9;\nmodel;\ny = rho*y(-1) + e + end + 0;\nend;\n",
        "var y end;\nvarexo e;\nparameters rho;\nrho = 0.9;\nmodel;\nend = 0.1*y;\ny = rho*y(-1) + e;\nend;\n",
    ] {
        let got = rust_e001(src);
        let hit = got
            .iter()
            .find(|d| d.message.contains("unexpected END"))
            .unwrap_or_else(|| panic!("{got:?}"));
        assert_eq!(hit.message, "syntax error, unexpected END");
    }
}

/// `end = 0;` inside `histval` is the closer, same as in `initval`.
#[test]
fn end_assignment_inside_histval_still_refuses() {
    let src = "\
var y end;
varexo e;
parameters rho;
rho = 0.9;
model;
y = rho*y(-1) + e;
end;
histval;
y(0) = 0;
end = 0;
end;
";
    let got = rust_e001(src);
    assert!(
        got.iter()
            .any(|d| d.message.contains("unexpected IDENTIFIER")),
        "{got:?}"
    );
}

/// `+ end` inside the model is the closer. The equation does not parse, and the
/// declaration itself is not a reserved-identifier error.
#[test]
fn end_used_in_an_equation_still_refuses() {
    let src = "\
var y end;
varexo e;
parameters rho;
rho = 0.9;
model;
y = rho*y(-1) + e + end;
end;
";
    let got = rust_e001(src);
    assert!(
        got.iter().any(|d| d.code == "E001"),
        "using end in an equation must still refuse: {got:?}"
    );
    assert!(
        got.iter()
            .all(|d| !d.message.contains("Invalid Dynare identifier")),
        "{got:?}"
    );
}

/// A declared opener name inside an expression block is that name. The parse
/// shape accepts; the separate aggregate equation count refuses at transform.
#[test]
fn declared_opener_row_in_matched_moments_is_parse_valid() {
    let src = "\
var y shocks;
varexo e;
parameters a;
a = 0.5;
model;
y = a*y(-1) + e + shocks;
end;
matched_moments;
shocks;
end;
shocks;
var e; stderr 0.01;
end;
stoch_simul(order=1, irf=0, nograph);
";
    let errors: Vec<_> = analyze(&parse(src))
        .into_iter()
        .filter(|d| d.severity == Severity::Error)
        .map(|d| d.code)
        .collect();
    assert_eq!(errors, vec!["E188"], "{errors:?}");
    if let Some(pp) = dygnosis::find_preprocessor(None) {
        let check = dygnosis::run_preprocessor(
            src,
            &pp,
            None,
            std::time::Duration::from_secs(30),
            dygnosis::JsonStage::Check,
        );
        assert!(check.success, "{}{}", check.raw_stdout, check.raw_stderr);
        let transform = dygnosis::run_preprocessor(
            src,
            &pp,
            None,
            std::time::Duration::from_secs(30),
            dygnosis::JsonStage::Transform,
        );
        assert!(!transform.success);
        assert!(format!("{}{}", transform.raw_stdout, transform.raw_stderr)
            .contains("There are 1 equations but 2 endogenous variables!"));
    }
}

/// `name = value` at statement head, when `name` is an `<INITIAL>` keyword.
/// 7.1 refuses `syntax error, unexpected EQUAL, expecting ';' or '('`.
#[test]
fn keyword_followed_by_eq_is_their_syntax_error() {
    for (name, decl) in [
        ("model", "parameters model;"),
        ("steady", "parameters steady;"),
        ("shocks", "parameters shocks;"),
    ] {
        let src = format!(
            "var y;\nvarexo e;\n{decl}\n{name} = 0.2;\n\
             model;\ny = 0.2*y(-1) + e;\nend;\n\
             shocks;\nvar e; stderr 0.01;\nend;\n\
             stoch_simul(order=1, irf=0, nograph);\n"
        );
        let got = rust_e001(&src);
        let hit = got
            .iter()
            .find(|d| d.message.contains("unexpected EQUAL"))
            .unwrap_or_else(|| panic!("{name}: {got:?}"));
        assert_eq!(
            hit.message,
            "syntax error, unexpected EQUAL, expecting ';' or '('"
        );
        assert!(
            !got.iter().any(|d| d.message.contains("Missing 'end;'")),
            "{name} must not swallow the real block: {got:?}"
        );
    }
}
