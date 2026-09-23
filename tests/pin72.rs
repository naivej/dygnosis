//! Dynare 7.2 language and accepted block-scope regressions.

use dygnosis::{analyze, command_options, parse, Severity};

const QUIET: &[(&str, &str)] = &[
    ("quiet_e111_two_shocks_blocks.mod", "E111"),
    ("quiet_e111_two_corr_blocks.mod", "E111"),
    ("quiet_e111_two_overwrite_shocks_blocks.mod", "E111"),
    ("quiet_e111_two_overwrite_corr_blocks.mod", "E111"),
    ("quiet_e243_two_histval_blocks.mod", "E243"),
    ("quiet_e244_two_estimated_params_blocks.mod", "E244"),
    ("quiet_e244_two_estimated_params_init_blocks.mod", "E244"),
    ("quiet_e244_two_estimated_params_bounds_blocks.mod", "E244"),
    ("quiet_e245_two_estimated_params_blocks.mod", "E245"),
    ("quiet_e246_two_estimated_params_blocks.mod", "E246"),
    ("quiet_e247_two_estimated_params_blocks.mod", "E247"),
    ("quiet_e248_cross_block_value.mod", "E248"),
    ("quiet_e261_two_observation_trends_blocks.mod", "E261"),
    ("quiet_e273_two_generate_irfs_blocks.mod", "E273"),
    ("quiet_e313_two_filter_initial_state_blocks.mod", "E313"),
    ("quiet_e315_two_optim_weights_blocks.mod", "E315"),
    ("quiet_e316_two_optim_weights_pairs.mod", "E316"),
];

fn fixture(rel: &str) -> String {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/pin72")
        .join(rel);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
        .replace("\r\n", "\n")
}

#[test]
fn accepted_repetitions_in_separate_blocks() {
    let mut failures = Vec::new();
    for (name, code) in QUIET {
        let diags = analyze(&parse(&fixture(name)));
        if diags.iter().any(|d| d.severity == Severity::Error) {
            failures.push(format!("{name} ({code}): {diags:?}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn same_block_duplicate_after_empty_block_still_fires() {
    let base = "var y z; varexo e e2; parameters rho; rho=.5; model; y=rho*y(-1)+e+e2; z=y; end;";
    for (suffix, code) in [
        (
            "shocks; end; shocks; var e; stderr .1; var e; stderr .2; end;",
            "E111",
        ),
        ("histval; end; histval; y(0)=1; y(0)=2; end;", "E243"),
        (
            "estimated_params; end; estimated_params; rho; rho; end;",
            "E244",
        ),
    ] {
        let diags = analyze(&parse(&format!("{base} {suffix}")));
        assert_eq!(
            diags.iter().filter(|d| d.code == code).count(),
            1,
            "{code} after an empty block: {diags:?}"
        );
    }
}

#[test]
fn histval_file_nobs_is_e001_at_option() {
    let src = fixture("e001_histval_file_nobs.mod");
    let diags = analyze(&parse(&src));
    let e001 = diags.iter().find(|d| d.code == "E001").expect("E001");
    assert_eq!(e001.message, "syntax error, unexpected NOBS");
    assert_eq!(
        &src[e001.span.start as usize..e001.span.end as usize],
        "nobs"
    );
}

#[test]
fn histval_file_last_simulation_period_is_e001_at_option() {
    let src = fixture("e001_histval_file_last_simulation_period.mod");
    let diags = analyze(&parse(&src));
    let e001 = diags.iter().find(|d| d.code == "E001").expect("E001");
    assert_eq!(
        e001.message,
        "syntax error, unexpected LAST_SIMULATION_PERIOD"
    );
    assert_eq!(
        &src[e001.span.start as usize..e001.span.end as usize],
        "last_simulation_period"
    );
}

#[test]
fn initval_file_retains_both_options() {
    let histval_names: Vec<_> = command_options("histval_file")
        .iter()
        .map(|(name, _)| *name)
        .collect();
    let initval_names: Vec<_> = command_options("initval_file")
        .iter()
        .map(|(name, _)| *name)
        .collect();
    for name in ["nobs", "last_simulation_period"] {
        assert!(
            !histval_names.contains(&name),
            "histval_file still offers {name}"
        );
        assert!(initval_names.contains(&name), "initval_file dropped {name}");
        let quiet = format!("quiet_initval_file_{name}.mod");
        let diags = analyze(&parse(&fixture(&quiet)));
        assert!(
            diags.iter().all(|d| d.severity != Severity::Error),
            "{quiet}: {diags:?}"
        );
    }
}
