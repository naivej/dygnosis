use std::path::{Path, PathBuf};

use dygnosis::parse;

fn copilot_mod(archive_dir: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(".agents/skills/dynare-copilot/references/model-archive")
        .join(archive_dir)
        .join(format!("{archive_dir}.mod"))
}

fn read_mod(archive_dir: &str) -> String {
    let path = copilot_mod(archive_dir);
    std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!("fixture missing at {}: {e}", path.display());
    })
}

fn all_ref_names(model: &dygnosis::Model) -> Vec<String> {
    let mut names = Vec::new();
    for eq in model
        .equations
        .iter()
        .chain(model.steady_state_equations.iter())
    {
        for r in model.ident_refs(eq) {
            names.push(model.name(r.name).to_string());
        }
    }
    names
}

fn assert_trees_on_kept(model: &dygnosis::Model) {
    for eq in &model.equations {
        assert!(
            eq.lhs_expr.is_some(),
            "missing lhs_expr on model eq {:?}",
            eq.text
        );
        if !eq.lhs.is_empty() || !eq.rhs.is_empty() {
            assert!(
                eq.rhs_expr.is_some(),
                "missing rhs_expr on model eq {:?}",
                eq.text
            );
        }
    }
    for eq in &model.steady_state_equations {
        assert!(
            eq.lhs_expr.is_some(),
            "missing lhs_expr on ss eq {:?}",
            eq.text
        );
        if !eq.lhs.is_empty() || !eq.rhs.is_empty() {
            assert!(
                eq.rhs_expr.is_some(),
                "missing rhs_expr on ss eq {:?}",
                eq.text
            );
        }
    }
}

#[test]
fn trend_rbc_euler_c_lead_and_log_ig_not_a_ref() {
    let model = parse(&read_mod("trend_rbc_gov_inv"));
    assert_trees_on_kept(&model);

    let euler = model
        .equations
        .iter()
        .find(|e| e.name == "consumption Euler equation")
        .expect("Euler equation");
    let c_lead = model
        .ident_refs(euler)
        .into_iter()
        .find(|r| model.name(r.name) == "c" && r.timing == 1)
        .expect("c(+1) IdentRef");
    assert_eq!(c_lead.timing, 1);
    assert!(
        c_lead.timing_span.is_some(),
        "c(+1) should have timing_span"
    );

    let gov = model
        .equations
        .iter()
        .find(|e| e.name == "government investment process")
        .expect("government investment process");
    let refs = model.ident_refs(gov);
    assert!(
        refs.iter()
            .any(|r| model.name(r.name) == "ig" && r.timing == 0),
        "log(ig) should contribute ig at timing 0"
    );
    assert!(
        refs.iter().all(|r| model.name(r.name) != "log"),
        "Call callee log must not be an IdentRef"
    );
}

#[test]
fn ident_ref_span_is_lexeme_only_when_space_before_timing() {
    let model = parse("var c;\nmodel;\nc (+1);\nend;\n");
    assert_eq!(model.equations.len(), 1);
    let r = model
        .ident_refs(&model.equations[0])
        .into_iter()
        .find(|r| model.name(r.name) == "c")
        .expect("c IdentRef");
    assert_eq!(model.name(r.name), "c");
    assert_eq!(r.timing, 1);
    let lexeme = &model.source[r.span.start as usize..r.span.end as usize];
    assert_eq!(lexeme, "c");
    let ts = r.timing_span.expect("timing_span");
    let timing = &model.source[ts.start as usize..ts.end as usize];
    assert_eq!(timing, "(+1)");
}

#[test]
fn comment_and_string_mutations_are_not_ident_refs() {
    let original = read_mod("trend_rbc_gov_inv");
    let needle = "model;";
    let idx = original.find(needle).expect("model;");
    let insert_at = idx + needle.len();
    let mut mutated = String::new();
    mutated.push_str(&original[..insert_at]);
    mutated.push_str("\n// sneaky_ident\n'sneaky_ident';\n\"sneaky_ident\";\n");
    mutated.push_str(&original[insert_at..]);

    let model = parse(&mutated);
    let names = all_ref_names(&model);
    assert!(
        names.iter().all(|n| n != "sneaky_ident"),
        "sneaky_ident leaked into ident_refs: {names:?}"
    );
}

#[test]
fn archive_fixtures_have_expr_trees() {
    for dir in [
        "trend_rbc_gov_inv",
        "sims_wu_2019",
        "govt_rbc_irf_matching",
        "lk2024",
    ] {
        let model = parse(&read_mod(dir));
        assert_trees_on_kept(&model);
    }
}

#[test]
fn src_has_no_ident_harvest_regex() {
    let src_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut hits = Vec::new();
    visit_rs(&src_dir, &mut hits);
    assert!(
        hits.is_empty(),
        "identifier-harvest regex on equation text in src/: {hits:?}"
    );
}

fn visit_rs(dir: &Path, hits: &mut Vec<String>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.is_dir() {
            visit_rs(&path, hits);
            continue;
        }
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        if path.file_name().and_then(|n| n.to_str()) == Some("preprocessor.rs") {
            // Dynare stderr parser, not ident harvest from equation text.
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        let rel = path.display().to_string();
        for needle in [
            r"\b([A-Za-z_]",
            r"\bident\b",
            "Regex::new",
            "regex::Regex",
            "finditer",
        ] {
            if text.contains(needle) {
                hits.push(format!("{rel}: {needle}"));
            }
        }
    }
}
