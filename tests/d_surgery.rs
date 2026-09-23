//! Equation-surgery family locks: parse (0.5.3 01) and diagnostics (0.5.3 02).
//!
//! `model_remove` / `model_replace` are parsed and applied while parsing, so the
//! model object is post-removal. For `model_remove` — and only there — a removed
//! equation's endogenous becomes an exogenous while it is still used, or leaves the
//! model otherwise; a `model_replace` removal keeps every type, as 7.1 does.
//!
//! The statement refuses carry their own codes (E335–E337, plus E256 for a repeated
//! tag key); the malformed shapes are **E001**. A double-quoted string is lexer junk
//! wherever the grammar reads a string (verbatim bodies excepted).

use dygnosis::model::Model;
use dygnosis::{analyze, check_file, parse, Diagnostic};

const QUIET: &[&str] = &[
    "d_surgery/quiet_remove.mod",
    "d_surgery/quiet_remove_exo.mod",
    "d_surgery/quiet_remove_many.mod",
    "d_surgery/quiet_replace.mod",
    "d_surgery/quiet_tag_forms.mod",
    "d_surgery/quiet_dropped_symbol.mod",
    "d_surgery/quiet_optw_retyped.mod",
    "d_surgery/quiet_planner_dropped.mod",
];

/// One locked fire: fixture, code, and the mapped part of their message.
struct Fire {
    fixture: &'static str,
    code: &'static str,
    message: &'static str,
}

const FIRES: &[Fire] = &[
    Fire {
        fixture: "d_surgery/e335_tag_not_found.mod",
        code: "E335",
        message: "The equations specified by name=nosuchtag were not found.",
    },
    Fire {
        fixture: "d_surgery/e336_no_lhs_variable.mod",
        code: "E336",
        message: "Equation 1 has been excluded but it does not have a single variable on its left-hand side or an `endogenous` tag",
    },
    Fire {
        fixture: "d_surgery/e337_excluded_twice.mod",
        code: "E337",
        message: "Variable c was excluded twice via a model_remove or model_replace statement, or via the include_eqs or exclude_eqs option",
    },
    Fire {
        fixture: "d_surgery/e256_tag_twice_surgery.mod",
        code: "E256",
        message: "Tag 'name' cannot be used twice for the same equation",
    },
    Fire {
        fixture: "d_surgery/e314_filter_dropped.mod",
        code: "E314",
        message: "filter_initial_state: variable c does not appear in the model with the lag -1",
    },
    Fire {
        fixture: "d_surgery/e020_dropped_equation_name.mod",
        code: "E020",
        message: "Undeclared identifier 'zzz'",
    },
    Fire {
        fixture: "d_surgery/e251_planner_exogenous.mod",
        code: "E251",
        message: "You cannot include exogenous variables (or variables of undeclared type) in the planner objective",
    },
    Fire {
        fixture: "d_surgery/e317_after_removal.mod",
        code: "E317",
        message: "c is not endogenous.",
    },
    Fire {
        fixture: "d_surgery/e208_static_after_remove.mod",
        code: "E208",
        message: "the number of equations marked [static] must be equal to the number of equations marked [dynamic]",
    },
];

/// Every shape 7.1 refuses at the lexer with `character unrecognized by lexer`.
const DOUBLE_QUOTED: &[&str] = &[
    "d_surgery/e001_double_quoted_equation_tag.mod",
    "d_surgery/e001_double_quoted_bvar.mod",
    "d_surgery/e001_double_quoted_shock_group.mod",
    "d_surgery/e001_double_quoted_tag.mod",
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

fn find<'a>(diags: &'a [Diagnostic], code: &str) -> &'a Diagnostic {
    diags
        .iter()
        .find(|d| d.code == code)
        .unwrap_or_else(|| panic!("expected {code}, got {:?}", codes(diags)))
}

fn quiet(diags: &[Diagnostic], code: &str) {
    assert!(
        diags.iter().all(|d| d.code != code),
        "expected no {code}, got {:?}",
        codes(diags)
    );
}

fn e001_message(rel: &str) -> String {
    find(&analyze(&parse(&fixture(rel))), "E001")
        .message
        .clone()
}

fn endogenous(model: &Model, name: &str) -> bool {
    model.endogenous.iter().any(|d| model.name(d.name) == name)
}

fn exogenous(model: &Model, name: &str) -> bool {
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
fn statement_refuses_fire_with_their_text() {
    for fire in FIRES {
        let diags = analyze(&parse(&fixture(fire.fixture)));
        let hit = find(&diags, fire.code);
        assert!(
            hit.message.contains(fire.message),
            "{}: {} message {:?} must carry {:?}",
            fire.fixture,
            fire.code,
            hit.message,
            fire.message
        );
        let file_diags = check_file(&fixture(fire.fixture), &fixture_path(fire.fixture));
        assert!(
            file_diags.iter().any(|d| d.code == fire.code),
            "{}: check_file must emit {}, got {:?}",
            fire.fixture,
            fire.code,
            codes(&file_diags)
        );
    }
}

#[test]
fn repeated_tag_key_stops_the_statement() {
    // 7.1 refuses the tag list itself, so the not-found check never runs.
    let diags = analyze(&parse(&fixture("d_surgery/e256_tag_twice_surgery.mod")));
    quiet(&diags, "E335");
}

#[test]
fn double_quoted_string_is_lexer_junk() {
    for rel in DOUBLE_QUOTED {
        let diags = analyze(&parse(&fixture(rel)));
        let hit = find(&diags, "E001");
        assert!(
            hit.message
                .contains("Double-quoted string in the .mod file"),
            "{rel}: {:?}",
            hit.message
        );
    }
}

#[test]
fn verbatim_body_keeps_its_double_quotes() {
    let model = parse(&fixture("d_surgery/quiet_verbatim_quotes.mod"));
    assert!(
        analyze(&model).is_empty(),
        "verbatim passes raw text through: {:?}",
        codes(&analyze(&model))
    );
}

#[test]
fn dropped_symbol_entries_do_not_fire() {
    // The removal drops `c`; 7.1 accepts the entries that named it before that point,
    // and refuses `observation_trends` / `filter_initial_state` like we do.
    let diags = analyze(&parse(&fixture("d_surgery/quiet_dropped_symbol.mod")));
    quiet(&diags, "E058");
    quiet(&diags, "E090");
    let model = parse(&fixture("d_surgery/quiet_dropped_symbol.mod"));
    assert!(model
        .excluded_endogenous
        .iter()
        .any(|d| model.name(d.name) == "c"));
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
    let model = parse(&fixture("d_surgery/e001_unquoted_tag_value.mod"));
    assert_eq!(model.equations.len(), 3);
    assert!(model.equation_surgery[0].removed.is_empty());
}

#[test]
fn registry_known_codes_include_shock_diagnostics() {
    assert_eq!(dygnosis::explain::known_codes().len(), 338);
}
