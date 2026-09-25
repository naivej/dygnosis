use std::path::PathBuf;

use dygnosis::explain::known_codes;
use dygnosis::lexer::{tokenize, TokenKind};
use dygnosis::{
    analyze, check_e010, check_file, check_parse, count_gap, equations, has_structural_error, parse,
};

fn read_mod(rel: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(rel);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("fixture missing at {}: {e}", path.display()))
        .replace("\r\n", "\n")
}

fn expected_dump(name: &str) -> serde_json::Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/expected/occbin")
        .join(name);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("expected missing at {}: {e}", path.display()))
        .replace("\r\n", "\n");
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn occbin_expr_text(expr: &dygnosis::OccbinExpr) -> serde_json::Value {
    serde_json::Value::String(expr.text.clone())
}

fn dump_lib(model: &dygnosis::Model) -> serde_json::Value {
    let gap = count_gap(model);
    let eqs: Vec<serde_json::Value> = equations(model)
        .into_iter()
        .map(|row| {
            let mut v = serde_json::json!({
                "index": row.index,
                "name": row.name,
                "text": row.text,
                "tags": row.tags,
            });
            if let Some(comp) = row.complementarity {
                let matched = comp.matched.map(|m| {
                    serde_json::json!({
                        "variable": m.variable,
                        "lower_bound": m.lower_bound,
                        "upper_bound": m.upper_bound,
                    })
                });
                v["complementarity"] = serde_json::json!({
                    "text": comp.text,
                    "matched": matched,
                });
            }
            v
        })
        .collect();
    let constraints: Vec<serde_json::Value> = model
        .occbin_constraints
        .iter()
        .map(|c| {
            serde_json::json!({
                "name": c.name,
                "bind": c.bind.as_ref().map(occbin_expr_text),
                "relax": c.relax.as_ref().map(occbin_expr_text),
                "error_bind": c.error_bind.as_ref().map(occbin_expr_text),
                "error_relax": c.error_relax.as_ref().map(occbin_expr_text),
            })
        })
        .collect();
    serde_json::json!({
        "count_gap": {
            "n_endogenous": gap.n_endogenous,
            "n_equations": gap.n_equations,
            "delta": gap.delta,
            "expected_delta": gap.expected_delta,
        },
        "equations": eqs,
        "occbin_constraints": constraints,
    })
}

#[test]
fn square_mod_structure_and_collapse() {
    let text = read_mod("occbin/square.mod");
    let model = parse(&text);
    assert!(
        check_parse(&model).is_empty(),
        "square.mod check_parse: {:?}",
        check_parse(&model)
    );
    assert!(!has_structural_error(&model));

    let rows = equations(&model);
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0].index, 0);
    assert_eq!(rows[1].index, 1);
    assert_eq!(rows[2].index, 2);
    assert_eq!(rows[1].name, "policy");
    assert_eq!(rows[2].name, "policy");
    assert_eq!(rows[1].tags.get("relax").map(String::as_str), Some("ELB"));
    assert_eq!(rows[2].tags.get("bind").map(String::as_str), Some("ELB"));
    assert_eq!(rows[1].tags.get("name").map(String::as_str), Some("policy"));
    assert_eq!(rows[2].tags.get("name").map(String::as_str), Some("policy"));

    let gap = count_gap(&model);
    assert_eq!(gap.n_equations, 2);
    assert_eq!(gap.n_endogenous, 2);
    assert!(
        check_e010(&model).is_empty(),
        "square.mod check_e010: {:?}",
        check_e010(&model)
    );

    assert_eq!(model.occbin_constraints.len(), 1);
    let c = &model.occbin_constraints[0];
    assert_eq!(c.name, "ELB");
    assert!(c.bind.is_some());
    assert!(c.relax.is_some());
    let bind = c.bind.as_ref().unwrap();
    assert!(
        bind.text.contains("<=") && bind.text.contains('i'),
        "bind text {}",
        bind.text
    );
}

#[test]
fn perp_mod_records_complementarity() {
    let text = read_mod("occbin/perp.mod");
    let model = parse(&text);
    assert!(
        check_parse(&model).is_empty(),
        "perp.mod check_parse: {:?}",
        check_parse(&model)
    );
    let rows = equations(&model);
    assert_eq!(rows.len(), 1);
    let row = &rows[0];
    let comp = row
        .complementarity
        .as_ref()
        .expect("perp.mod complementarity");
    assert_eq!(row.text, "i = 0");
    assert!(!row.text.contains('⟂'));
    let written = model
        .equations
        .iter()
        .find(|eq| eq.text == "i = 0")
        .expect("perp equation");
    assert!(!written.lhs.contains('⟂'));
    assert!(!written.rhs.contains('⟂'));
    assert!(
        comp.text.contains("i >= 0"),
        "complementarity text {}",
        comp.text
    );
    let matched = comp.matched.as_ref().expect("matched triple");
    assert_eq!(matched.variable, "i");
    assert_eq!(matched.lower_bound.as_deref(), Some("0"));
    assert!(matched.upper_bound.is_none());
}

#[test]
fn perp_ascii_mod_same_recording_and_token() {
    let text = read_mod("occbin/perp_ascii.mod");
    let kinds: Vec<TokenKind> = tokenize(&text).into_iter().map(|t| t.kind).collect();
    assert!(
        kinds.contains(&TokenKind::Perpendicular),
        "tokenize {kinds:?}"
    );
    let model = parse(&text);
    assert!(
        check_parse(&model).is_empty(),
        "perp_ascii.mod check_parse: {:?}",
        check_parse(&model)
    );
    let rows = equations(&model);
    assert_eq!(rows.len(), 1);
    let row = &rows[0];
    let comp = row
        .complementarity
        .as_ref()
        .expect("perp_ascii complementarity");
    assert_eq!(row.text, "i = 0");
    assert!(!row.text.contains("_|_"));
    assert!(
        comp.text.contains("i >= 0"),
        "complementarity text {}",
        comp.text
    );
    let matched = comp.matched.as_ref().expect("matched triple");
    assert_eq!(matched.variable, "i");
    assert_eq!(matched.lower_bound.as_deref(), Some("0"));
    assert!(matched.upper_bound.is_none());
}

#[test]
fn surprise_mod_sets_flag() {
    let text = read_mod("occbin/surprise.mod");
    let model = parse(&text);
    assert!(model.shocks_surprise);
    assert!(
        check_parse(&model).is_empty(),
        "surprise.mod check_parse: {:?}",
        check_parse(&model)
    );
    let known: Vec<&str> = known_codes();
    let codes: Vec<String> = analyze(&model).into_iter().map(|d| d.code).collect();
    for code in &codes {
        assert!(
            known.contains(&code.as_str()),
            "surprise.mod new code {code}"
        );
    }
}

#[test]
fn tags_mod_does_not_collapse_duplicate_name() {
    let model = parse(&read_mod("equations/tags.mod"));
    let rows = equations(&model);
    assert_eq!(rows.len(), 3);
    assert_eq!(count_gap(&model).n_equations, 3);
}

#[test]
fn reader_mod_tag_map_and_flag_vec() {
    let model = parse(&read_mod("equations/reader.mod"));
    let rows = equations(&model);
    assert_eq!(rows[0].tags.get("name").map(String::as_str), Some("euler"));

    let static_eq = model
        .equations
        .iter()
        .find(|e| e.static_tag)
        .expect("static equation");
    assert_eq!(
        static_eq.tag_map.get("static").map(String::as_str),
        Some("")
    );
    assert_eq!(static_eq.tags, vec!["static".to_string()]);

    let dynamic_eq = model
        .equations
        .iter()
        .find(|e| e.dynamic_tag)
        .expect("dynamic equation");
    assert_eq!(
        dynamic_eq.tag_map.get("dynamic").map(String::as_str),
        Some("")
    );
    assert_eq!(dynamic_eq.tags, vec!["dynamic".to_string()]);
}

#[test]
fn square_and_perp_library_dumps() {
    for name in ["square.json", "perp.json"] {
        let stem = name.trim_end_matches(".json");
        let model = parse(&read_mod(&format!("occbin/{stem}.mod")));
        let got = dump_lib(&model);
        let expected = expected_dump(name);
        assert_eq!(got, expected, "{name}");
    }
}

#[test]
fn square_invariants_list_vs_count_known_codes() {
    let text = read_mod("occbin/square.mod");
    let model = parse(&text);
    assert!(check_parse(&model).is_empty());
    assert!(check_e010(&model).is_empty());
    let rows = equations(&model);
    assert_eq!(rows.len(), 3);
    assert_eq!(count_gap(&model).n_equations, 2);
    assert_eq!(rows[0].index, 0);
    assert_eq!(rows[0].name, "");
    assert_eq!(rows[0].text, "is = rhos*is(-1)+e");
    assert!(!rows[0].static_tag);
    assert!(!rows[0].dynamic_tag);
    assert!(!rows[0].idents.is_empty());

    let known: Vec<&str> = known_codes();
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/occbin/square.mod")
        .to_string_lossy()
        .to_string();
    let mut codes: Vec<String> = analyze(&model).into_iter().map(|d| d.code).collect();
    codes.extend(check_file(&text, &path).into_iter().map(|d| d.code));
    for code in &codes {
        assert!(
            known.contains(&code.as_str()),
            "square.mod unknown family {code}"
        );
        assert_ne!(code, "W013", "square OccBin must be W013-quiet");
    }
}

#[test]
fn reader_flag_vec_still_w054_class() {
    let model = parse(&read_mod("equations/reader.mod"));
    let static_eq = model
        .equations
        .iter()
        .find(|e| e.static_tag)
        .expect("static equation");
    assert_eq!(static_eq.tags, vec!["static".to_string()]);
    let dynamic_eq = model
        .equations
        .iter()
        .find(|e| e.dynamic_tag)
        .expect("dynamic equation");
    assert_eq!(dynamic_eq.tags, vec!["dynamic".to_string()]);
}

#[test]
fn official_rbc_occbin_not_e001_if_present() {
    let path = PathBuf::from(r"C:\dynare\7.1\examples\occbin\rbc_occbin.mod");
    if !path.is_file() {
        return;
    }
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
        .replace("\r\n", "\n");
    let model = parse(&text);
    assert!(
        check_parse(&model).is_empty(),
        "rbc_occbin.mod check_parse: {:?}",
        check_parse(&model)
    );
}

#[test]
fn complementarity_form2_bound_then_endo() {
    let model = parse("var i; model; i = 0 ⟂ 0 <= i; end;");
    let rows = equations(&model);
    assert_eq!(rows.len(), 1);
    assert!(!rows[0].text.contains('⟂'));
    let matched = rows[0]
        .complementarity
        .as_ref()
        .expect("form 2 complementarity")
        .matched
        .as_ref()
        .expect("form 2 matched");
    assert_eq!(matched.variable, "i");
    assert_eq!(matched.lower_bound.as_deref(), Some("0"));
    assert!(matched.upper_bound.is_none());
}

#[test]
fn complementarity_chain_lower_and_upper() {
    let model = parse("var i; model; i = 0 ⟂ 0 <= i <= 1; end;");
    let rows = equations(&model);
    assert_eq!(rows.len(), 1);
    assert!(!rows[0].text.contains('⟂'));
    let matched = rows[0]
        .complementarity
        .as_ref()
        .expect("chain complementarity")
        .matched
        .as_ref()
        .expect("chain matched");
    assert_eq!(matched.variable, "i");
    assert_eq!(matched.lower_bound.as_deref(), Some("0"));
    assert_eq!(matched.upper_bound.as_deref(), Some("1"));
}

#[test]
fn empty_name_bind_relax_not_collapsed() {
    let model = parse("var i;\nmodel;\n[bind='ELB'] i = 0;\n[relax='ELB'] i = 1;\nend;\n");
    let rows = equations(&model);
    assert_eq!(rows.len(), 2);
    assert_eq!(count_gap(&model).n_equations, 2);
}

#[test]
fn mshocks_surprise_does_not_set_flag() {
    let model =
        parse("var y; varexo e; model; y = e; end; mshocks(surprise); var e; stderr 0.01; end;");
    assert!(!model.shocks_surprise);
}

#[test]
fn shocks_surprise_sticky_across_later_plain_shocks() {
    let model = parse(
        "var y; varexo e; model; y = e; end; shocks(surprise); var e; stderr 0.01; end; shocks; var e; stderr 0.01; end;",
    );
    assert!(model.shocks_surprise);
}

#[test]
fn duplicate_bind_clause_last_wins_not_e001() {
    let model = parse(
        "var i; model; i = 0; end; occbin_constraints; name 'ELB'; bind i <= 0; bind i <= 1; end;",
    );
    assert!(
        check_parse(&model).is_empty(),
        "duplicate bind check_parse: {:?}",
        check_parse(&model)
    );
    assert_eq!(model.occbin_constraints.len(), 1);
    let bind = model.occbin_constraints[0]
        .bind
        .as_ref()
        .expect("stored bind");
    assert_eq!(bind.text, "i <= 1");
}
