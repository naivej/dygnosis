use dygnosis::{analyze, parse};

fn diags(src: &str) -> Vec<dygnosis::Diagnostic> {
    analyze(&parse(src))
}

fn w211(src: &str) -> Vec<dygnosis::Diagnostic> {
    diags(src)
        .into_iter()
        .filter(|diag| diag.code == "W211")
        .collect()
}

fn model(exo: &str, equation: &str, command: &str) -> String {
    format!("var y;\n{exo}\nmodel;\n{equation}\nend;\n{command}\n")
}

#[test]
fn lead_on_ordinary_varexo_warns_for_stoch_simul_and_estimation() {
    for command in ["stoch_simul;", "estimation;"] {
        let src = model("varexo e;", "y = e(+1);", command);
        let hits = w211(&src);
        assert_eq!(hits.len(), 1, "{command}: {hits:?}");
        assert_eq!(
            hits[0].message,
            "Exogenous variable 'e' is used with a lead. Check the intended shock timing."
        );
        let slice = &src[hits[0].span.start as usize..hits[0].span.end as usize];
        assert!(slice.contains('e'), "{slice}");
        assert!(hits[0].fix.is_none());
    }
}

#[test]
fn two_leads_are_two_warnings() {
    let src = model("varexo e, u;", "y = e(+1) + u(+2);", "stoch_simul;");
    assert_eq!(w211(&src).len(), 2);
}

#[test]
fn model_local_lead_warns() {
    let src = "\
var y;
varexo e;
model;
  # rho = e(+1);
  y = rho;
end;
stoch_simul;
";
    let hits = w211(src);
    assert_eq!(hits.len(), 1, "{hits:?}");
    let slice = &src[hits[0].span.start as usize..hits[0].span.end as usize];
    assert!(slice.contains("e"), "{slice}");
}

#[test]
fn excluded_contexts_stay_quiet() {
    let quiet = [
        model("varexo e;", "y = e(+1);", ""),
        model("varexo e;", "y = e(-1);", "stoch_simul;"),
        model("varexo e;", "y = e;", "stoch_simul;"),
        model("varexo_det e;", "y = e(+1);", "stoch_simul;"),
        model("varexo e;", "y = e(+1);", "simul;"),
        model("varexo e;", "y = e(+1);", "perfect_foresight_setup;"),
        model("varexo e;", "y = e(+1);", "perfect_foresight_solver;"),
        model(
            "varexo e;",
            "y = e(+1);",
            "perfect_foresight_with_expectation_errors_setup;",
        ),
        model(
            "varexo e;",
            "y = e(+1);",
            "perfect_foresight_with_expectation_errors_solver;",
        ),
        model(
            "varexo e;",
            "y = e(+1);",
            "stoch_simul;\nperfect_foresight_setup;",
        ),
        "\
heterogeneity_dimension h;
var y;
varexo e;
varexo(heterogeneity=h) u;
model;
  y = e;
end;
model(heterogeneity=h);
  u = u(+1);
end;
stoch_simul;
"
        .to_string(),
    ];
    for src in quiet {
        assert!(w211(&src).is_empty(), "{src}\n{:?}", diags(&src));
    }
}

#[test]
fn incomplete_structure_hides_the_lead() {
    let include = "\
@#include \"missing.inc\"
var y;
varexo e;
model;
y = e(+1);
end;
stoch_simul;
";
    assert!(w211(include).is_empty(), "{:?}", diags(include));
    let unknown = "\
var y;
varexo e;
model;
y = e(+1) + @{UNDEF};
end;
stoch_simul;
";
    assert!(w211(unknown).is_empty(), "{:?}", diags(unknown));
}

#[test]
fn parse_error_hides_the_warning() {
    let src = "\
var café;
varexo e;
model;
y = e(+1);
end;
stoch_simul;
";
    let got = diags(src);
    assert!(got.iter().any(|diag| diag.code == "E001"), "{got:?}");
    assert!(got.iter().all(|diag| diag.code != "W211"), "{got:?}");
}
