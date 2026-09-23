use dygnosis::{check_parse, command_options, is_known_command, list_options, option_doc, parse};

fn to_value(payload: impl serde::Serialize) -> serde_json::Value {
    serde_json::to_value(payload).unwrap()
}

#[test]
fn list_options_omitted_matches_expected() {
    let expected: serde_json::Value =
        serde_json::from_str(include_str!("expected/list_options.omitted.json")).unwrap();
    assert_eq!(to_value(list_options(None)), expected);
    assert_eq!(to_value(list_options(Some(""))), expected);
}

#[test]
fn list_options_known_matches_expected() {
    let expected: serde_json::Value =
        serde_json::from_str(include_str!("expected/list_options.known.json")).unwrap();
    for (key, payload) in expected.as_object().unwrap() {
        assert_eq!(
            to_value(list_options(Some(key))),
            *payload,
            "known mismatch for {key}"
        );
    }
    assert_eq!(
        to_value(list_options(Some("Stoch_Simul"))),
        expected["stoch_simul"]
    );
}

#[test]
fn list_options_unknown_matches_expected() {
    let expected: serde_json::Value =
        serde_json::from_str(include_str!("expected/list_options.unknown.json")).unwrap();
    for (input, payload) in expected.as_object().unwrap() {
        assert_eq!(
            to_value(list_options(Some(input))),
            *payload,
            "unknown mismatch for {input:?}"
        );
    }
}

#[test]
fn command_options_accessors() {
    assert!(command_options("nope").is_empty());
    assert!(command_options("").is_empty());
    let known: serde_json::Value =
        serde_json::from_str(include_str!("expected/list_options.known.json")).unwrap();
    let names: Vec<&str> = command_options("STOCH_SIMUL")
        .iter()
        .map(|(name, _)| *name)
        .collect();
    let expected: Vec<&str> = known["stoch_simul"]["options"]
        .as_array()
        .unwrap()
        .iter()
        .map(|opt| opt["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, expected);
    assert!(is_known_command("STOCH_SIMUL"));
    assert!(!is_known_command("nope"));
}

#[test]
fn option_doc_matches_expected() {
    let docs: serde_json::Value =
        serde_json::from_str(include_str!("expected/option_docs.json")).unwrap();
    for (key, val) in docs.as_object().unwrap() {
        assert_eq!(
            option_doc(key),
            val.as_str().unwrap(),
            "option_doc mismatch for {key}"
        );
    }
    assert_eq!(option_doc("nsam"), "");
    assert_eq!(option_doc("Nsam"), docs["Nsam"].as_str().unwrap());

    let known: serde_json::Value =
        serde_json::from_str(include_str!("expected/list_options.known.json")).unwrap();
    let mut saw_absent = false;
    for payload in known.as_object().unwrap().values() {
        for opt in payload["options"].as_array().unwrap() {
            let name = opt["name"].as_str().unwrap();
            if !docs.as_object().unwrap().contains_key(name) {
                assert_eq!(option_doc(name), "");
                saw_absent = true;
            }
        }
    }
    assert!(
        saw_absent,
        "expected a COMMAND_OPTIONS name absent from OPTION_DOCS"
    );
}

const BIND_HELP: &str = "Mandatory condition evaluated in the baseline/steady-state regime to check whether the constraint becomes binding.";
const ERROR_BIND_HELP: &str = "Optional numerical criterion for the size of the bind-constraint violation. Default: absolute value of the bind inequality.";
const ERROR_RELAX_HELP: &str = "Optional numerical criterion for the size of the relax-constraint violation. Default: absolute value of the relax inequality.";
const NAME_CLAUSE_HELP: &str =
    "Quoted constraint name (name 'STRING';), used in bind/relax equation tags.";
const RELAX_HELP: &str = "Optional condition evaluated in the binding regime to check whether the constraint is relaxed. If omitted, Dynare checks whether the bind expression is false.";

fn option_names(command: &str) -> Vec<&str> {
    command_options(command)
        .iter()
        .map(|(name, _)| *name)
        .collect()
}

#[test]
fn occbin_constraints_is_catalogued_block() {
    assert!(is_known_command("occbin_constraints"));
    assert!(is_known_command("OCCBIN_CONSTRAINTS"));

    assert_eq!(
        option_names("occbin_constraints"),
        ["bind", "error_bind", "error_relax", "name", "relax"]
    );

    let payload = to_value(list_options(Some("occbin_constraints")));
    assert_eq!(payload["command"], "occbin_constraints");
    assert_eq!(payload["known"], true);
    assert_eq!(payload["n_options"], 5);
    let options = payload["options"].as_array().unwrap();
    assert_eq!(options.len(), 5);
    assert_eq!(options[0]["name"], "bind");
    assert_eq!(options[0]["description"], BIND_HELP);
    assert_eq!(options[1]["name"], "error_bind");
    assert_eq!(options[1]["description"], ERROR_BIND_HELP);
    assert_eq!(options[2]["name"], "error_relax");
    assert_eq!(options[2]["description"], ERROR_RELAX_HELP);
    assert_eq!(options[3]["name"], "name");
    assert_eq!(options[3]["description"], NAME_CLAUSE_HELP);
    assert_eq!(options[4]["name"], "relax");
    assert_eq!(options[4]["description"], RELAX_HELP);

    let upper = to_value(list_options(Some("OCCBIN_CONSTRAINTS")));
    assert_eq!(upper["n_options"], 5);
    assert_eq!(upper, payload);

    let omitted = to_value(list_options(None));
    assert_eq!(omitted["n_commands"], 85);
    let i = omitted["commands"]
        .as_array()
        .unwrap()
        .iter()
        .position(|c| c == "occbin_constraints")
        .expect("occbin_constraints missing from omitted commands");
    assert_eq!(omitted["commands"][i - 1], "mshocks");
    assert_eq!(omitted["commands"][i], "occbin_constraints");
    assert_eq!(omitted["commands"][i + 1], "occbin_graph");

    assert!(option_doc("name").contains("M-/MEX file"));
    assert_eq!(
        option_doc("name"),
        "The name of the function, which must also be the name of the M-/MEX file implementing it."
    );
    assert_eq!(option_doc("bind"), BIND_HELP);
    assert_eq!(option_doc("error_bind"), ERROR_BIND_HELP);
    assert_eq!(option_doc("error_relax"), ERROR_RELAX_HELP);
    assert_eq!(option_doc("relax"), RELAX_HELP);

    assert_eq!(option_names("occbin_graph"), ["noconstant"]);
    assert_eq!(
        option_names("occbin_setup"),
        [
            "filter_init_periods_using_particles",
            "filter_particle_diagnostics",
            "filter_particle_diagnostics_graph_periods",
            "filter_particle_diagnostics_nograph",
            "filter_particle_draw_states_from_empirical_density",
            "filter_particle_initial_state_ergodic_simul",
            "filter_particle_number_of_particles",
            "filter_particle_number_of_shocks_per_particle",
            "filter_particle_state_draws",
            "filter_particle_state_importance_sampling_logpost_crit_threshold",
            "filter_particle_state_importance_sampling_pkf_init",
            "filter_particle_state_importance_sampling_slice_burnin",
            "filter_particle_state_importance_sampling_slice_override_iteration",
            "filter_particle_use_pkf_updated_state_threshold",
            "filter_use_relaxation",
            "likelihood_brute_force_extra_regime_guess",
            "likelihood_brute_force_regime_guess",
            "likelihood_check_ahead_periods",
            "likelihood_curb_retrench",
            "likelihood_first_period_binding_regime_allowed",
            "likelihood_first_period_occbin_update",
            "likelihood_inversion_filter",
            "likelihood_max_check_ahead_periods",
            "likelihood_max_kalman_iterations",
            "likelihood_maxit",
            "likelihood_periodic_solution",
            "likelihood_periods",
            "likelihood_piecewise_kalman_filter",
            "particle_filtering",
            "posterior_importance_sampling",
            "posterior_importance_sampling_filter",
            "posterior_importance_sampling_orig_dname",
            "posterior_importance_sampling_orig_filter",
            "posterior_importance_sampling_orig_fname",
            "posterior_importance_sampling_sub_draws",
            "simul_check_ahead_periods",
            "simul_curb_retrench",
            "simul_debug",
            "simul_max_check_ahead_periods",
            "simul_maxit",
            "simul_periodic_solution",
            "simul_periodic_solution_strict",
            "simul_periodic_solution_threshold",
            "simul_periods",
            "simul_reset_check_ahead_periods",
            "simul_reset_regime_in_new_period",
            "smoother_check_ahead_periods",
            "smoother_curb_retrench",
            "smoother_debug",
            "smoother_first_period_occbin_update",
            "smoother_inversion_filter",
            "smoother_max_check_ahead_periods",
            "smoother_max_number_of_iterations",
            "smoother_maxit",
            "smoother_periodic_solution",
            "smoother_periods",
            "smoother_piecewise_kalman_filter",
            "smoother_plot",
        ]
    );
    assert_eq!(
        option_names("occbin_solver"),
        [
            "simul_check_ahead_periods",
            "simul_curb_retrench",
            "simul_debug",
            "simul_max_check_ahead_periods",
            "simul_maxit",
            "simul_periodic_solution",
            "simul_periodic_solution_strict",
            "simul_periodic_solution_threshold",
            "simul_periods",
            "simul_reset_check_ahead_periods",
            "simul_reset_regime_in_new_period",
        ]
    );
    assert_eq!(
        option_names("occbin_write_regimes"),
        ["filename", "periods", "simul", "smoother"]
    );

    assert!(command_options("lmmcp").is_empty());
    assert!(!is_known_command("lmmcp"));
    for command in [
        "extended_path",
        "perfect_foresight_solver",
        "perfect_foresight_with_expectation_errors_solver",
        "simul",
    ] {
        assert!(
            command_options(command)
                .iter()
                .any(|(name, _)| *name == "lmmcp"),
            "lmmcp missing on {command}"
        );
    }
    for command in [
        "occbin_constraints",
        "occbin_graph",
        "occbin_setup",
        "occbin_solver",
        "occbin_write_regimes",
    ] {
        assert!(
            command_options(command)
                .iter()
                .all(|(name, _)| *name != "lmmcp"),
            "lmmcp unexpectedly on {command}"
        );
    }
}

/// The five MS-SBVAR members whose option list is the statement's own options.
#[test]
fn ms_sbvar_family_options_are_catalogued() {
    assert_eq!(
        option_names("sbvar"),
        [
            "aband",
            "alpha",
            "apband",
            "beta",
            "cms",
            "cnum",
            "coefficients_prior_hyperparameters",
            "contemp_reduced_form",
            "cross_restrictions",
            "datafile",
            "dummy_obs",
            "eq_cms",
            "eq_ms",
            "final_subperiod",
            "final_year",
            "flat_prior",
            "foreband",
            "forecast",
            "freq",
            "gsig2_lmdm",
            "indxap",
            "indxestima",
            "indxfore",
            "indxgdls",
            "indxgforehat",
            "indxgimfhat",
            "indximf",
            "indxovr",
            "indxparr",
            "indxscalesstates",
            "initial_subperiod",
            "initial_year",
            "ncms",
            "ncsk",
            "ninv",
            "nlags",
            "no_bayesian_prior",
            "nstates",
            "nstd",
            "q_diag",
            "real_pseudo_forecast",
            "restriction_fname",
            "tlindx",
            "tlnumber",
            "vlist",
            "vlistlog",
            "vlistper",
        ]
    );
    assert_eq!(option_names("plot_conditional_forecast"), ["periods"]);
    assert!(option_names("svar_global_identification_check").is_empty());

    // The three members whose body is rows, not options.
    for command in [
        "svar_identification",
        "svar_global_identification_check",
        "conditional_forecast_paths",
    ] {
        assert!(is_known_command(command), "{command}");
        assert!(
            command_options(command).is_empty(),
            "{command} takes no options"
        );
    }

    // The dotted `prior` statement: its head is a symbol, so the entry is keyed
    // on the tail word the user writes after the dot.
    assert!(is_known_command("prior"));
    assert_eq!(
        option_names("prior"),
        [
            "domain", "interval", "mean", "median", "mode", "shape", "shift", "stdev", "truncate",
            "variance",
        ]
    );
    assert_eq!(
        option_doc("median"),
        "A shortcut to setting error_band_percentiles=[0.5]."
    );
}

/// A `sbvar` option 7.1 cannot reach is not catalogued: `DATA` is produced only
/// by a line that starts with `data`, so `data=` is a syntax error inside the
/// option list. The grammar accepts `datafile` there.
#[test]
fn sbvar_has_no_unreachable_data_option() {
    assert!(option_names("sbvar").contains(&"datafile"));
    assert!(!option_names("sbvar").contains(&"data"));
}

/// The two IRF rows carry the manual's sentence for their own block; the two
/// row-body blocks take no parenthesised production at the pin, so their lists
/// are empty.
const MATCHED_IRFS_OVERWRITE_HELP: &str =
    "The overwrite option replaces the current matched_irfs block with the new one.";
const MATCHED_IRFS_WEIGHTS_OVERWRITE_HELP: &str =
    "The overwrite option replaces the current matched_irfs_weights block with the new one.";

#[test]
fn the_four_family_blocks_are_catalogued() {
    assert_eq!(option_names("matched_irfs"), ["overwrite"]);
    assert_eq!(option_names("matched_irfs_weights"), ["overwrite"]);
    assert!(option_names("matched_moments").is_empty());
    assert!(option_names("moment_calibration").is_empty());

    for command in [
        "matched_irfs",
        "matched_irfs_weights",
        "matched_moments",
        "moment_calibration",
    ] {
        assert!(is_known_command(command), "{command}");
        assert!(is_known_command(&command.to_uppercase()), "{command}");
    }

    let payload = to_value(list_options(Some("matched_irfs")));
    assert_eq!(payload["command"], "matched_irfs");
    assert_eq!(payload["known"], true);
    assert_eq!(payload["n_options"], 1);
    assert_eq!(payload["options"][0]["name"], "overwrite");
    assert_eq!(
        payload["options"][0]["description"],
        MATCHED_IRFS_OVERWRITE_HELP
    );

    let payload = to_value(list_options(Some("matched_irfs_weights")));
    assert_eq!(payload["n_options"], 1);
    assert_eq!(payload["options"][0]["name"], "overwrite");
    assert_eq!(
        payload["options"][0]["description"],
        MATCHED_IRFS_WEIGHTS_OVERWRITE_HELP
    );

    for command in ["matched_moments", "moment_calibration"] {
        let payload = to_value(list_options(Some(command)));
        assert_eq!(payload["command"], command);
        assert_eq!(payload["known"], true);
        assert_eq!(payload["n_options"], 0, "{command}");
        assert_eq!(payload["options"].as_array().unwrap().len(), 0, "{command}");
    }
}

/// The common fallback stays true for every block; each block gives the exact
/// scope of its own overwrite operation.
#[test]
fn overwrite_docs_match_each_block() {
    const HELP: &str = "Replaces earlier settings according to the current block's overwrite rule.";
    assert_eq!(option_doc("overwrite"), HELP);
    let shocks = command_options("shocks")
        .iter()
        .find(|(name, _)| *name == "overwrite")
        .unwrap()
        .1;
    let mshocks = command_options("mshocks")
        .iter()
        .find(|(name, _)| *name == "overwrite")
        .unwrap()
        .1;
    let paths = command_options("shock_paths")
        .iter()
        .find(|(name, _)| *name == "overwrite")
        .unwrap()
        .1;
    assert!(shocks.contains("deterministic schedules"));
    assert!(shocks.contains("variance, standard-error, covariance, correlation"));
    assert!(shocks.contains("other skew rows remain"));
    assert!(mshocks.contains("earlier deterministic shocks"));
    assert!(!mshocks.contains("variance, standard-error"));
    assert!(paths.contains("perfect_foresight_controlled_paths entries"));
    assert!(paths.contains("same learnt_in value"));
    assert_eq!(
        command_options("matched_irfs")[0].1,
        MATCHED_IRFS_OVERWRITE_HELP
    );
    assert_eq!(
        command_options("matched_irfs_weights")[0].1,
        MATCHED_IRFS_WEIGHTS_OVERWRITE_HELP
    );
}

#[test]
fn pinned_72_shock_and_date_catalog_has_only_opener_options() {
    for (command, expected) in [
        ("shocks", vec!["learnt_in", "overwrite", "surprise"]),
        (
            "mshocks",
            vec!["learnt_in", "overwrite", "relative_to_initval"],
        ),
        ("heteroskedastic_shocks", vec!["overwrite"]),
        ("shock_paths", vec!["learnt_in", "overwrite"]),
        ("endval", vec!["learnt_in"]),
        ("perfect_foresight_controlled_paths", vec!["learnt_in"]),
        ("options", vec!["bounds", "init", "jscale"]),
    ] {
        assert!(is_known_command(command), "{command}");
        assert_eq!(option_names(command), expected, "{command}");
        let payload = to_value(list_options(Some(&command.to_uppercase())));
        assert_eq!(payload["known"], true, "{command}");
        assert_eq!(payload["n_options"], expected.len(), "{command}");
    }

    for command in ["database", "set_time", "subsamples"] {
        assert!(is_known_command(command), "{command}");
        assert!(command_options(command).is_empty(), "{command}");
        let payload = to_value(list_options(Some(command)));
        assert_eq!(payload["known"], true, "{command}");
        assert_eq!(payload["n_options"], 0, "{command}");
    }
    assert!(!is_known_command("date"));
    for (command, option) in [
        ("data", "first_obs"),
        ("data", "last_obs"),
        ("perfect_foresight_setup", "first_simulation_period"),
        ("perfect_foresight_setup", "last_simulation_period"),
        ("plot_shock_decomposition", "plot_init_date"),
        ("plot_shock_decomposition", "plot_end_date"),
    ] {
        assert!(
            option_names(command).contains(&option),
            "{command}.{option}"
        );
    }
    for option in [
        "bounds",
        "init",
        "jscale",
        "learnt_in",
        "relative_to_initval",
        "surprise",
    ] {
        assert!(!option_doc(option).is_empty(), "{option}");
    }
    assert!(!option_names("shocks").contains(&"heterogeneity"));
    assert!(!option_names("mshocks").contains(&"surprise"));
    assert!(!option_names("shock_paths").contains(&"relative_to_initval"));
    for body_word in ["periods", "values", "scales", "exogenize", "endogenize"] {
        assert!(!option_names("shock_paths").contains(&body_word));
        assert!(!option_names("mshocks").contains(&body_word));
    }
}

/// A declared head left without its `;` before a family command is **E001**, the
/// way the `stoch_simul` follower already is. 7.1 refuses every shape in this
/// table with `syntax error, unexpected IDENTIFIER`, so the two agree.
#[test]
fn a_declared_head_without_semi_before_a_family_command_is_e001() {
    const HEAD: &str = "var y c;\nvarexo e;\nparameters a;\na = 0.5;\n\nmodel;\ny = a*y(-1) + e;\nc = y;\nend;\n\na = 0.6\n";

    for follower in [
        "matched_irfs;",
        "matched_irfs_weights;",
        "matched_moments;",
        "moment_calibration;",
        "method_of_moments(mom_method=GMM, datafile=msdata);",
        "stoch_simul(order=1);",
    ] {
        let text = format!("{HEAD}{follower}\n");
        let got: Vec<String> = check_parse(&parse(&text))
            .into_iter()
            .map(|d| d.message.clone())
            .collect();
        assert!(
            got.iter().any(
                |m| m.contains("Parameter assignment 'a' is missing its terminating semicolon")
            ),
            "{follower}: expected the follower E001, got {got:?}"
        );
    }
}

/// The other direction: a completed assignment before the same commands keeps
/// them quiet. 7.1 accepts every one of these files.
#[test]
fn a_completed_assignment_before_a_family_command_leaves_e001_quiet() {
    const HEAD: &str = "var y c;\nvarexo e;\nparameters a;\na = 0.5;\n\nmodel;\ny = a*y(-1) + e;\nc = y;\nend;\n\na = 0.6;\n";

    for follower in [
        "matched_irfs;\nvar y; varexo e; periods 1; values 1; end;",
        "matched_irfs_weights;\ny(1), e, c(1), e, 20;\nend;",
        "matched_moments;\ny;\nend;",
        "moment_calibration;\ny, c(0), [0.1, 0.5];\nend;",
        "method_of_moments(mom_method=GMM, datafile=msdata);",
        "stoch_simul(order=1);",
    ] {
        let text = format!("{HEAD}{follower}\n");
        let got: Vec<String> = check_parse(&parse(&text))
            .into_iter()
            .map(|d| d.message.clone())
            .collect();
        assert!(
            !got.iter()
                .any(|m| m.contains("is missing its terminating semicolon")),
            "{follower}: expected no follower E001, got {got:?}"
        );
    }
}
