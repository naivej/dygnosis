//! D-lists locks (0.6.0 03b).
//!
//! The nine commands whose trailing symbol list this slice starts walking, the
//! two labels 7.1 has beyond the shipped `{endogenous}` / `{parameter}`, and the
//! five trigger edges the reach audit found on the shipped surface.
//!
//! Every fire fixture carries one problem, and its sentence is their text
//! character for character (see `honesty.rs` for the run against the installed
//! preprocessor).

use dygnosis::explain::known_codes;
use dygnosis::{analyze, parse, Diagnostic};

fn fixture(rel: &str) -> String {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(rel);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
        .replace("\r\n", "\n")
}

fn diags(rel: &str) -> Vec<Diagnostic> {
    analyze(&parse(&fixture(rel)))
}

fn codes(got: &[Diagnostic]) -> Vec<&str> {
    got.iter().map(|d| d.code.as_str()).collect()
}

fn find<'a>(got: &'a [Diagnostic], code: &str) -> &'a Diagnostic {
    got.iter()
        .find(|d| d.code == code)
        .unwrap_or_else(|| panic!("expected {code}, got {:?}", codes(got)))
}

fn quiet(got: &[Diagnostic], code: &str) {
    assert!(
        got.iter().all(|d| d.code != code),
        "expected no {code}, got {:?}",
        codes(got)
    );
}

/// The header every fire fixture is built on: a square model with the four
/// declaration kinds the rows below name.
fn head() -> &'static str {
    "var y c;\nvarexo e;\nvarexo_det ed;\nparameters a;\na = 0.5;\n\nmodel;\ny = a*y(-1) + e;\nc = y;\nend;\n\n"
}

/// The fire files, each with the one sentence 7.1 prints for it.
#[test]
fn the_fire_files_carry_their_sentences() {
    const TABLE: &[(&str, &str, &str)] = &[
        (
            "lists/e239_forecast_z.mod",
            "E239",
            "forecast: Variable z was not declared.",
        ),
        (
            "lists/e239_rplot_z.mod",
            "E239",
            "rplot: Variable z was not declared.",
        ),
        (
            "lists/e239_dynasave_z.mod",
            "E239",
            "dynasave: Variable z was not declared.",
        ),
        (
            "lists/e239_dynatype_z.mod",
            "E239",
            "dynatype: Variable z was not declared.",
        ),
        (
            "lists/e239_shockdec_z.mod",
            "E239",
            "shock_decomposition: Variable z was not declared.",
        ),
        (
            "lists/e239_squeeze_z.mod",
            "E239",
            "squeeze_shock_decomposition: Variable z was not declared.",
        ),
        (
            "lists/e239_osr_aux_endo.mod",
            "E239",
            "osr: Variable AUX_ENDO_1 was not declared.",
        ),
        (
            "lists/e239_toplevel_forecast.mod",
            "E239",
            "forecast: Variable xx was not declared.",
        ),
        (
            "lists/e239_mloc_forecast.mod",
            "E239",
            "forecast: Variable mloc was not declared.",
        ),
        (
            "lists/e239_forecast_upper.mod",
            "E239",
            "forecast: Variable z was not declared.",
        ),
        (
            "lists/e240_forecast_a.mod",
            "E240",
            "forecast: Variable a is not one of {endogenous}",
        ),
        (
            "lists/e240_rplot_a.mod",
            "E240",
            "rplot: Variable a is not one of {endogenous, exogenous}",
        ),
        (
            "lists/e240_rplot_ed.mod",
            "E240",
            "rplot: Variable ed is not one of {endogenous, exogenous}",
        ),
        (
            "lists/e240_dynatype_a.mod",
            "E240",
            "dynatype: Variable a is not one of {endogenous, exogenous}",
        ),
        (
            "lists/e240_shockdec_e.mod",
            "E240",
            "shock_decomposition: Variable e is not one of {endogenous}",
        ),
        (
            "lists/e240_realtime_a.mod",
            "E240",
            "realtime_shock_decomposition: Variable a is not one of {endogenous}",
        ),
        (
            "lists/e240_icdec_a.mod",
            "E240",
            "initial_condition_decomposition: Variable a is not one of {endogenous}",
        ),
        (
            "lists/e240_plotsd_a.mod",
            "E240",
            "plot_shock_decomposition: Variable a is not one of {endogenous, epilogue}",
        ),
        (
            "lists/e240_plotsd_exo.mod",
            "E240",
            "plot_shock_decomposition: Variable e is not one of {endogenous, epilogue}",
        ),
        (
            "lists/e240_osr_trailing.mod",
            "E240",
            "osr: Variable a is not one of {endogenous}",
        ),
        (
            "lists/e240_epilogue_forecast.mod",
            "E240",
            "forecast: Variable gg is not one of {endogenous}",
        ),
        (
            "lists/e240_gg_stoch.mod",
            "E240",
            "stoch_simul: Variable gg is not one of {endogenous}",
        ),
        (
            "lists/e240_trend_rplot.mod",
            "E240",
            "rplot: Variable tt is not one of {endogenous, exogenous}",
        ),
        (
            "lists/e240_removed_stoch.mod",
            "E240",
            "stoch_simul: Variable c is not one of {endogenous}",
        ),
        (
            "lists/e240_modellocal_lists.mod",
            "E240",
            "forecast: Variable foo is not one of {endogenous}",
        ),
    ];
    for (rel, code, message) in TABLE {
        let got = diags(rel);
        let hit = find(&got, code);
        assert_eq!(hit.message, *message, "{rel}");
        assert_eq!(
            got.iter().filter(|d| d.code == *code).count(),
            1,
            "{rel}: one fire per file, got {:?}",
            codes(&got)
        );
    }
}

/// The external-function fixture carries our **W160** beside the **E240** 7.1
/// does not emit. **W160** is a workspace row (the companion is looked up next
/// to the file), so this lock is the **E240** and the fixture tolerates it.
#[test]
fn an_external_function_name_reports_its_type() {
    let got = diags("lists/e240_extfun_dynatype.mod");
    assert_eq!(
        find(&got, "E240").message,
        "dynatype: Variable foo is not one of {endogenous, exogenous}"
    );
    quiet(&got, "E239");
}

/// Every fire file stands at the span of the name it refuses.
#[test]
fn a_fire_lands_on_the_listed_name() {
    for (rel, name) in [
        ("lists/e239_forecast_z.mod", "z"),
        ("lists/e240_rplot_ed.mod", "ed"),
        ("lists/e239_osr_aux_endo.mod", "AUX_ENDO_1"),
    ] {
        let source = fixture(rel);
        let got = diags(rel);
        let hit = got
            .iter()
            .find(|d| d.code == "E239" || d.code == "E240")
            .unwrap_or_else(|| panic!("{rel}: no fire"));
        let at = &source[hit.span.start as usize..hit.span.end as usize];
        assert_eq!(at, name, "{rel}");
    }
}

/// The nine commands are walked, with the command string their sentence carries.
#[test]
fn the_nine_command_lists_are_collected() {
    const TABLE: &[(&str, &str)] = &[
        ("forecast y;", "forecast"),
        ("rplot y;", "rplot"),
        ("dynasave('f.csv') y;", "dynasave"),
        ("dynatype('f.m') y;", "dynatype"),
        ("shock_decomposition y;", "shock_decomposition"),
        (
            "realtime_shock_decomposition y;",
            "realtime_shock_decomposition",
        ),
        (
            "initial_condition_decomposition y;",
            "initial_condition_decomposition",
        ),
        ("plot_shock_decomposition y;", "plot_shock_decomposition"),
        (
            "squeeze_shock_decomposition y;",
            "squeeze_shock_decomposition",
        ),
    ];
    for (stmt, command) in TABLE {
        let model = parse(&format!("{}{stmt}", head()));
        assert_eq!(model.command_symbols.len(), 1, "{stmt}");
        assert_eq!(model.command_symbols[0].command, *command, "{stmt}");
        assert_eq!(model.name(model.command_symbols[0].name), "y", "{stmt}");
    }
}

/// A list collects its names in order, and the option list contributes none.
#[test]
fn a_list_collects_names_in_order() {
    let model = parse(&format!("{}dynasave('f.csv') y, z;\n", head()));
    let names: Vec<&str> = model
        .command_symbols
        .iter()
        .map(|s| model.name(s.name))
        .collect();
    assert_eq!(names, vec!["y", "z"]);
    assert_eq!(model.command_symbols[0].command, "dynasave");
}

/// An empty trailing list collects nothing, which is how their accept is
/// matched: `squeeze_shock_decomposition;` is one of the command's two forms.
#[test]
fn an_empty_list_collects_nothing() {
    for stmt in [
        "squeeze_shock_decomposition;",
        "forecast;",
        "shock_decomposition;",
        "dynasave('f.csv');",
    ] {
        let model = parse(&format!("{}{stmt}", head()));
        assert!(model.command_symbols.is_empty(), "{stmt}");
        let got = analyze(&model);
        quiet(&got, "E239");
        quiet(&got, "E240");
    }
}

/// The legal shapes stay quiet: a plain `varexo` on `rplot`, the list written
/// above the declarations, an epilogue name on the one command that allows it.
#[test]
fn the_legal_shapes_stay_quiet() {
    for rel in ["lists/quiet_lists.mod", "lists/quiet_rplot_exo.mod"] {
        let got = diags(rel);
        quiet(&got, "E239");
        quiet(&got, "E240");
    }

    // A list written before the declaration still resolves: their check runs
    // over the finished symbol table.
    let above = parse(&format!("forecast y;\n{}", head()));
    quiet(&analyze(&above), "E239");

    // `plot_shock_decomposition` allows an epilogue name with the block either
    // side of the statement, and refuses every other kind.
    let after = format!(
        "{}epilogue;\ngg = y + c;\nend;\n\nplot_shock_decomposition gg;\n",
        head()
    );
    quiet(&analyze(&parse(&after)), "E240");
    let before = format!(
        "epilogue;\ngg = y + c;\nend;\n\n{}plot_shock_decomposition gg;\n",
        head()
    );
    quiet(&analyze(&parse(&before)), "E240");
}

/// `rplot ed, e;` fires its **E240** on the `varexo_det` and nothing on the
/// plain `varexo` beside it.
#[test]
fn the_plain_varexo_filter_is_per_name() {
    let got = analyze(&parse(&format!("{}rplot ed, e;\n", head())));
    let hits: Vec<&Diagnostic> = got.iter().filter(|d| d.code == "E240").collect();
    assert_eq!(hits.len(), 1, "{:?}", codes(&got));
    assert_eq!(
        hits[0].message,
        "rplot: Variable ed is not one of {endogenous, exogenous}"
    );
}

/// The aux hit stops that statement's list, and only that one: their `return`
/// leaves the later statements' lists unread, while an earlier list keeps its
/// own.
#[test]
fn the_aux_hit_stops_one_list() {
    for rel in [
        "lists/quiet_aux_prefix.mod",
        "lists/quiet_osr_aux_expect.mod",
    ] {
        let got = diags(rel);
        for code in ["E239", "E240", "W202"] {
            quiet(&got, code);
        }
    }
    let stops = format!("{}squeeze_shock_decomposition AUX_ENDO_1, z;\n", head());
    let got = analyze(&parse(&stops));
    quiet(&got, "E239");
    quiet(&got, "E240");

    // The refusal before the hit still fires, and the hit after a refusal is
    // never reached.
    let after = analyze(&parse(&format!("{}forecast z, AUX_ENDO_1;\n", head())));
    assert_eq!(
        find(&after, "E239").message,
        "forecast: Variable z was not declared."
    );
}

/// **W202** stays `stoch_simul`'s: the other commands accept a repeated name.
#[test]
fn only_stoch_simul_warns_on_a_repeated_name() {
    for stmt in [
        "forecast y, y;",
        "rplot y, y;",
        "shock_decomposition y, y;",
        "stoch_simul y, y;",
    ] {
        let got = analyze(&parse(&format!("{}{stmt}", head())));
        if stmt.starts_with("stoch_simul") {
            find(&got, "W202");
        } else {
            quiet(&got, "W202");
        }
        quiet(&got, "E239");
        quiet(&got, "E240");
    }
}

/// The `osr_params` statement's own list is told apart from the `osr`
/// statement's, and both print `osr: `.
#[test]
fn the_osr_statements_are_told_apart() {
    let model = parse(&format!("{}osr_params a;\nosr a;\n", head()));
    assert_eq!(model.command_symbols.len(), 2);
    assert_eq!(model.command_symbols[0].command, "osr_params");
    assert_eq!(model.command_symbols[1].command, "osr");

    let got = analyze(&model);
    let hits: Vec<&Diagnostic> = got.iter().filter(|d| d.code == "E240").collect();
    assert_eq!(hits.len(), 1, "{:?}", codes(&got));
    assert_eq!(
        hits[0].message,
        "osr: Variable a is not one of {endogenous}"
    );
    // The shipped E103 keeps firing on that file.
    find(&got, "E103");

    // `osr_params a;` alone is a legal list.
    let alone = analyze(&parse(&format!("{}osr_params a;\n", head())));
    quiet(&alone, "E239");
    quiet(&alone, "E240");
}

/// Every aux arm is checked against the set their regex is built from: the two
/// `AUX_ENDO_` / `LOG_` arms come with an endogenous set, so `osr_params` has
/// neither.
#[test]
fn the_aux_regex_follows_the_allowed_set() {
    for name in ["AUX_ENDO_1", "LOG_1"] {
        let got = analyze(&parse(&format!("{}osr_params {name};\n", head())));
        assert_eq!(
            find(&got, "E239").message,
            format!("osr: Variable {name} was not declared.")
        );
    }
    for name in ["AUX_EXPECT_1", "MULT_1"] {
        let got = analyze(&parse(&format!("{}osr_params {name};\n", head())));
        quiet(&got, "E239");
        quiet(&got, "E240");
        // The same two arms are pass-overs when the set holds endogenous.
        for stmt in [
            format!("rplot {name};"),
            format!("dynatype('f.m') {name};"),
            format!("plot_shock_decomposition {name};"),
            format!("stoch_simul {name};"),
        ] {
            let got = analyze(&parse(&format!("{}{stmt}", head())));
            quiet(&got, "E239");
            quiet(&got, "E240");
        }
    }
}

/// The prefix is their lowercase word whatever the statement's spelling.
#[test]
fn the_prefix_is_the_lowercase_command() {
    for (stmt, message) in [
        (
            "STOCH_SIMUL z;",
            "stoch_simul: Variable z was not declared.",
        ),
        ("FORECAST z;", "forecast: Variable z was not declared."),
        ("Rplot z;", "rplot: Variable z was not declared."),
        (
            "DynaSave('f.csv') z;",
            "dynasave: Variable z was not declared.",
        ),
        ("OSR_PARAMS z;", "osr: Variable z was not declared."),
    ] {
        let got = analyze(&parse(&format!("{}{stmt}", head())));
        assert_eq!(find(&got, "E239").message, *message, "{stmt}");
    }
}

/// The declared set names the five kinds their table holds that a list can
/// name, and the one it does not.
#[test]
fn the_declared_set_covers_the_kinds_they_know() {
    // An epilogue name, a trend name and an external-function name.
    let rows = [
        (
            format!(
                "{}epilogue;\ngg = y + c;\nend;\n\nstoch_simul gg;\n",
                head()
            ),
            "stoch_simul: Variable gg is not one of {endogenous}",
        ),
        (
            format!(
                "trend_var(growth_factor=0.5) tt;\n{}stoch_simul tt;\n",
                head()
            ),
            "stoch_simul: Variable tt is not one of {endogenous}",
        ),
        (
            format!("external_function(name=foo);\n{}stoch_simul foo;\n", head()),
            "stoch_simul: Variable foo is not one of {endogenous}",
        ),
    ];
    for (source, message) in rows {
        let got = analyze(&parse(&source));
        assert_eq!(find(&got, "E240").message, message);
        quiet(&got, "E239");
    }

    // A `#` model-local stays out of the set: at top level 7.1 does not know the
    // name either, so both sides say `was not declared.`
    let mloc = analyze(&parse(&format!(
        "# mloc = 3;\n{}stoch_simul mloc;\n",
        head()
    )));
    assert_eq!(
        find(&mloc, "E239").message,
        "stoch_simul: Variable mloc was not declared."
    );
    quiet(&mloc, "E240");

    // A top-level assignment is no declaration either.
    let xx = analyze(&parse(&format!("xx = 3;\n{}forecast xx;\n", head())));
    assert_eq!(
        find(&xx, "E239").message,
        "forecast: Variable xx was not declared."
    );
    quiet(&xx, "E240");
}

/// A name a `model_remove` dropped is in their table as an `excludedVariable`,
/// so the list reports its type.
#[test]
fn a_removed_name_reports_its_type() {
    let got = diags("lists/e240_removed_stoch.mod");
    assert_eq!(
        find(&got, "E240").message,
        "stoch_simul: Variable c is not one of {endogenous}"
    );
    quiet(&got, "E239");
}

/// A name the removal re-typed to exogenous, because the model still uses it:
/// `rplot` accepts it, and an endogenous-only command refuses it.
#[test]
fn a_retyped_removed_name_follows_the_new_type() {
    let source = "var y c;\nvarexo e;\nparameters a;\na = 0.5;\n\nmodel;\n[name='e1'] y = a*y(-1) + e + c;\n[name='e2'] c = y;\nend;\n\nmodel_remove([name='e2']);\n\n";
    let rplot = analyze(&parse(&format!("{source}rplot c;")));
    quiet(&rplot, "E239");
    quiet(&rplot, "E240");

    let stoch = analyze(&parse(&format!("{source}stoch_simul c;")));
    assert_eq!(
        find(&stoch, "E240").message,
        "stoch_simul: Variable c is not one of {endogenous}"
    );
}

/// A `#` definition **inside** the model block is a type of their own, so a list
/// reports its type rather than the undeclared sentence — on every command, and
/// on the shipped surface too.
#[test]
fn a_model_local_in_the_model_block_reports_its_type() {
    let head = "var y c;\nvarexo e;\nvarexo_det ed;\nparameters a;\na = 0.5;\n\nmodel;\n#foo = a*y;\ny = a*y(-1) + e;\nc = y + foo;\nend;\n\n";
    const TABLE: &[(&str, &str)] = &[
        (
            "forecast foo;",
            "forecast: Variable foo is not one of {endogenous}",
        ),
        (
            "rplot foo;",
            "rplot: Variable foo is not one of {endogenous, exogenous}",
        ),
        (
            "osr_params foo;",
            "osr: Variable foo is not one of {parameter}",
        ),
        (
            "plot_shock_decomposition foo;",
            "plot_shock_decomposition: Variable foo is not one of {endogenous, epilogue}",
        ),
        (
            "stoch_simul foo;",
            "stoch_simul: Variable foo is not one of {endogenous}",
        ),
    ];
    for (stmt, message) in TABLE {
        let got = analyze(&parse(&format!("{head}{stmt}")));
        assert_eq!(find(&got, "E240").message, *message, "{stmt}");
        quiet(&got, "E239");
    }
}

/// The audit's `osr` finding: the trailing list on `osr` itself, with the name
/// written on a preceding `osr_params`.
#[test]
fn the_osr_trailing_list_fires_its_first_e240() {
    let got = diags("lists/e240_osr_trailing.mod");
    assert_eq!(
        find(&got, "E240").message,
        "osr: Variable a is not one of {endogenous}"
    );
}

/// The two quiet findings from the audit: the `osr_params` pass-over and the
/// `stoch_simul` list whose second name their hit never reaches.
#[test]
fn the_audit_quiet_shapes_stay_quiet() {
    let prefix = analyze(&parse(&format!("{}stoch_simul AUX_ENDO_1, a;\n", head())));
    quiet(&prefix, "E239");
    quiet(&prefix, "E240");
}

/// The nine commands with a declared name of the right type stay quiet, and so
/// does every shipped one.
#[test]
fn a_declared_name_of_the_right_type_is_quiet() {
    for stmt in [
        "forecast y c;",
        "rplot y c e;",
        "dynasave('f.csv') y c e;",
        "dynatype('f.m') y c e;",
        "shock_decomposition y c;",
        "realtime_shock_decomposition y c;",
        "initial_condition_decomposition y c;",
        "plot_shock_decomposition y c;",
        "squeeze_shock_decomposition y c;",
        "stoch_simul y c;",
        "estimation(datafile='d.csv') y c;",
        "calib_smoother y c;",
        "osr_params a;",
    ] {
        let got = analyze(&parse(&format!("{}{stmt}", head())));
        quiet(&got, "E239");
        quiet(&got, "E240");
    }
}

/// No new code ships, so the registry does not move.
#[test]
fn registry_known_codes_include_shock_diagnostics() {
    assert_eq!(known_codes().len(), 359);
}
