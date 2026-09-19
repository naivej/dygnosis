//! P-parse family locks: `model_remove` / `model_replace` are parsed, so the model
//! object is post-removal, and the shapes 7.1 refuses at parse carry **E001**.
//!
//! The removal mirrors 7.1's parse-time `removeEquations`: matching equations leave
//! `Model::equations` (with a record in `Model::equation_surgery`). For
//! `model_remove` — and only there — a removed equation's endogenous becomes an
//! exogenous while it is still used, or leaves the model otherwise; a
//! `model_replace` removal keeps every type, as 7.1 does.

use dygnosis::{analyze, check_file, parse, Diagnostic};

const QUIET: &[&str] = &[
    "d_surgery/quiet_remove.mod",
    "d_surgery/quiet_remove_exo.mod",
    "d_surgery/quiet_remove_many.mod",
    "d_surgery/quiet_replace.mod",
    "d_surgery/quiet_tag_forms.mod",
];

fn fixture(rel: &str) -> String {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(rel);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
        .replace("\r\n", "\n")
}

fn fixture_path(rel: &str) -> String {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(rel)
        .to_str()
        .expect("utf-8 fixture path")
        .to_string()
}

fn codes(diags: &[Diagnostic]) -> Vec<&str> {
    diags.iter().map(|d| d.code.as_str()).collect()
}

fn e001_message(rel: &str) -> String {
    let diags = analyze(&parse(&fixture(rel)));
    diags
        .iter()
        .find(|d| d.code == "E001")
        .unwrap_or_else(|| panic!("{rel}: expected E001, got {:?}", codes(&diags)))
        .message
        .clone()
}

fn endogenous(model: &dygnosis::model::Model, name: &str) -> bool {
    model.endogenous.iter().any(|d| model.name(d.name) == name)
}

fn exogenous(model: &dygnosis::model::Model, name: &str) -> bool {
    model.exogenous.iter().any(|d| model.name(d.name) == name)
}

#[test]
fn legal_surgery_files_are_quiet() {
    for rel in QUIET {
        let diags = analyze(&parse(&fixture(rel)));
        assert!(
            diags.is_empty(),
            "{rel}: expected no diagnostics, got {:?}",
            codes(&diags)
        );
    }
    let rel = "d_surgery/quiet_remove.mod";
    let diags = check_file(&fixture(rel), &fixture_path(rel));
    assert!(
        diags.is_empty(),
        "{rel} check_file: expected no diagnostics, got {:?}",
        codes(&diags)
    );
}

#[test]
fn legal_remove_is_parsed_and_post_removal() {
    let model = parse(&fixture("d_surgery/quiet_remove.mod"));
    assert_eq!(model.equations.len(), 2, "e3 is removed");
    assert_eq!(model.equation_surgery.len(), 1);
    let surgery = &model.equation_surgery[0];
    assert!(!surgery.replace);
    assert_eq!(
        surgery.tag_sets,
        vec![vec![("name".to_string(), "e3".to_string())]]
    );
    assert!(surgery.unmatched.is_empty());
    assert!(surgery.tag_twice.is_empty());
    assert_eq!(surgery.removed.len(), 1);
    assert_eq!(surgery.removed[0].number, 3);
    assert_eq!(surgery.removed[0].endogenous.as_deref(), Some("dummy1"));
    assert!(!endogenous(&model, "dummy1"));
    assert!(!exogenous(&model, "dummy1"));
}

#[test]
fn legal_remove_turns_a_used_endogenous_into_an_exogenous() {
    let model = parse(&fixture("d_surgery/quiet_remove_exo.mod"));
    assert_eq!(model.equations.len(), 1);
    assert_eq!(model.equation_surgery[0].removed.len(), 2);
    assert!(endogenous(&model, "k"));
    assert!(!endogenous(&model, "c"));
    assert!(exogenous(&model, "c"));
}

#[test]
fn one_tag_set_removes_every_match() {
    let model = parse(&fixture("d_surgery/quiet_remove_many.mod"));
    let surgery = &model.equation_surgery[0];
    assert_eq!(
        surgery.tag_sets,
        vec![vec![("grp".to_string(), "g".to_string())]]
    );
    assert_eq!(surgery.removed.len(), 2, "one set, two equations");
    assert_eq!(
        surgery
            .removed
            .iter()
            .map(|row| row.number)
            .collect::<Vec<_>>(),
        vec![1, 2]
    );
    assert_eq!(model.equations.len(), 1);
    assert!(!endogenous(&model, "c") && !exogenous(&model, "c"));
    assert!(
        exogenous(&model, "dummy1"),
        "still used by the remaining equation"
    );
    assert!(endogenous(&model, "k"));
}

#[test]
fn tag_forms_match_like_the_official() {
    let model = parse(&fixture("d_surgery/quiet_tag_forms.mod"));
    assert_eq!(model.equations.len(), 1);
    let surgery = &model.equation_surgery[0];
    assert_eq!(
        surgery.removed.len(),
        2,
        "the bare flag pair and the bracketed conjunction"
    );
    assert!(surgery.unmatched.is_empty());
    assert!(exogenous(&model, "c"));
}

#[test]
fn legal_replace_adds_the_body_after_removal() {
    let model = parse(&fixture("d_surgery/quiet_replace.mod"));
    assert_eq!(model.equations.len(), 2);
    let surgery = &model.equation_surgery[0];
    assert!(surgery.replace);
    assert_eq!(surgery.removed.len(), 1);
    assert_eq!(surgery.removed[0].number, 1);
    assert!(
        model
            .equations
            .iter()
            .any(|eq| eq.lhs == "c" && eq.rhs == "k"),
        "the body equation is in the model: {:?}",
        model
            .equations
            .iter()
            .map(|eq| &eq.text)
            .collect::<Vec<_>>()
    );
    // `model_replace` removes without the type change; 7.1 prints nothing there.
    assert!(endogenous(&model, "c") && endogenous(&model, "k"));
}

#[test]
fn malformed_surgery_shapes_are_e001() {
    let empty = e001_message("d_surgery/e001_empty_tag_list.mod");
    assert!(
        empty.contains("Missing equation tag in the 'model_remove' statement"),
        "{empty}"
    );
    let quoted = e001_message("d_surgery/e001_double_quoted_tag.mod");
    assert!(
        quoted.contains("Double-quoted tag in the 'model_remove' statement"),
        "{quoted}"
    );
    let unquoted = e001_message("d_surgery/e001_unquoted_tag_value.mod");
    assert!(
        unquoted.contains("Unquoted tag value in the 'model_remove' statement"),
        "{unquoted}"
    );
    let body = e001_message("d_surgery/e001_empty_replace_body.mod");
    assert!(
        body.contains("Missing equation in the 'model_replace' block"),
        "{body}"
    );
}

#[test]
fn refused_tag_list_does_not_remove_an_equation() {
    for (rel, expected) in [
        ("d_surgery/e001_double_quoted_tag.mod", 2),
        ("d_surgery/e001_unquoted_tag_value.mod", 3),
    ] {
        let model = parse(&fixture(rel));
        assert_eq!(model.equations.len(), expected, "{rel}");
        assert!(model.equation_surgery[0].removed.is_empty(), "{rel}");
    }
}

#[test]
fn registry_is_unchanged() {
    assert_eq!(dygnosis::explain::known_codes().len(), 245);
}
