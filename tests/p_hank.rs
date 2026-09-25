//! P-hank locks: the heterogeneity family parses as written, the shipped
//! generic codes reach the new surfaces exactly where 7.2 refuses, and the
//! Bison-shape E001s carry the pinned sentences.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use dygnosis::model::{HeterogeneityCommandKind, ShockBlockKind, ShockKind};
use dygnosis::{
    analyze, classify_variable_timing, dynare_equations, dynare_expand, dynare_model_info,
    expand_report, find_preprocessor, parse, run_preprocessor, structure_summary, JsonStage,
    TimingClass,
};

fn fixture(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/p_hank")
        .join(name);
    let source = std::fs::read_to_string(&path).unwrap();
    assert!(source.starts_with("// inventory: "), "{}", path.display());
    source
}

fn pinned_binary() -> Option<PathBuf> {
    let pinned = PathBuf::from("C:/dynare/7.2/preprocessor/dynare-preprocessor.exe");
    if pinned.is_file() {
        return Some(pinned);
    }
    find_preprocessor(None).filter(|path| {
        path.components()
            .any(|part| part.as_os_str().to_string_lossy() == "7.2")
    })
}

fn official_check(source: &str, binary: &Path) -> (bool, String) {
    let result = run_preprocessor(
        source,
        binary,
        None,
        Duration::from_secs(30),
        JsonStage::Check,
    );
    (
        result.success,
        format!("{} {}", result.raw_stdout, result.raw_stderr),
    )
}

fn quiet_file(name: &str, binary: Option<&Path>) -> dygnosis::model::Model {
    let source = fixture(name);
    let model = parse(&source);
    let diagnostics = analyze(&model);
    assert!(
        !diagnostics.iter().any(|diag| diag.code == "E001"),
        "{name}: {diagnostics:?}"
    );
    assert!(
        !diagnostics
            .iter()
            .any(|diag| diag.severity == dygnosis::Severity::Error),
        "{name}: unexpected Error {diagnostics:?}"
    );
    assert!(
        !diagnostics
            .iter()
            .any(|diag| diag.severity == dygnosis::Severity::Warning),
        "{name}: unexpected Warning {diagnostics:?}"
    );
    if let Some(binary) = binary {
        let (accepted, report) = official_check(&source, binary);
        assert!(accepted, "7.2 refused {name}: {report}");
    }
    model
}

fn errors(source: &str) -> Vec<dygnosis::Diagnostic> {
    analyze(&parse(source))
        .into_iter()
        .filter(|diag| diag.severity == dygnosis::Severity::Error)
        .collect()
}

fn declared_dimension(model: &dygnosis::model::Model, name: &str) -> bool {
    model
        .endogenous
        .iter()
        .chain(&model.exogenous)
        .chain(&model.parameters)
        .any(|decl| {
            model.name(decl.name) == name
                && decl
                    .heterogeneity
                    .is_some_and(|(dim, _)| model.name(dim) == "d")
        })
}

#[test]
fn agent_views_keep_aggregate_and_heterogeneous_equations_separate() {
    let source = fixture("accepted_family.mod");
    let model = parse(&source);
    let timing = classify_variable_timing(&model);
    assert_eq!(timing["yh"].class, TimingClass::Predetermined);
    assert_eq!(timing["yh"].offsets, [-1, 0]);
    assert_eq!(structure_summary(&model).endogenous, 2);

    let info = dynare_model_info(&source, None, None);
    assert_eq!(info["n_endogenous"], 2);
    assert_eq!(info["n_equations"], 2);
    assert_eq!(info["endogenous"], serde_json::json!(["y", "c"]));
    let dimension = &info["heterogeneity_dimensions"][0];
    assert_eq!(dimension["dimension"], "d");
    assert_eq!(dimension["n_endogenous"], 1);
    assert_eq!(dimension["n_equations"], 1);
    assert_eq!(dimension["predetermined"], serde_json::json!(["yh"]));

    let equations = dynare_equations(&source, None, None, None, None);
    assert_eq!(equations["equations"].as_array().unwrap().len(), 2);
    let block = &equations["heterogeneous_equations"][0];
    assert_eq!(block["dimension"], "d");
    assert_eq!(block["equations"].as_array().unwrap().len(), 1);
    let row = &block["equations"][0];
    assert_eq!(row["lhs"], "yh");
    assert_eq!(row["origin"]["line"], 15);
    assert!(row["idents"].as_array().unwrap().iter().any(|ident| {
        ident["name"] == "yh" && ident["timing"] == -1 && ident["timing_class"] == "predetermined"
    }));

    let named_source =
        source.replacen("yh = ph*yh(-1)", "[name='household law'] yh = ph*yh(-1)", 1);
    let named = dynare_equations(&named_source, None, None, Some("household law"), None);
    assert!(named["equations"].as_array().unwrap().is_empty());
    assert_eq!(
        named["heterogeneous_equations"][0]["equations"][0]["lhs"],
        "yh"
    );
    assert!(named["heterogeneous_equations"][0]["equations"][0]["explain"].is_string());
}

#[test]
fn heterogeneous_equation_origin_resolves_an_include() {
    let files = HashMap::from([
        (
            "main.mod".to_string(),
            "heterogeneity_dimension d;\nvar y;\nvar(heterogeneity=d) a;\nmodel; y=SUM(a); end;\nmodel(heterogeneity=d);\n@#include \"het.inc\"\nend;\n".to_string(),
        ),
        ("het.inc".to_string(), "a = a(-1);\n".to_string()),
    ]);
    let result = dynare_equations(
        &files["main.mod"],
        Some("main.mod"),
        Some(&files),
        None,
        None,
    );
    let row = &result["heterogeneous_equations"][0]["equations"][0];
    assert_eq!(row["origin_uri"], "het.inc");
    assert_eq!(row["origin"]["line"], 1);

    let expanded = dynare_expand(&files["main.mod"], Some("main.mod"), Some(&files));
    assert_eq!(expanded["n_equations"], 2);
    assert_eq!(expanded["origins"][1]["scope"], "heterogeneous");
    assert_eq!(expanded["origins"][1]["origin_uri"], "het.inc");
}

#[test]
fn expanded_hank_counts_both_trees_in_file_order() {
    let source = "heterogeneity_dimension d;\nvar y;\nvar(heterogeneity=d) a;\nmodel(heterogeneity=d);\na = a(-1);\nend;\nmodel;\ny = SUM(a);\nend;\n";
    let report = expand_report(source);
    assert_eq!(report.n_equations, 2);
    assert_eq!(report.origins.len(), 2);
    assert_eq!(report.aggregate_origins.len(), 1);
    assert_eq!(report.heterogeneous_origins[0].len(), 1);
    assert_eq!(report.origins[0].dimension.as_deref(), Some("d"));
    assert_eq!(report.origins[1].dimension, None);

    let expanded = dynare_expand(source, None, None);
    assert_eq!(expanded["n_equations"], 2);
    assert_eq!(expanded["n_aggregate_equations"], 1);
    assert_eq!(expanded["n_heterogeneous_equations"], 1);
    assert_eq!(expanded["heterogeneity_dimensions"][0]["n_equations"], 1);
    assert_eq!(expanded["origins"][0]["index"], 0);
    assert_eq!(expanded["origins"][0]["scope"], "heterogeneous");
    assert_eq!(expanded["origins"][0]["dimension"], "d");
    assert_eq!(expanded["origins"][0]["scope_index"], 0);
    assert_eq!(expanded["origins"][1]["index"], 1);
    assert_eq!(expanded["origins"][1]["scope"], "aggregate");
    assert_eq!(expanded["origins"][1]["scope_index"], 0);
    let effective = expanded["effective_text"].as_str().unwrap();
    assert!(
        effective.contains("model(heterogeneity = d)"),
        "{effective}"
    );
    assert!(effective.contains("model ; y = SUM(a)"), "{effective}");

    let equations = dynare_equations(source, None, None, None, None);
    assert_eq!(equations["equations"][0]["origin"]["line"], 8);
    assert_eq!(
        equations["heterogeneous_equations"][0]["equations"][0]["origin"]["line"],
        5
    );
}

#[test]
fn accepted_family_keeps_written_structure() {
    let binary = pinned_binary();
    let model = quiet_file("accepted_family.mod", binary.as_deref());

    // One dimension record per name, with the statement span.
    assert_eq!(model.heterogeneity_dimensions.len(), 1);
    let dim = &model.heterogeneity_dimensions[0];
    assert_eq!(model.name(dim.name), "d");
    assert_eq!(
        &model.source[dim.span.start as usize..dim.span.end as usize],
        "heterogeneity_dimension d;"
    );

    // The three heterogeneous declaration forms join the ordinary vectors.
    assert!(declared_dimension(&model, "yh"));
    assert!(declared_dimension(&model, "eh"));
    assert!(declared_dimension(&model, "ph"));
    assert!(
        !model
            .endogenous
            .iter()
            .any(|decl| model.name(decl.name) == "y" && decl.heterogeneity.is_some()),
        "aggregate declarations carry no dimension"
    );

    // Aggregate equations stay aggregate: 2 written, SUM visible as a Call.
    assert_eq!(model.equations.len(), 2);
    assert_eq!(model.summary().n_model_equations, 2);
    let sum_in_aggregate = model.equations.iter().any(|eq| {
        model.exprs.iter().any(|(_, expr)| {
            let within = expr.span.start >= eq.span.start && expr.span.end <= eq.span.end;
            within
                && matches!(&expr.kind, dygnosis::ExprKind::Call { callee, .. }
                    if model.name(*callee).eq_ignore_ascii_case("sum"))
        })
    });
    assert!(
        sum_in_aggregate,
        "SUM(yh) must appear in the aggregate walk"
    );

    // One heterogeneous body per dimension, its own equation list.
    assert_eq!(model.heterogeneous_models.len(), 1);
    let block = &model.heterogeneous_models[0];
    assert_eq!(model.name(block.dimension), "d");
    assert_eq!(block.equations.len(), 1);
    assert!(block.equations[0].complementarity.is_some());
    assert_eq!(
        &model.source[block.span.start as usize..block.span.end as usize]
            .trim()
            .lines()
            .next()
            .unwrap(),
        &"model(heterogeneity=d);"
    );

    // Heterogeneous shock rows live on their blocks, not in the flat view.
    let het_blocks: Vec<_> = model
        .shock_blocks
        .iter()
        .filter(|block| block.kind == ShockBlockKind::Heterogeneous)
        .collect();
    assert_eq!(het_blocks.len(), 3);
    assert!(model.shock_stmts.is_empty());
    assert!(het_blocks[0].options.heterogeneity.is_some());
    assert!(!het_blocks[0].options.overwrite);
    assert!(het_blocks[1].options.overwrite);
    assert!(het_blocks[2].options.overwrite);
    let kinds: Vec<String> = het_blocks[0]
        .stochastic
        .iter()
        .map(|stmt| match stmt.kind {
            ShockKind::Stderr(name) => format!("stderr:{}", model.name(name)),
            ShockKind::Cov(ref names) => format!("cov:{}", names.len()),
            ShockKind::Corr { a, b } => {
                format!("corr:{}:{}", model.name(a), model.name(b))
            }
            ShockKind::Var(name) => format!("var:{}", model.name(name)),
            _ => panic!("unexpected row kind"),
        })
        .collect();
    assert_eq!(
        kinds,
        ["stderr:eh", "cov:2", "corr:eh3:eh4", "var:eh5"],
        "both overwrite orders and all four row kinds parse"
    );
    // The stderr rows keep their written tree; the `=` rows fold.
    assert!(het_blocks[0]
        .stochastic
        .iter()
        .all(|stmt| stmt.rhs_expr.is_some()));
    assert!(het_blocks[0].stochastic[1].rhs.is_some());
    assert!(het_blocks[0].stochastic[2].rhs.is_some());
    assert!(het_blocks[0].stochastic[3].rhs.is_some());
    assert_eq!(het_blocks[1].stochastic.len(), 1);
    assert_eq!(het_blocks[2].stochastic.len(), 1);

    // The four commands, their written options, and the simulate list.
    assert_eq!(model.heterogeneity_commands.len(), 4);
    let kinds: Vec<HeterogeneityCommandKind> = model
        .heterogeneity_commands
        .iter()
        .map(|stmt| stmt.kind)
        .collect();
    assert_eq!(
        kinds,
        [
            HeterogeneityCommandKind::LoadSteadyState,
            HeterogeneityCommandKind::ComputeSteadyState,
            HeterogeneityCommandKind::Solve,
            HeterogeneityCommandKind::Simulate,
        ]
    );
    let load = &model.heterogeneity_commands[0];
    assert_eq!(load.options.len(), 3);
    assert_eq!(load.options[0].name, "filename");
    assert_eq!(
        load.options[0]
            .value
            .as_ref()
            .map(|(text, _)| text.as_str()),
        Some("'ss.txt'")
    );
    assert_eq!(load.options[1].name, "variable");
    assert_eq!(load.options[2].name, "tolf");
    let solve = &model.heterogeneity_commands[2];
    assert_eq!(solve.options.len(), 1);
    assert_eq!(solve.options[0].name, "truncation_horizon");
    let simulate = &model.heterogeneity_commands[3];
    assert_eq!(simulate.options.len(), 1);
    assert_eq!(simulate.options[0].name, "irf");
    let names: Vec<&str> = simulate
        .simulate_names
        .iter()
        .map(|(name, _)| model.name(*name))
        .collect();
    assert_eq!(names, ["y", "c"]);
    assert!(simulate.span.end > simulate.options[0].name_span.end);
}

#[test]
fn two_dimensions_stay_separate_records() {
    let binary = pinned_binary();
    let model = quiet_file("quiet_two_dimensions.mod", binary.as_deref());
    let names: Vec<&str> = model
        .heterogeneity_dimensions
        .iter()
        .map(|dim| model.name(dim.name))
        .collect();
    assert_eq!(names, ["d1", "d2"]);
    let dims: Vec<&str> = model
        .heterogeneous_models
        .iter()
        .map(|block| model.name(block.dimension))
        .collect();
    assert_eq!(dims, ["d1", "d2"]);
    assert_eq!(model.equations.len(), 1);
    assert_eq!(
        model
            .heterogeneous_models
            .iter()
            .map(|block| block.equations.len())
            .collect::<Vec<_>>(),
        [1, 1]
    );
}

#[test]
fn perp_spellings_parse_in_heterogeneous_bodies() {
    let binary = pinned_binary();
    let model = quiet_file("quiet_perp_spellings.mod", binary.as_deref());
    let block = &model.heterogeneous_models[0];
    assert_eq!(block.equations.len(), 2);
    for eq in &block.equations {
        let comp = eq.complementarity.as_ref().expect("perpendicular stored");
        assert!(comp.matched.is_some(), "{}", comp.text);
    }
    assert!(!analyze(&parse(&fixture("quiet_perp_spellings.mod")))
        .iter()
        .any(|diag| diag.code == "E183"));
}

#[test]
fn unknown_dimension_is_stored_and_stays_silent() {
    let binary = pinned_binary();
    let source = fixture("fire_dim_unknown_var.mod");
    let model = parse(&source);
    assert!(declared_dimension(&model, "yh"));
    let diagnostics = analyze(&model);
    assert!(
        !diagnostics
            .iter()
            .any(|diag| diag.severity == dygnosis::Severity::Error),
        "S054 is 02's; slice 01 stores the shape: {diagnostics:?}"
    );
    assert!(!diagnostics.iter().any(|diag| diag.code == "E001"));
    if let Some(ref binary) = binary {
        let (accepted, report) = official_check(&source, binary);
        assert!(!accepted);
        assert!(
            report.contains("Unknown heterogeneity dimension: d"),
            "{report}"
        );
    }
}

#[test]
fn bison_fires_lock_official_sentences() {
    let binary = pinned_binary();
    for (name, message, offending) in [
        (
            "fire_het_model_mixed_options.mod",
            "syntax error, unexpected COMMA, expecting ')'",
            ",",
        ),
        (
            "fire_var_het_mixed_options.mod",
            "syntax error, unexpected COMMA, expecting ')'",
            ",",
        ),
        (
            "fire_het_model_empty.mod",
            "syntax error, unexpected END",
            "end",
        ),
        (
            "fire_shocks_double_overwrite.mod",
            "syntax error, unexpected COMMA, expecting ')'",
            ",",
        ),
    ] {
        let source = fixture(name);
        let diagnostics = analyze(&parse(&source));
        let ours = diagnostics
            .iter()
            .find(|diag| diag.code == "E001")
            .unwrap_or_else(|| panic!("missing E001 in {name}: {diagnostics:?}"));
        assert_eq!(ours.message, message, "{name}");
        assert_eq!(
            &source[ours.span.start as usize..ours.span.end as usize],
            offending,
            "{name}"
        );
        if let Some(ref binary) = binary {
            let (accepted, report) = official_check(&source, binary);
            assert!(!accepted, "7.2 accepted {name}");
            assert!(report.contains(message), "{name}: {report}");
        }
    }
}

#[test]
fn unknown_symbol_reaches_heterogeneous_equations() {
    let binary = pinned_binary();
    let source = fixture("fire_het_unknown_symbol.mod");
    let diagnostics = analyze(&parse(&source));
    let ours = diagnostics
        .iter()
        .find(|diag| diag.code == "E020")
        .unwrap_or_else(|| panic!("missing E020: {diagnostics:?}"));
    assert_eq!(
        &source[ours.span.start as usize..ours.span.end as usize],
        "zz"
    );
    if let Some(ref binary) = binary {
        let (accepted, report) = official_check(&source, binary);
        assert!(!accepted);
        assert!(report.contains("Unknown symbol: zz"), "{report}");
    }
}

#[test]
fn duplicate_declarations_match_official_wording() {
    let binary = pinned_binary();
    // The E030 files carry an official ERROR, so `onlyjson` runs exit 1; the
    // W031 files print only the official warning and exit 0 there.
    for (name, code, message, expect_refusal) in [
        (
            "fire_dim_cross_kind.mod",
            "E030",
            "Symbol yh declared twice with different types!",
            true,
        ),
        (
            "fire_dim_same_kind_twice.mod",
            "W031",
            "Symbol yh declared twice.",
            false,
        ),
        (
            "fire_dim_two_dims_twice.mod",
            "W031",
            "Symbol yh declared twice.",
            false,
        ),
    ] {
        let source = fixture(name);
        let diagnostics = analyze(&parse(&source));
        let ours = diagnostics
            .iter()
            .find(|diag| diag.code == code)
            .unwrap_or_else(|| panic!("missing {code} in {name}: {diagnostics:?}"));
        assert_eq!(ours.message, message, "{name}");
        assert_eq!(
            &source[ours.span.start as usize..ours.span.end as usize],
            "yh",
            "{name}"
        );
        if let Some(ref binary) = binary {
            let (accepted, report) = official_check(&source, binary);
            if expect_refusal {
                assert!(!accepted, "7.2 accepted {name}");
            }
            assert!(report.contains(message), "{name}: {report}");
        }
    }
}

#[test]
fn command_option_twice_reaches_e271() {
    let binary = pinned_binary();
    let source = fixture("fire_command_option_twice.mod");
    let model = parse(&source);
    assert_eq!(model.heterogeneity_commands.len(), 1);
    assert_eq!(model.heterogeneity_commands[0].options.len(), 2);
    let diagnostics = analyze(&model);
    let ours = diagnostics
        .iter()
        .find(|diag| diag.code == "E271")
        .unwrap_or_else(|| panic!("missing E271: {diagnostics:?}"));
    // 7.2 keys the twice-refusal on the option's internal name.
    assert_eq!(ours.message, "option check.tolf declared twice");
    assert_eq!(
        &source[ours.span.start as usize..ours.span.end as usize],
        "tolf"
    );
    if let Some(ref binary) = binary {
        let (accepted, report) = official_check(&source, binary);
        assert!(!accepted);
        assert!(
            report.contains("option check.tolf declared twice"),
            "{report}"
        );
    }
}

#[test]
fn simulate_trailing_list_stays_quiet() {
    let binary = pinned_binary();
    let source = fixture("quiet_simulate_unknown_name.mod");
    let model = parse(&source);
    assert_eq!(model.heterogeneity_commands.len(), 1);
    let names: Vec<&str> = model.heterogeneity_commands[0]
        .simulate_names
        .iter()
        .map(|(name, _)| model.name(*name))
        .collect();
    assert_eq!(names, ["zz"]);
    let diagnostics = analyze(&model);
    assert!(
        !diagnostics.iter().any(|diag| diag.code == "E058"),
        "E058 must stay quiet on the simulate list: {diagnostics:?}"
    );
    assert!(
        !diagnostics
            .iter()
            .any(|diag| diag.severity == dygnosis::Severity::Error),
        "{diagnostics:?}"
    );
    if let Some(ref binary) = binary {
        let (accepted, report) = official_check(&source, binary);
        assert!(accepted, "7.2 refused: {report}");
    }
}

#[test]
fn sum_bans_keep_firing_on_heterogeneous_files() {
    let binary = pinned_binary();
    for (name, code, message) in [
        (
            "fire_epilogue_sum.mod",
            "E293",
            "The SUM() operator is forbidden in epilogue block",
        ),
        (
            "fire_occbin_sum.mod",
            "E182",
            "The SUM() operator is forbidden in occbin_constraints block",
        ),
    ] {
        let source = fixture(name);
        let diagnostics = analyze(&parse(&source));
        let ours = diagnostics
            .iter()
            .find(|diag| diag.code == code)
            .unwrap_or_else(|| panic!("missing {code} in {name}: {diagnostics:?}"));
        assert_eq!(ours.message, message, "{name}");
        // One code on one shape: the bind-tagged equation keeps the
        // missing-constraint refusal out of the occbin fire, and the
        // complete bind/relax regime pair keeps E172 out of it.
        if name == "fire_occbin_sum.mod" {
            assert!(
                !diagnostics.iter().any(|diag| diag.code == "E175"),
                "E175 must stay quiet: {diagnostics:?}"
            );
            assert!(
                !diagnostics.iter().any(|diag| diag.code == "E172"),
                "E172 must stay quiet: {diagnostics:?}"
            );
            assert!(
                !diagnostics
                    .iter()
                    .any(|diag| diag.severity == dygnosis::Severity::Warning),
                "no Warning beside the SUM ban: {diagnostics:?}"
            );
        }
        if let Some(ref binary) = binary {
            let (accepted, report) = official_check(&source, binary);
            assert!(!accepted, "7.2 accepted {name}");
            assert!(report.contains(message), "{name}: {report}");
        }
    }
}

#[test]
fn dsge_prior_weight_keeps_refusal_under_heterogeneity() {
    let binary = pinned_binary();
    let source = fixture("fire_dsge_weight_het.mod");
    let diagnostics = errors(&source);
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0].code, "E303");
    assert_eq!(
        diagnostics[0].message,
        "dsge_prior_weight cannot be declared as a parameter. Use the dsge_var option in the estimation statement instead."
    );
    if let Some(ref binary) = binary {
        let (accepted, report) = official_check(&source, binary);
        assert!(!accepted);
        assert!(report.contains(&diagnostics[0].message), "{report}");
    }
}

#[test]
fn heterogeneous_pound_locals_follow_the_official_order() {
    let binary = pinned_binary();

    // Use before the `#` definition: 7.2 refuses while parsing the body.
    let source = fixture("fire_het_pound_early_use.mod");
    let diagnostics = errors(&source);
    let ours = diagnostics
        .iter()
        .find(|diag| diag.code == "E025")
        .unwrap_or_else(|| panic!("missing E025: {diagnostics:?}"));
    assert_eq!(
        ours.message,
        "a has wrong type or was already used on the right-hand side. You cannot use it on the left-hand side of a pound ('#') expression"
    );
    if let Some(ref binary) = binary {
        let (accepted, report) = official_check(&source, binary);
        assert!(!accepted);
        assert!(report.contains(&ours.message), "{report}");
    }

    // A repeated `#` name in one body: 7.2 refuses while parsing.
    let source = fixture("fire_het_pound_twice.mod");
    let diagnostics = errors(&source);
    let ours = diagnostics
        .iter()
        .find(|diag| diag.code == "E030")
        .unwrap_or_else(|| panic!("missing E030: {diagnostics:?}"));
    assert_eq!(ours.message, "Local model variable a declared twice.");
    if let Some(ref binary) = binary {
        let (accepted, report) = official_check(&source, binary);
        assert!(!accepted);
        assert!(report.contains(&ours.message), "{report}");
    }

    // A defined-then-used local in one heterogeneous body is accepted.
    quiet_file("quiet_het_pound_local.mod", binary.as_deref());
}

#[test]
fn complementarity_form_reaches_heterogeneous_bodies() {
    let binary = pinned_binary();
    let source = fixture("fire_het_comp_bad.mod");
    let diagnostics = errors(&source);
    let ours = diagnostics
        .iter()
        .find(|diag| diag.code == "E183")
        .unwrap_or_else(|| panic!("missing E183: {diagnostics:?}"));
    assert_eq!(
        ours.message,
        "Complementarity condition has an incorrect form"
    );
    if let Some(ref binary) = binary {
        let (accepted, report) = official_check(&source, binary);
        assert!(!accepted);
        assert!(
            report.contains("Complementarity condition has an incorrect form"),
            "{report}"
        );
    }
}

#[test]
fn merged_equation_reach_into_heterogeneous_bodies() {
    let binary = pinned_binary();
    quiet_file("quiet_het_two_equations.mod", binary.as_deref());
    let source = fixture("fire_het_merged_equations.mod");
    let diagnostics = analyze(&parse(&source));
    let ours = diagnostics
        .iter()
        .find(|diag| diag.code == "E001")
        .unwrap_or_else(|| panic!("missing E001: {diagnostics:?}"));
    assert!(
        ours.message
            .starts_with("Equation appears to contain multiple equations merged"),
        "{}",
        ours.message
    );
    assert!(ours.message.contains("'c2'"), "{}", ours.message);
    if let Some(ref binary) = binary {
        let (accepted, report) = official_check(&source, binary);
        assert!(!accepted);
        assert!(
            report.contains("syntax error, unexpected IDENTIFIER"),
            "{report}"
        );
    }
}

#[test]
fn unused_exogenous_tracking_counts_heterogeneous_bodies() {
    let binary = pinned_binary();
    // A heterogeneous exogenous used only in the heterogeneous body is used,
    // and a heterogeneous-declared exogenous is never refused (the binary
    // accepts one that nothing uses), so E021 stays quiet.
    quiet_file("quiet_het_exo_body_use.mod", binary.as_deref());
    // A plain exogenous truly unused refuses with the official sentence.
    let source = fixture("fire_e021_unused_exogenous.mod");
    let diagnostics = analyze(&parse(&source));
    let ours = diagnostics
        .iter()
        .find(|diag| diag.code == "E021")
        .unwrap_or_else(|| panic!("missing E021: {diagnostics:?}"));
    assert_eq!(
        ours.message,
        "e2 not used in model block. To bypass this error, use the `nostrict` option. This may lead to crashes or unexpected behavior."
    );
    if let Some(ref binary) = binary {
        let (accepted, report) = official_check(&source, binary);
        assert!(!accepted);
        assert!(report.contains(&ours.message), "{report}");
    }
}

#[test]
fn det_exo_lead_reaches_heterogeneous_bodies() {
    let binary = pinned_binary();
    let source = fixture("fire_e024_det_lead.mod");
    let diagnostics = analyze(&parse(&source));
    let ours = diagnostics
        .iter()
        .find(|diag| diag.code == "E024")
        .unwrap_or_else(|| panic!("missing E024: {diagnostics:?}"));
    assert_eq!(
        ours.message,
        "Exogenous deterministic variable ed cannot be given a lead or a lag"
    );
    if let Some(ref binary) = binary {
        let (accepted, report) = official_check(&source, binary);
        assert!(!accepted);
        // Our E024 keeps its existing no-period wording; the official sentence
        // carries a trailing full stop.
        assert!(
            report.contains("Exogenous deterministic variable ed cannot be given a lead or a lag."),
            "{report}"
        );
    }
}

#[test]
fn simulate_comma_list_is_fully_stored() {
    let binary = pinned_binary();
    let source = fixture("quiet_simulate_comma_names.mod");
    let model = quiet_file("quiet_simulate_comma_names.mod", binary.as_deref());
    assert_eq!(model.heterogeneity_commands.len(), 1);
    let names: Vec<&str> = model.heterogeneity_commands[0]
        .simulate_names
        .iter()
        .map(|(name, _)| model.name(*name))
        .collect();
    assert_eq!(names, ["y", "c"]);
    assert!(model.heterogeneity_commands[0]
        .simulate_names
        .iter()
        .all(|(_, span)| span.end > span.start));
    assert!(!analyze(&parse(&source))
        .iter()
        .any(|diag| diag.code == "E058"));
}

#[test]
fn count_gap_compares_aggregate_tree_with_plain_endogenous() {
    let binary = pinned_binary();
    // One aggregate equation against two plain endogenous, with the opposite
    // heterogeneous gap (two equations, one heterogeneous endogenous). The
    // official transform-stage count compares the aggregate tree with the
    // plain-endogenous symbol count, so the gaps cannot cancel.
    let source = fixture("fire_count_no_cancel.mod");
    let diagnostics = analyze(&parse(&source));
    let ours = diagnostics
        .iter()
        .find(|diag| diag.code == "W013")
        .unwrap_or_else(|| panic!("missing W013: {diagnostics:?}"));
    assert!(
        ours.message
            .contains("1 equation(s) but 2 endogenous variable(s)"),
        "{}",
        ours.message
    );
    if let Some(ref binary) = binary {
        // The refusal lives in transformPass, so it only runs at the transform
        // stage: with `onlyjson`, `json=check` exits before it.
        let transform = run_preprocessor(
            &source,
            binary,
            None,
            Duration::from_secs(30),
            JsonStage::Transform,
        );
        let report = format!("{} {}", transform.raw_stdout, transform.raw_stderr);
        assert!(!transform.success);
        assert!(report.contains("z not used in the model block"), "{report}");
    }
}

#[test]
fn reworked_d_open_neighbour_is_accepted() {
    let binary = pinned_binary();
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/d_open/quiet_heterogeneity.mod");
    let source = std::fs::read_to_string(&path)
        .unwrap()
        .replace("\r\n", "\n");
    let model = parse(&source);
    let diagnostics = analyze(&model);
    assert!(
        !diagnostics.iter().any(|diag| diag.code == "E001"),
        "{diagnostics:?}"
    );
    assert!(
        !diagnostics
            .iter()
            .any(|diag| diag.severity == dygnosis::Severity::Error),
        "{diagnostics:?}"
    );
    assert_eq!(model.heterogeneity_dimensions.len(), 1);
    assert_eq!(model.heterogeneous_models.len(), 1);
    assert_eq!(model.equations.len(), 2);
    if let Some(ref binary) = binary {
        let (accepted, report) = official_check(&source, binary);
        assert!(accepted, "7.2 refused the reworked neighbour: {report}");
    }
}

#[test]
fn installed_hank_example_keeps_written_view() {
    let binary = pinned_binary();
    let path = Path::new("C:/dynare/7.2/examples/heterogeneity/hank_one_asset.mod");
    let Ok(source) = std::fs::read_to_string(path) else {
        return;
    };
    let model = parse(&source);
    let diagnostics = analyze(&model);
    assert!(
        !diagnostics.iter().any(|diag| diag.code == "E001"),
        "{diagnostics:?}"
    );
    let dims: Vec<&str> = model
        .heterogeneity_dimensions
        .iter()
        .map(|dim| model.name(dim.name))
        .collect();
    assert_eq!(dims, ["households"]);
    assert_eq!(model.heterogeneous_models.len(), 1);
    assert_eq!(
        model.name(model.heterogeneous_models[0].dimension),
        "households"
    );
    assert_eq!(model.heterogeneous_models[0].equations.len(), 5);
    assert_eq!(model.summary().n_model_equations, 7);
    let het_endo: Vec<&str> = model
        .endogenous
        .iter()
        .filter(|decl| decl.heterogeneity.is_some())
        .map(|decl| model.name(decl.name))
        .collect();
    assert_eq!(het_endo, ["c", "n", "ns", "a", "Va"]);
    assert!(model
        .exogenous
        .iter()
        .any(|decl| model.name(decl.name) == "e" && decl.heterogeneity.is_some()));
    if let Some(ref binary) = binary {
        let (accepted, report) = official_check(&source, binary);
        assert!(accepted, "7.2 refused the installed example: {report}");
    }
}

#[test]
fn pound_locals_are_scoped_to_one_data_tree() {
    let binary = pinned_binary();
    // The same `#` name in the aggregate model and in each dimension is a
    // different `AddLocalVariable` table. 7.2 accepts the file.
    quiet_file("quiet_local_per_tree.mod", binary.as_deref());

    // A use before every definition is `Unknown symbol`, even when a later
    // tree defines the name. It is not the pound-LHS sentence.
    let source = fixture("fire_e020_other_tree_local.mod");
    let diagnostics = analyze(&parse(&source));
    assert!(
        diagnostics.iter().any(|diag| diag.code == "E020"),
        "missing E020: {diagnostics:?}"
    );
    assert!(
        !diagnostics
            .iter()
            .any(|diag| diag.code == "E025" || diag.code == "E030"),
        "other-tree local must not be E025 or E030: {diagnostics:?}"
    );
    if let Some(ref binary) = binary {
        let (accepted, report) = official_check(&source, binary);
        assert!(!accepted);
        assert!(report.contains("Unknown symbol: a"), "{report}");
    }

    // The pound-LHS refusal still fires inside the tree that defines the name,
    // and the other tree's own definition is not a second declaration.
    let source = fixture("fire_e025_same_tree_other_def.mod");
    let diagnostics = analyze(&parse(&source));
    let ours = diagnostics
        .iter()
        .find(|diag| diag.code == "E025")
        .unwrap_or_else(|| panic!("missing E025: {diagnostics:?}"));
    assert_eq!(
        ours.message,
        "a has wrong type or was already used on the right-hand side. You cannot use it on the left-hand side of a pound ('#') expression"
    );
    assert!(
        !diagnostics.iter().any(|diag| diag.code == "E030"),
        "cross-tree `#a` is not declared twice: {diagnostics:?}"
    );
    if let Some(ref binary) = binary {
        let (accepted, report) = official_check(&source, binary);
        assert!(!accepted);
        assert!(report.contains(&ours.message), "{report}");
    }

    // A later tree that uses a local defined only in an earlier tree crashes
    // with no ERROR sentence. Stay quiet.
    let source = fixture("quiet_cross_tree_local_use.mod");
    let diagnostics = analyze(&parse(&source));
    assert!(
        !diagnostics
            .iter()
            .any(|diag| diag.severity == dygnosis::Severity::Error
                || diag.severity == dygnosis::Severity::Warning),
        "{diagnostics:?}"
    );
    if let Some(ref binary) = binary {
        let result = run_preprocessor(
            &source,
            binary,
            None,
            Duration::from_secs(30),
            JsonStage::Check,
        );
        let report = format!("{} {}", result.raw_stdout, result.raw_stderr);
        assert!(report.contains("UnknownLocalVariableException"), "{report}");
        assert!(!report.contains("ERROR:"), "{report}");
    }
}

#[test]
fn heterogeneous_only_model_still_checks_unused_names() {
    let binary = pinned_binary();
    // An unused heterogeneous endogenous is not in `endo_nbr`. Accepted.
    quiet_file("quiet_unused_het_endo.mod", binary.as_deref());

    let source = fixture("fire_e021_het_only.mod");
    let diagnostics = analyze(&parse(&source));
    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|diag| diag.severity == dygnosis::Severity::Error)
        .collect();
    assert_eq!(errors.len(), 1, "{diagnostics:?}");
    assert_eq!(errors[0].code, "E021");
    assert_eq!(
        errors[0].message,
        "e2 not used in model block. To bypass this error, use the `nostrict` option. This may lead to crashes or unexpected behavior."
    );
    if let Some(ref binary) = binary {
        let (accepted, report) = official_check(&source, binary);
        assert!(!accepted);
        assert!(report.contains(&errors[0].message), "{report}");
    }

    let source = fixture("fire_w022_het_only.mod");
    let diagnostics = analyze(&parse(&source));
    assert!(
        !diagnostics
            .iter()
            .any(|diag| diag.severity == dygnosis::Severity::Error),
        "{diagnostics:?}"
    );
    let ours = diagnostics
        .iter()
        .find(|diag| diag.code == "W022")
        .unwrap_or_else(|| panic!("missing W022: {diagnostics:?}"));
    assert_eq!(ours.message, "Parameter(s) p not used in the model");
    if let Some(ref binary) = binary {
        let (accepted, report) = official_check(&source, binary);
        assert!(accepted, "{report}");
        assert!(
            report.contains("Parameter(s) p  not used in the model"),
            "{report}"
        );
    }

    let source = fixture("fire_w020_het_only.mod");
    let diagnostics = analyze(&parse(&source));
    let ours = diagnostics
        .iter()
        .find(|diag| diag.code == "W020")
        .unwrap_or_else(|| panic!("missing W020: {diagnostics:?}"));
    assert_eq!(
        ours.message,
        "Endogenous variable 'y' is declared but never referenced in the model block."
    );
    assert!(
        !diagnostics
            .iter()
            .any(|diag| diag.severity == dygnosis::Severity::Error),
        "{diagnostics:?}"
    );
    if let Some(ref binary) = binary {
        let transform = run_preprocessor(
            &source,
            binary,
            None,
            Duration::from_secs(30),
            JsonStage::Transform,
        );
        let report = format!("{} {}", transform.raw_stdout, transform.raw_stderr);
        assert!(!transform.success);
        assert!(report.contains("y not used in the model block"), "{report}");
    }
}

#[test]
fn simulate_print_and_noprint_are_one_option() {
    let binary = pinned_binary();
    let source = fixture("fire_simulate_print_noprint.mod");
    let diagnostics = analyze(&parse(&source));
    let ours = diagnostics
        .iter()
        .find(|diag| diag.code == "E271")
        .unwrap_or_else(|| panic!("missing E271: {diagnostics:?}"));
    assert_eq!(ours.message, "option noprint declared twice");
    assert_eq!(
        &source[ours.span.start as usize..ours.span.end as usize],
        "noprint"
    );
    if let Some(ref binary) = binary {
        let (accepted, report) = official_check(&source, binary);
        assert!(!accepted);
        assert!(report.contains("option noprint declared twice"), "{report}");
    }
}

#[test]
fn count_warning_does_not_name_a_heterogeneous_use() {
    let binary = pinned_binary();
    let source = fixture("fire_w013_het_use.mod");
    let diagnostics = analyze(&parse(&source));
    let ours = diagnostics
        .iter()
        .find(|diag| diag.code == "W013")
        .unwrap_or_else(|| panic!("missing W013: {diagnostics:?}"));
    assert!(
        ours.message
            .contains("1 equation(s) but 2 endogenous variable(s)"),
        "{}",
        ours.message
    );
    assert!(
        !ours.message.contains("remove z"),
        "z is used in the heterogeneous body: {}",
        ours.message
    );
    if let Some(ref binary) = binary {
        let transform = run_preprocessor(
            &source,
            binary,
            None,
            Duration::from_secs(30),
            JsonStage::Transform,
        );
        let report = format!("{} {}", transform.raw_stdout, transform.raw_stderr);
        assert!(!transform.success);
        assert!(
            report.contains("There are 1 equations but 2 endogenous variables!"),
            "{report}"
        );
    }
}
