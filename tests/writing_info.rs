use std::path::PathBuf;

use dygnosis::explain::{explain, ExplainKind};
use dygnosis::{analyze, check_file, parse, Diagnostic};

fn fixture(name: &str) -> (String, String) {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/writing")
        .join(name);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("missing {}: {err}", path.display()))
        .replace("\r\n", "\n");
    (text, path.to_string_lossy().to_string())
}

fn codes(name: &str) -> Vec<String> {
    let (text, path) = fixture(name);
    check_file(&text, &path)
        .into_iter()
        .map(|diag| diag.code)
        .filter(|code| code.starts_with('I') && code != "I050")
        .collect()
}

fn one(name: &str, code: &str) -> Diagnostic {
    let (text, path) = fixture(name);
    let hits: Vec<_> = check_file(&text, &path)
        .into_iter()
        .filter(|diag| diag.code == code)
        .collect();
    assert_eq!(hits.len(), 1, "{name} {code}: {hits:?}");
    hits.into_iter().next().unwrap()
}

fn slice_of(name: &str, diag: &Diagnostic) -> String {
    let (text, _) = fixture(name);
    let start = diag.span.start as usize;
    let end = diag.span.end as usize;
    text.get(start..end).unwrap_or("").to_string()
}

#[test]
fn singular_summaries_anchor_on_the_first_site() {
    let unnamed = one("one_each.mod", "I208");
    assert_eq!(unnamed.message, "1 counted equation has no name tag.");
    assert!(slice_of("one_each.mod", &unnamed).contains("y ="));

    let symbols = one("one_each.mod", "I209");
    assert_eq!(symbols.message, "3 symbols have no long_name.");
    assert_eq!(slice_of("one_each.mod", &symbols), "y");

    let numbers = one("one_each.mod", "I210");
    assert_eq!(
        numbers.message,
        "1 number is written directly in equations. Consider named parameters."
    );
    assert_eq!(slice_of("one_each.mod", &numbers), "2");
}

#[test]
fn plural_wording() {
    assert_eq!(
        one("plural.mod", "I208").message,
        "2 counted equations have no name tag."
    );
    assert_eq!(
        one("plural.mod", "I209").message,
        "2 symbols have no long_name."
    );
    assert_eq!(
        one("plural.mod", "I210").message,
        "2 numbers are written directly in equations. Consider named parameters."
    );
    assert_eq!(slice_of("plural.mod", &one("plural.mod", "I210")), "2");
}

#[test]
fn named_zero_and_one_stay_quiet() {
    let got = codes("quiet_named.mod");
    assert!(
        !got.iter()
            .any(|code| code == "I208" || code == "I209" || code == "I210"),
        "{got:?}"
    );
}

#[test]
fn power_counts_and_signed_one_is_quiet() {
    let numbers = one("power.mod", "I210");
    assert_eq!(
        numbers.message,
        "1 number is written directly in equations. Consider named parameters."
    );
    assert_eq!(slice_of("power.mod", &numbers), "2");
    assert!(codes("power.mod").iter().all(|code| code != "I208"));
    assert!(codes("power.mod").iter().all(|code| code != "I209"));
}

#[test]
fn macro_copies_count_and_share_the_template_anchor() {
    assert_eq!(
        one("loop.mod", "I208").message,
        "3 counted equations have no name tag."
    );
    let numbers = one("loop.mod", "I210");
    assert_eq!(
        numbers.message,
        "3 numbers are written directly in equations. Consider named parameters."
    );
    assert_eq!(slice_of("loop.mod", &numbers), "4");
}

#[test]
fn macro_names_count_once_each() {
    let symbols = one("macro_names.mod", "I209");
    assert_eq!(symbols.message, "3 symbols have no long_name.");
    assert_eq!(slice_of("macro_names.mod", &symbols), "z");
    assert!(!codes("macro_names.mod").iter().any(|code| code == "I208"));
}

#[test]
fn removed_equation_is_not_counted() {
    let got = codes("removed.mod");
    assert!(
        !got.iter().any(|code| code == "I208" || code == "I210"),
        "{got:?}"
    );
}

#[test]
fn heterogeneous_rows_share_one_summary() {
    assert_eq!(
        one("het.mod", "I208").message,
        "2 counted equations have no name tag."
    );
    let symbols = one("het.mod", "I209");
    assert_eq!(symbols.message, "1 symbol has no long_name.");
    assert_eq!(slice_of("het.mod", &symbols), "c");
    let numbers = one("het.mod", "I210");
    assert_eq!(
        numbers.message,
        "1 number is written directly in equations. Consider named parameters."
    );
    assert_eq!(slice_of("het.mod", &numbers), "3");
}

#[test]
fn include_anchor_uses_the_child_file() {
    let (text, path) = fixture("parent.mod");
    let diags = check_file(&text, &path);
    let unnamed = diags.iter().find(|diag| diag.code == "I208").unwrap();
    let child = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/writing/child.mod"),
    )
    .unwrap()
    .replace("\r\n", "\n");
    let slice = &child[unnamed.span.start as usize..unnamed.span.end as usize];
    assert!(slice.contains("y = 4"), "{slice:?}");
    let number = diags.iter().find(|diag| diag.code == "I210").unwrap();
    let literal = &child[number.span.start as usize..number.span.end as usize];
    assert_eq!(literal, "4");
    assert!(diags.iter().all(|diag| diag.code != "I209"));
}

#[test]
fn unresolved_include_and_syntax_withhold_the_summaries() {
    for name in ["unresolved.mod", "syntax.mod"] {
        let got = codes(name);
        assert!(
            !got.iter()
                .any(|code| code == "I208" || code == "I209" || code == "I210"),
            "{name}: {got:?}"
        );
    }
}

#[test]
fn disable_comment_does_not_silence_information() {
    let got = codes("suppress.mod");
    assert!(got.iter().any(|code| code == "I208"), "{got:?}");
    assert!(got.iter().any(|code| code == "I210"), "{got:?}");
}

#[test]
fn empty_long_name_still_counts() {
    let symbols = one("empty_long_name.mod", "I209");
    assert_eq!(symbols.message, "1 symbol has no long_name.");
    assert_eq!(slice_of("empty_long_name.mod", &symbols), "y");
}

#[test]
fn deterministic_exogenous_is_counted_once() {
    let symbols = one("det_once.mod", "I209");
    assert_eq!(symbols.message, "1 symbol has no long_name.");
    assert_eq!(slice_of("det_once.mod", &symbols), "tau");
}

#[test]
fn concatenated_names_count_once_each_and_are_not_duplicates() {
    let src = "\
@#define is = 1:2
@#for i in is
var x@{i};
@#endfor
model;
@#for i in is
x@{i} = 0;
@#endfor
end;
";
    let model = parse(src);
    let names: Vec<&str> = model
        .endogenous
        .iter()
        .map(|decl| model.name(decl.name))
        .collect();
    assert_eq!(names, ["x1", "x2"]);
    let diags = analyze(&model);
    assert!(diags.iter().all(|diag| diag.code != "W031"), "{diags:?}");
    let note = diags.iter().find(|diag| diag.code == "I209").unwrap();
    assert_eq!(note.message, "2 symbols have no long_name.");
}

#[test]
fn repeated_declaration_still_warns_and_counts_once() {
    let src = "\
var x;
var x;
model;
x = 0;
end;
";
    let diags = analyze(&parse(src));
    assert!(diags.iter().any(|diag| diag.code == "W031"), "{diags:?}");
    let note = diags.iter().find(|diag| diag.code == "I209").unwrap();
    assert_eq!(note.message, "1 symbol has no long_name.");
}

#[test]
fn unresolved_name_expansion_withholds_the_summary() {
    let src = "\
var x@{UNDEF};
model;
x = 0;
end;
";
    let diags = analyze(&parse(src));
    assert!(diags.iter().all(|diag| diag.code != "I209"), "{diags:?}");
    assert!(diags.iter().any(|diag| diag.code == "E063"), "{diags:?}");
}

#[test]
fn explain_calls_them_writing_preferences() {
    for code in ["I208", "I209", "I210"] {
        let entry = explain(code).unwrap();
        assert_eq!(entry.kind, ExplainKind::Added);
        assert!(entry.body.contains("writing preference"));
        assert!(entry.body.contains("not a Dynare refusal"));
    }
}
