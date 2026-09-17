use std::path::PathBuf;

use dygnosis::span::LineIndex;
use dygnosis::{analyze, check_occbin, parse, Diagnostic, Severity};

const OCCBIN_ERRORS: &[&str] = &[
    "E170", "E171", "E172", "E173", "E174", "E175", "E176", "E177", "E180", "E181",
    "E182", "E183", "E184", "E185",
];

struct Fire {
    rel: &'static str,
    code: &'static str,
    severity: Severity,
    message: &'static str,
    needle: &'static str,
}

const FIRES: &[Fire] = &[
    Fire {
        rel: "occbin/e170_two_blocks.mod",
        code: "E170",
        severity: Severity::Error,
        message: "Multiple 'occbin_constraints' blocks are not allowed",
        needle: "name 'INEG'",
    },
    Fire {
        rel: "occbin/e171_three.mod",
        code: "E171",
        severity: Severity::Error,
        message: "only up to two constraints are supported",
        needle: "name 'IRR'",
    },
    Fire {
        rel: "occbin/e172_missing_regime.mod",
        code: "E172",
        severity: Severity::Error,
        message: "relax='ELB' is not defined",
        needle: "[name='policy', bind='ELB']",
    },
    Fire {
        rel: "occbin/e173_bind_no_name.mod",
        code: "E173",
        severity: Severity::Error,
        message: "must have a 'name' tag",
        needle: "[bind='ELB']",
    },
    Fire {
        rel: "occbin/e174_bind_missing.mod",
        code: "E174",
        severity: Severity::Error,
        message: "The 'bind' expression is missing in constraint 'ELB'",
        needle: "name 'ELB'",
    },
    Fire {
        rel: "occbin/e175_no_equation.mod",
        code: "E175",
        severity: Severity::Error,
        message: "No equation has been declared for constraint 'ELB'",
        needle: "'ELB'",
    },
    Fire {
        rel: "occbin/e176_bind_and_relax.mod",
        code: "E176",
        severity: Severity::Error,
        message: "The constraint 'ELB' is both in the 'bind' and 'relax' tags",
        needle: "[name='policy', bind='ELB', relax='ELB']",
    },
    Fire {
        rel: "occbin/e177_regime_dup.mod",
        code: "E177",
        severity: Severity::Error,
        message: "has already been declared for this equation",
        needle: "bind='ELB'] i = 0",
    },
    Fire {
        rel: "occbin/e180_mcp_perp.mod",
        code: "E180",
        severity: Severity::Error,
        message: "Can't have both an 'mcp' tag and a complementarity condition after the perpendicular symbol",
        needle: "mcp",
    },
    Fire {
        rel: "occbin/e181_bind_eq.mod",
        code: "E181",
        severity: Severity::Error,
        message: "The 'bind' expression must be an inequality constraint",
        needle: "i == 0",
    },
    Fire {
        rel: "occbin/e182_lead.mod",
        code: "E182",
        severity: Severity::Error,
        message: "Leads and lags on variables are forbidden in 'occbin_constraints'",
        needle: "i(+1)",
    },
    Fire {
        rel: "occbin/e183_perp_form.mod",
        code: "E183",
        severity: Severity::Error,
        message: "Complementarity condition has an incorrect form",
        needle: "i",
    },
    Fire {
        rel: "occbin/e184_dup_clause.mod",
        code: "E184",
        severity: Severity::Error,
        message: "The 'bind' clause is declared multiple times",
        needle: "bind",
    },
    Fire {
        rel: "occbin/e185_bad_name.mod",
        code: "E185",
        severity: Severity::Error,
        message: "not a valid Occbin constraint name",
        needle: "ELB-1",
    },
    Fire {
        rel: "occbin/w170_mcp.mod",
        code: "W170",
        severity: Severity::Warning,
        message: "Specifying complementarity conditions with the 'mcp' tag is obsolete",
        needle: "mcp",
    },
];

fn check_mod(rel: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(rel);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("fixture missing at {}: {e}", path.display()))
        .replace("\r\n", "\n")
}

fn occbin_diags(model: &dygnosis::Model) -> Vec<Diagnostic> {
    check_occbin(model)
}

fn assert_needle(text: &str, d: &Diagnostic, needle: &str) {
    let start = d.span.start as usize;
    let end = d.span.end as usize;
    assert!(
        end <= text.len() && start <= end,
        "span {}..{} out of range for {}, message={}",
        d.span.start,
        d.span.end,
        d.code,
        d.message
    );
    let slice = &text[start..end];
    assert!(
        slice.contains(needle),
        "{} span {slice:?} should contain {needle:?}, message={}",
        d.code,
        d.message
    );
}

fn find_code<'a>(diags: &'a [Diagnostic], code: &str) -> &'a Diagnostic {
    diags
        .iter()
        .find(|d| d.code == code)
        .unwrap_or_else(|| panic!("missing {code} in {:?}", codes_of(diags)))
}

fn codes_of(diags: &[Diagnostic]) -> Vec<&str> {
    diags.iter().map(|d| d.code.as_str()).collect()
}

fn assert_fire(check: &[Diagnostic], analyze_diags: &[Diagnostic], text: &str, fire: &Fire) {
    for diags in [check, analyze_diags] {
        let d = find_code(diags, fire.code);
        assert_eq!(d.severity, fire.severity, "{} severity", fire.code);
        assert!(
            d.message.contains(fire.message),
            "{} message {:?} should contain {:?}",
            fire.code,
            d.message,
            fire.message
        );
        assert!(d.fix.is_none(), "{} fix should be None", fire.code);
        assert_needle(text, d, fire.needle);
    }
}

#[test]
fn occbin_fires() {
    for fire in FIRES {
        let text = check_mod(fire.rel);
        let model = parse(&text);
        let check = occbin_diags(&model);
        let analyzed = analyze(&model);
        assert_fire(&check, &analyzed, &text, fire);
    }
}

#[test]
fn w170_is_deprecated_warning_without_e180() {
    let text = check_mod("occbin/w170_mcp.mod");
    let model = parse(&text);
    let diags = check_occbin(&model);
    let d = find_code(&diags, "W170");
    assert_eq!(d.severity, Severity::Warning);
    assert_eq!(d.tags, vec![2]);
    assert!(diags.iter().all(|d| d.code != "E180"));
}

#[test]
fn e180_does_not_emit_w170() {
    let model = parse(&check_mod("occbin/e180_mcp_perp.mod"));
    let diags = check_occbin(&model);
    assert!(
        diags.iter().any(|d| d.code == "E180"),
        "expected E180, got {:?}",
        codes_of(&diags)
    );
    assert!(
        diags.iter().all(|d| d.code != "W170"),
        "E180 file must not also emit W170: {:?}",
        codes_of(&diags)
    );
}

#[test]
fn square_has_no_occbin_error() {
    let text = check_mod("occbin/square.mod");
    let model = parse(&text);
    let analyzed = analyze(&model);
    let occbin = OCCBIN_ERRORS
        .iter()
        .copied()
        .filter(|c| analyzed.iter().any(|d| d.code == *c))
        .collect::<Vec<_>>();
    assert!(
        occbin.is_empty(),
        "square.mod must not emit E170–E185, got {occbin:?} from {:?}",
        codes_of(&analyzed)
    );
    assert!(
        analyzed.iter().all(|d| d.code != "W170"),
        "square.mod must not emit W170"
    );
    assert!(
        check_occbin(&model).is_empty(),
        "square.mod check_occbin: {:?}",
        codes_of(&check_occbin(&model))
    );
}

#[test]
fn surprise_has_no_occbin_error() {
    let model = parse(&check_mod("occbin/surprise.mod"));
    let analyzed = analyze(&model);
    let occbin = OCCBIN_ERRORS
        .iter()
        .copied()
        .filter(|c| analyzed.iter().any(|d| d.code == *c))
        .collect::<Vec<_>>();
    assert!(
        occbin.is_empty(),
        "surprise.mod must not emit E170–E185, got {occbin:?} from {:?}",
        codes_of(&analyzed)
    );
    assert!(
        analyzed.iter().all(|d| d.code != "W170"),
        "surprise.mod must not emit W170"
    );
}

#[test]
fn perp_has_no_e183() {
    let model = parse(&check_mod("occbin/perp.mod"));
    let analyzed = analyze(&model);
    assert!(
        analyzed.iter().all(|d| d.code != "E183"),
        "perp.mod must not emit E183, got {:?}",
        codes_of(&analyzed)
    );
    assert!(
        check_occbin(&model).iter().all(|d| d.code != "E183"),
        "perp.mod check_occbin E183: {:?}",
        codes_of(&check_occbin(&model))
    );
}

#[test]
fn delete_occbin_end_is_e001_only() {
    let model = parse(&check_mod("e001/delete_occbin_end.mod"));
    let analyzed = analyze(&model);
    assert!(
        !analyzed.is_empty() && analyzed.iter().all(|d| d.code == "E001"),
        "delete_occbin_end.mod should be E001 only, got {:?}",
        codes_of(&analyzed)
    );
}

#[test]
fn tags_mod_has_no_occbin_codes() {
    let model = parse(&check_mod("equations/tags.mod"));
    let diags = check_occbin(&model);
    let occbin = diags
        .iter()
        .filter(|d| OCCBIN_ERRORS.contains(&d.code.as_str()) || d.code == "W170")
        .map(|d| d.code.as_str())
        .collect::<Vec<_>>();
    assert!(
        occbin.is_empty(),
        "tags.mod must not emit OccBin codes, got {occbin:?}"
    );
}

#[test]
fn e182_family_shapes() {
    let cases = [
        (
            "occbin/e182_exo.mod",
            "Exogenous variable ee cannot be used in 'occbin_constraints'",
            "ee",
        ),
        (
            "occbin/e182_local.mod",
            "Model local variable zz cannot be used in 'occbin_constraints'",
            "zz",
        ),
        (
            "occbin/e182_expectation.mod",
            "The 'expectation' operator is forbidden in 'occbin_constraints'",
            "EXPECTATION(-1)(i)",
        ),
        (
            "occbin/e182_sum.mod",
            "The SUM() operator is forbidden in occbin_constraints block",
            "SUM(i)",
        ),
    ];
    for (rel, message, needle) in cases {
        let text = check_mod(rel);
        let model = parse(&text);
        let check = check_occbin(&model);
        let analyzed = analyze(&model);
        let fire = Fire {
            rel,
            code: "E182",
            severity: Severity::Error,
            message,
            needle,
        };
        assert_fire(&check, &analyzed, &text, &fire);
    }
}

#[test]
fn e185_name_already_used() {
    let text = check_mod("occbin/e185_name_used.mod");
    let model = parse(&text);
    let check = check_occbin(&model);
    let analyzed = analyze(&model);
    let fire = Fire {
        rel: "occbin/e185_name_used.mod",
        code: "E185",
        severity: Severity::Error,
        message: "The name 'occbin_ELB_bind' is already used",
        needle: "ELB",
    };
    assert_fire(&check, &analyzed, &text, &fire);
}

#[test]
fn e177_needle_is_the_later_bind_copy() {
    let text = check_mod("occbin/e177_regime_dup.mod");
    let model = parse(&text);
    let diags = check_occbin(&model);
    let d = find_code(&diags, "E177");
    let index = LineIndex::new(&text);
    let start = index.position(&text, d.span.start);
    let first = text
        .find("[name='policy', bind='ELB']")
        .expect("first bind");
    let second = text[first + 1..]
        .find("[name='policy', bind='ELB']")
        .map(|rel| first + 1 + rel)
        .expect("second bind");
    let first_pos = index.position(&text, first as u32);
    let second_pos = index.position(&text, second as u32);
    assert_ne!(
        start.line, first_pos.line,
        "E177 should not point at the first bind copy"
    );
    assert_eq!(
        start.line, second_pos.line,
        "E177 should point at the later bind copy"
    );
}

#[test]
fn e184_points_at_the_later_bind_keyword() {
    let text = check_mod("occbin/e184_dup_clause.mod");
    let model = parse(&text);
    let diags = check_occbin(&model);
    let d = find_code(&diags, "E184");
    let slice = &text[d.span.start as usize..d.span.end as usize];
    assert_eq!(slice, "bind");
    let first = text.find("bind i <= 0").expect("first bind clause");
    assert!(
        d.span.start as usize > first,
        "E184 should be the later bind, start={} first={first}",
        d.span.start
    );
}
