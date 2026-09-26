use std::path::{Path, PathBuf};

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
@#define a = beta
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
