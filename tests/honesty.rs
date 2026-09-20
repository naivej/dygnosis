use std::path::{Path, PathBuf};
use std::time::Duration;

use dygnosis::preprocessor::find_preprocessor;
use dygnosis::{analyze, check_file, parse, run_preprocessor, Diagnostic, JsonStage, Severity};

const ACCEPT_ARCHIVES: &[&str] = &[
    "trend_rbc_gov_inv",
    "sims_wu_2019",
    "lk2024",
    "govt_rbc_irf_matching",
];

const NAMED_HOLES: &[&str] = &[];

const SAME_GROUND_WARNINGS: &[&str] = &[
    "W022", "W031", "W042", "W121", "W131", "W150", "W170", "W200",
];

enum HonestyKind {
    Error { workspace_only: bool },
    Warning,
}

struct HonestyRow {
    code: &'static str,
    fixture: &'static str,
    kind: HonestyKind,
    their_needle: &'static str,
    our_needle: &'static str,
    stage: JsonStage,
}

const HONESTY_FIRE: &[HonestyRow] = &[
    HonestyRow {
        code: "E001",
        fixture: "e001/delete_model_end.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "syntax error",
        our_needle: "end",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E020",
        fixture: "e020/e020_typo.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "Unknown symbol",
        our_needle: "Undeclared identifier",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E021",
        fixture: "w010/w021_exo.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "not used in model block",
        our_needle: "not used in model block",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E023",
        fixture: "e020/e023_predetermined.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "Unknown symbol",
        our_needle: "Predetermined variable",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E024",
        fixture: "e020/e024_timed_det.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "cannot be given a lead or a lag",
        our_needle: "cannot be given a lead or a lag",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E025",
        fixture: "e020/e025_use_before.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "has wrong type or was already used",
        our_needle: "has wrong type or was already used",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E030",
        fixture: "e030/var_varexo_timed.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "declared twice with different types",
        our_needle: "declared twice with different types",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E058",
        fixture: "shape/w050_initval.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "Unknown symbol",
        our_needle: "is not declared",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E059",
        fixture: "shape/w053_initval.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "neither endogenous or exogenous",
        our_needle: "neither endogenous or exogenous",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E061",
        fixture: "e060/missing",
        kind: HonestyKind::Error {
            workspace_only: true,
        },
        their_needle: "Could not open",
        our_needle: "Could not open",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E062",
        fixture: "e060/e062_if.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "syntax error",
        our_needle: "Unterminated",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E063",
        fixture: "e060/e063_undef.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "Unknown variable",
        our_needle: "Unknown variable",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E064",
        fixture: "e060/e064_quoted.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "Macro-processing error",
        our_needle: "Macro-processing error",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E065",
        fixture: "e060/e065_varexo.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "STEADY_STATE",
        our_needle: "STEADY_STATE",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E090",
        fixture: "w090/w090_exo.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "is not endogenous",
        our_needle: "is not a declared endogenous",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E093",
        fixture: "w090/w093_param.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "Unknown symbol",
        our_needle: "estimated_params",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E095",
        fixture: "w090/w095_ot.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "is not an observed variable",
        our_needle: "is not an observed variable",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E100",
        fixture: "w100/w100_ramsey.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "planner_objective statement must be used",
        our_needle: "planner_objective statement must be used",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E101",
        fixture: "w100/w101_inst.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "Unknown symbol",
        our_needle: "Policy instrument",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E103",
        fixture: "w100/w103_both.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "The osr statement requires",
        our_needle: "The osr statement requires",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E111",
        fixture: "w110/w111_dup_var.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "variance or stderr of shock",
        our_needle: "variance or stderr of shock",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E130",
        fixture: "w130/w130_order.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "is undefined in the declaration",
        our_needle: "is undefined in the declaration",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E170",
        fixture: "occbin/e170_two_blocks.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "Multiple 'occbin_constraints' blocks are not allowed",
        our_needle: "Multiple 'occbin_constraints' blocks are not allowed",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E171",
        fixture: "occbin/e171_three.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "only up to two constraints are supported",
        our_needle: "only up to two constraints are supported",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E172",
        fixture: "occbin/e172_missing_regime.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "is not defined",
        our_needle: "is not defined",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E173",
        fixture: "occbin/e173_bind_no_name.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "must have a 'name' tag",
        our_needle: "must have a 'name' tag",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E174",
        fixture: "occbin/e174_bind_missing.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "The 'bind' expression is missing",
        our_needle: "The 'bind' expression is missing",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E175",
        fixture: "occbin/e175_no_equation.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "No equation has been declared for constraint",
        our_needle: "No equation has been declared for constraint",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E176",
        fixture: "occbin/e176_bind_and_relax.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "is both in the 'bind' and 'relax' tags",
        our_needle: "is both in the 'bind' and 'relax' tags",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E177",
        fixture: "occbin/e177_regime_dup.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "has already been declared for this equation",
        our_needle: "has already been declared for this equation",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E180",
        fixture: "occbin/e180_mcp_perp.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "Can't have both an 'mcp' tag",
        our_needle: "Can't have both an 'mcp' tag",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E181",
        fixture: "occbin/e181_bind_eq.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "must be an inequality constraint",
        our_needle: "must be an inequality constraint",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E182",
        fixture: "occbin/e182_lead.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "Leads and lags on variables are forbidden",
        our_needle: "Leads and lags on variables are forbidden",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E183",
        fixture: "occbin/e183_perp_form.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "Complementarity condition has an incorrect form",
        our_needle: "Complementarity condition has an incorrect form",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E184",
        fixture: "occbin/e184_dup_clause.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "clause is declared multiple times",
        our_needle: "clause is declared multiple times",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E185",
        fixture: "occbin/e185_bad_name.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "unauthorized characters",
        our_needle: "unauthorized characters",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "W022",
        fixture: "w010/w022_unused.mod",
        kind: HonestyKind::Warning,
        their_needle: "not used in the model",
        our_needle: "not used in the model",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "W031",
        fixture: "e030/same_kind_var.mod",
        kind: HonestyKind::Warning,
        their_needle: "Symbol y declared twice",
        our_needle: "Symbol y declared twice",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "W042",
        fixture: "shape/w042_missing.mod",
        kind: HonestyKind::Warning,
        their_needle: "is not assigned a value",
        our_needle: "is not assigned a value",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "W121",
        fixture: "w120/w121_lead.mod",
        kind: HonestyKind::Warning,
        their_needle: "used with a lead or a lag",
        our_needle: "used with a lead or a lag",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "W131",
        fixture: "w130/w131_zero.mod",
        kind: HonestyKind::Warning,
        their_needle: "is declared twice",
        our_needle: "is declared twice",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "W150",
        fixture: "w130/w150_simul.mod",
        kind: HonestyKind::Warning,
        their_needle: "deprecated",
        our_needle: "deprecated",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "W170",
        fixture: "occbin/w170_mcp.mod",
        kind: HonestyKind::Warning,
        their_needle: "obsolete",
        our_needle: "obsolete",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E178",
        fixture: "occbin/e178_surprise.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "the 'shocks(surprise)' block can only be used in conjunction with the 'occbin_constraints' block.",
        our_needle: "the 'shocks(surprise)' block can only be used in conjunction with the 'occbin_constraints' block.",
        stage: JsonStage::Transform,
    },
    HonestyRow {
        code: "E179",
        fixture: "clash/e179_identification.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "the 'occbin_constraints' block is not compatible with commands other than 'estimation', 'stoch_simul', and 'calib_smoother'.",
        our_needle: "the 'occbin_constraints' block is not compatible with commands other than 'estimation', 'stoch_simul', and 'calib_smoother'.",
        stage: JsonStage::Transform,
    },
    HonestyRow {
        code: "E104",
        fixture: "clash/e104_two_planner.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "there can only be one planner_objective statement",
        our_needle: "there can only be one planner_objective statement",
        stage: JsonStage::Transform,
    },
    HonestyRow {
        code: "E026",
        fixture: "clash/e026_varexo_det_simul.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "A .mod file cannot contain both one of {perfect_foresight_solver, simul, perfect_foresight_with_expectation_errors_solver} and varexo_det declaration (all exogenous variables are deterministic in this case)",
        our_needle: "A .mod file cannot contain both one of {perfect_foresight_solver, simul, perfect_foresight_with_expectation_errors_solver} and varexo_det declaration (all exogenous variables are deterministic in this case)",
        stage: JsonStage::Transform,
    },
    HonestyRow {
        code: "E027",
        fixture: "clash/e027_ramsey_varexo_det.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "ramsey_model and ramsey_policy are incompatible with deterministic exogenous variables",
        our_needle: "ramsey_model and ramsey_policy are incompatible with deterministic exogenous variables",
        stage: JsonStage::Transform,
    },
    HonestyRow {
        code: "E028",
        fixture: "clash/e028_identification_varexo_det.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "identification is incompatible with deterministic exogenous variables",
        our_needle: "identification is incompatible with deterministic exogenous variables",
        stage: JsonStage::Transform,
    },
    HonestyRow {
        code: "E113",
        fixture: "clash/e113_shock_paths_shocks.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "the 'shock_paths' block cannot be used in conjunction with either 'shocks', 'mshocks', 'endval' or 'perfect_foresight_controlled_paths' blocks.",
        our_needle: "the 'shock_paths' block cannot be used in conjunction with either 'shocks', 'mshocks', 'endval' or 'perfect_foresight_controlled_paths' blocks.",
        stage: JsonStage::Transform,
    },
    HonestyRow {
        code: "E200",
        fixture: "d_check/e200_write_latex.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "write_latex_steady_state_model statement without a steady_state_model",
        our_needle: "write_latex_steady_state_model statement without a steady_state_model",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E201",
        fixture: "d_check/e201_zero_eq.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "At least one model equation must be declared",
        our_needle: "At least one model equation must be declared",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E202",
        fixture: "w100/e202_disc_ramsey.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "You cannot use the discretionary_policy command when you use either ramsey_model",
        our_needle: "You cannot use the discretionary_policy command when you use either ramsey_model",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E203",
        fixture: "w100/e203_ramsey_constraints.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "ramsey_constraints block requires the presence of a ramsey_model",
        our_needle: "ramsey_constraints block requires the presence of a ramsey_model",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E204",
        fixture: "w100/e204_osr_both.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "cannot have both optim_weights and a planner_objective",
        our_needle: "cannot have both optim_weights and a planner_objective",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E205",
        fixture: "d_check/e205_pf_stoch.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "cannot mix perfect foresight context with stochastic context",
        our_needle: "cannot mix perfect foresight context with stochastic context",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E206",
        fixture: "d_check/e206_use_dll_bytecode.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "'use_dll' option is not compatible with 'bytecode'",
        our_needle: "'use_dll' option is not compatible with 'bytecode'",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E207",
        fixture: "d_check/e207_no_static.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "no_static option is incompatible",
        our_needle: "no_static option is incompatible",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E208",
        fixture: "d_check/e208_static_dynamic.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "equations marked [static] must be equal to the number of equations marked [dynamic]",
        our_needle: "equations marked [static] must be equal to the number of equations marked [dynamic]",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E209",
        fixture: "d_check/e209_tags_ramsey.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "marking equations as [static] or [dynamic] is not possible",
        our_needle: "marking equations as [static] or [dynamic] is not possible",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "W200",
        fixture: "d_check/w200_stoch_abs.mod",
        kind: HonestyKind::Warning,
        their_needle: "unsuitable for a stochastic context",
        our_needle: "unsuitable for a stochastic context",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E210",
        fixture: "d_check/e210_linear_abs.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "on an endogenous variable",
        our_needle: "on an endogenous variable",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E211",
        fixture: "d_check/e211_linear_exo.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "on an exogenous variable in a non-perfect-foresight context",
        our_needle: "on an exogenous variable in a non-perfect-foresight context",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E212",
        fixture: "d_check/e212_estimated_shock.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "also appear in the expressions defining the variance/covariance matrix of shocks",
        our_needle: "also appear in the expressions defining the variance/covariance matrix of shocks",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E213",
        fixture: "d_check/e213_pf_order.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "perfect_foresight_setup' command must come before 'perfect_foresight_solver",
        our_needle: "perfect_foresight_setup' command must come before 'perfect_foresight_solver",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E214",
        fixture: "d_check/e214_pfee_order.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "perfect_foresight_with_expectation_errors_setup' command must come before",
        our_needle: "perfect_foresight_with_expectation_errors_setup' command must come before",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E215",
        fixture: "w100/w100_disc_ok.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "discretionary_policy: the instruments option is required",
        our_needle: "discretionary_policy: the instruments option is required",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E216",
        fixture: "d_check/e216_extended_path.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "the 'periods' option of 'extended_path' is mandatory",
        our_needle: "the 'periods' option of 'extended_path' is mandatory",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E217",
        fixture: "d_check/e217_initval_after_endval.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "an 'initval' block cannot appear after an 'endval' block",
        our_needle: "an 'initval' block cannot appear after an 'endval' block",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E218",
        fixture: "d_check/e218_all_values.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "You have not set the following exogenous variables in initval",
        our_needle: "You have not set the following exogenous variables in initval",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E220",
        fixture: "d_walk/e220_bayesian_irf_counts.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "bayesian_irf option is passed to the estimation",
        our_needle: "bayesian_irf option is passed to the estimation",
        stage: JsonStage::Transform,
    },
    HonestyRow {
        code: "E221",
        fixture: "d_walk/e221_shocks_lt_varobs.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "greater than or equal to the number of observed variables",
        our_needle: "greater than or equal to the number of observed variables",
        stage: JsonStage::Transform,
    },
    HonestyRow {
        code: "E222",
        fixture: "d_walk/e222_dsge_var_missing_weight.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "dsge_prior_weight must be referenced in the estimated_params block",
        our_needle: "dsge_prior_weight must be referenced in the estimated_params block",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E223",
        fixture: "d_walk/e223_weight_and_calibrated.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "the prior weight cannot be calibrated",
        our_needle: "the prior weight cannot be calibrated",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E224",
        fixture: "d_walk/e224_weight_without_dsge_var.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "the dsge_var option must be passed to the estimation statement",
        our_needle: "the dsge_var option must be passed to the estimation statement",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E225",
        fixture: "d_walk/e225_dsge_varlag_without_dsge_var.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "requires a dsge_var option to be passed if the dsge_varlag option is passed",
        our_needle: "requires a dsge_var option to be passed if the dsge_varlag option is passed",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E226",
        fixture: "d_walk/e226_two_estimation_dsge_var.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "cannot take more than one dsge_var option",
        our_needle: "cannot take more than one dsge_var option",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E227",
        fixture: "d_walk/e227_estimation_no_data.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "requires a data file to be supplied via the datafile option",
        our_needle: "requires a data file to be supplied via the datafile option",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E228",
        fixture: "d_walk/e228_mode_file_use_calibration.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "incompatible with the use_calibration option",
        our_needle: "incompatible with the use_calibration option",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E229",
        fixture: "d_walk/e229_mh_tune_jscale_mh_jscale.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "mh_tune_jscale and mh_jscale options",
        our_needle: "mh_tune_jscale and mh_jscale options",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E230",
        fixture: "d_walk/e230_mh_tune_guess_alone.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "mh_tune_guess in estimation statement cannot be used without",
        our_needle: "mh_tune_guess in estimation statement cannot be used without",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E231",
        fixture: "d_walk/e231_gmf_proposal_montecarlo.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "incompatible with proposal_approximation=montecarlo",
        our_needle: "incompatible with proposal_approximation=montecarlo",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E232",
        fixture: "d_walk/e232_gmf_distribution_montecarlo.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "incompatible with distribution_approximation=montecarlo",
        our_needle: "incompatible with distribution_approximation=montecarlo",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E233",
        fixture: "d_walk/e233_estimated_planner_discount.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "that appears in the discount factor of the planner",
        our_needle: "that appears in the discount factor of the planner",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E234",
        fixture: "d_walk/e234_prior_function_no_function.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "require the 'function' option",
        our_needle: "require the 'function' option",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E235",
        fixture: "d_walk/e235_disc_order.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "discretionary_policy: order > 1 is not yet implemented",
        our_needle: "discretionary_policy: order > 1 is not yet implemented",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E236",
        fixture: "d_walk/e236_identification_order.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "order option of identification command must be between 1 and 3",
        our_needle: "order option of identification command must be between 1 and 3",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E237",
        fixture: "d_walk/e237_max_dim_cova_group.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "max_dim_cova_group option to identification only accepts integers > 0",
        our_needle: "max_dim_cova_group option to identification only accepts integers > 0",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E238",
        fixture: "d_walk/e238_stoch_simul_filters.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "stoch_simul: can only use one of HP, one-sided HP, and bandpass filters",
        our_needle: "stoch_simul: can only use one of HP, one-sided HP, and bandpass filters",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E239",
        fixture: "d_walk/e239_stoch_simul_z.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "Variable z was not declared",
        our_needle: "Variable z was not declared",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E240",
        fixture: "d_walk/e240_stoch_simul_rho.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "is not one of {endogenous}",
        our_needle: "is not one of {endogenous}",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "W201",
        fixture: "d_walk/w201_restriction_fname.mod",
        kind: HonestyKind::Warning,
        their_needle: "restriction_fname is now deprecated",
        our_needle: "restriction_fname is now deprecated",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "W202",
        fixture: "d_walk/w202_stoch_simul_dup.mod",
        kind: HonestyKind::Warning,
        their_needle: "found more than once in symbol list",
        our_needle: "found more than once in symbol list",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E241",
        fixture: "d_block/e241_histval_missing_exo.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "exogenous variables in endval",
        our_needle: "exogenous variables in endval",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E242",
        fixture: "d_block/e242_histval_lag.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "the lag on y should be less than or equal to 0",
        our_needle: "the lag on y should be less than or equal to 0",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E243",
        fixture: "d_block/e243_histval_dup.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "hist_val: (y, 0) declared twice",
        our_needle: "hist_val: (y, 0) declared twice",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E244",
        fixture: "d_block/e244_estimated_params_dup.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "the symbol rho is declared twice",
        our_needle: "the symbol rho is declared twice",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E245",
        fixture: "d_block/e245_estimated_params_stderr_dup.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "the stderr of e is declared twice",
        our_needle: "the stderr of e is declared twice",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E246",
        fixture: "d_block/e246_estimated_params_corr_dup.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "the correlation between e and e2 is declared twice",
        our_needle: "the correlation between e and e2 is declared twice",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E247",
        fixture: "d_block/e247_estimated_params_skew_dup.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "the skewness of e is declared twice",
        our_needle: "the skewness of e is declared twice",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E248",
        fixture: "d_block/e248_estimated_params_value_used.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "the value of estimated parameter rho is used",
        our_needle: "the value of estimated parameter rho is used",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E249",
        fixture: "d_block/e249_estimated_params_skew_endo.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "skewness can only be specified for exogenous variables",
        our_needle: "skewness can only be specified for exogenous variables",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E250",
        fixture: "d_block/e250_beta_half_half.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "mean = standard deviation = 0.5",
        our_needle: "mean = standard deviation = 0.5",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E251",
        fixture: "d_block/e251_planner_exo.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "You cannot include exogenous variables",
        our_needle: "You cannot include exogenous variables",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E252",
        fixture: "d_block/e252_planner_lead.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "Leads and lags on variables are forbidden in 'planner_objective'",
        our_needle: "Leads and lags on variables are forbidden in 'planner_objective'",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E253",
        fixture: "d_block/e253_planner_local.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "cannot be used in 'planner_objective'",
        our_needle: "cannot be used in 'planner_objective'",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "W203",
        fixture: "d_block/w203_osr_params_twice.mod",
        kind: HonestyKind::Warning,
        their_needle: "more than one osr_params statement",
        our_needle: "more than one osr_params statement",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E254",
        fixture: "d_block/e254_osr_bounds_before.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "osr_params statement before the osr_params_bounds block",
        our_needle: "osr_params statement before the osr_params_bounds block",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E255",
        fixture: "d_block/e255_osr_bounds_not_param.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "must be a parameter to be used in the osr_bounds block",
        our_needle: "must be a parameter to be used in the osr_bounds block",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E256",
        fixture: "d_block/e256_tag_twice.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "Tag 'name' cannot be used twice",
        our_needle: "Tag 'name' cannot be used twice",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E257",
        fixture: "d_block/e257_default_eq_tag.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "Error creating default equation tag",
        our_needle: "Error creating default equation tag",
        stage: JsonStage::Transform,
    },
    HonestyRow {
        code: "E258",
        fixture: "d_block/e258_several_varobs.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "several 'varobs' statements",
        our_needle: "several 'varobs' statements",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E259",
        fixture: "d_block/e259_several_varexobs.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "several 'varexobs' statements",
        our_needle: "several 'varexobs' statements",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E260",
        fixture: "d_block/e260_varexobs_not_exo.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "varexobs: y is not an exogenous variable",
        our_needle: "varexobs: y is not an exogenous variable",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E261",
        fixture: "d_block/e261_trends_twice.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "observation_trends: y declared twice",
        our_needle: "observation_trends: y declared twice",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E262",
        fixture: "d_block/e262_mcp_lhs_not_var.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "mcp' tag is not a variable",
        our_needle: "mcp' tag is not a variable",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E263",
        fixture: "d_block/e263_mcp_lhs_not_endo.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "not an endogenous variable",
        our_needle: "not an endogenous variable",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E264",
        fixture: "d_block/e264_mcp_rhs_not_const.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "should be a constant",
        our_needle: "should be a constant",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E265",
        fixture: "d_block/e265_mcp_no_inequality.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "does not contain an inequality",
        our_needle: "does not contain an inequality",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E266",
        fixture: "d_block/e266_shock_var_param.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "setting a variance on 'rho'",
        our_needle: "setting a variance on 'rho'",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E267",
        fixture: "d_block/e267_shock_stderr_param.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "setting a standard error on 'rho'",
        our_needle: "setting a standard error on 'rho'",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E268",
        fixture: "d_block/e268_shock_cov_mixed.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "setting a covariance between",
        our_needle: "setting a covariance between",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E269",
        fixture: "d_block/e269_shock_corr_mixed.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "setting a correlation between",
        our_needle: "setting a correlation between",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E270",
        fixture: "d_block/e270_shock_skew_endo.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "skewness can only be specified for exogenous variables",
        our_needle: "skewness can only be specified for exogenous variables",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E271",
        fixture: "d_block/e271_option_twice.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "option dsge_var declared twice",
        our_needle: "option dsge_var declared twice",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E272",
        fixture: "d_block/e272_static_lag.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "An equation tagged [static] cannot contain",
        our_needle: "An equation tagged [static] cannot contain",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E273",
        fixture: "d_block/e273_generate_irfs_name.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "generate_irfs block must be unique",
        our_needle: "generate_irfs block must be unique",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E274",
        fixture: "d_block/e274_generate_irfs_exo.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "You have set the exogenous variable",
        our_needle: "You have set the exogenous variable",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E275",
        fixture: "d_block/e275_namespace.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "Namespace-qualified symbol",
        our_needle: "Namespace-qualified symbol",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E276",
        fixture: "d_block/e276_log_zero.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "log(0) not defined!",
        our_needle: "log(0) not defined!",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E277",
        fixture: "d_block/e277_log10_zero.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "log10(0) not defined!",
        our_needle: "log10(0) not defined!",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E278",
        fixture: "d_block/e278_div_zero.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "Division by zero when forming",
        our_needle: "Division by zero when forming",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E279",
        fixture: "d_block/e279_external_fn_outside.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "name of a MATLAB/Octave function",
        our_needle: "name of a MATLAB/Octave function",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E280",
        fixture: "d_block/e280_external_fn_inside.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "function name external to Dynare",
        our_needle: "function name external to Dynare",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E281",
        fixture: "d_block/e281_mod_file_local.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "not allowed inside model declaration",
        our_needle: "not allowed inside model declaration",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E282",
        fixture: "d_block/e282_model_local_outside.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "not allowed outside model declaration. Its scope is only inside model",
        our_needle: "not allowed outside model declaration. Its scope is only inside model",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E283",
        fixture: "d_block/e283_if_string.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "must evaluate to a boolean or a double",
        our_needle: "must evaluate to a boolean or a double",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E284",
        fixture: "d_block/e284_for_tuple.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "Encountered tuple of size",
        our_needle: "Encountered tuple of size",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E285",
        fixture: "d_block/e285_plus_type.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "Type mismatch for operands of +",
        our_needle: "Type mismatch for operands of +",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E286",
        fixture: "d_open/e286_with_epilogue_without_block.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "with_epilogue' option cannot be specified",
        our_needle: "with_epilogue' option cannot be specified",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E287",
        fixture: "d_open/e287_epilogue_dup.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "in the 'epilogue' block, variable",
        our_needle: "in the 'epilogue' block, variable",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E288",
        fixture: "d_open/e288_epilogue_unknown.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "used in the epilogue block but was not declared",
        our_needle: "used in the epilogue block but was not declared",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E289",
        fixture: "d_open/e289_epilogue_exo.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "because it is an exogenous variable.",
        our_needle: "because it is an exogenous variable.",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E290",
        fixture: "d_open/e290_epilogue_exo_det.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "exogenous deterministic variable.",
        our_needle: "exogenous deterministic variable.",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E291",
        fixture: "d_open/e291_epilogue_expectation.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "expectation' operator is forbidden in 'epilogue'",
        our_needle: "expectation' operator is forbidden in 'epilogue'",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E292",
        fixture: "d_open/e292_epilogue_steady_state.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "STEADY_STATE() operator is forbidden in epilogue",
        our_needle: "STEADY_STATE() operator is forbidden in epilogue",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E293",
        fixture: "d_open/e293_epilogue_sum.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "SUM() operator is forbidden in epilogue",
        our_needle: "SUM() operator is forbidden in epilogue",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E294",
        fixture: "d_open/e294_epilogue_outside.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "cannot be used outside the epilogue block",
        our_needle: "cannot be used outside the epilogue block",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E295",
        fixture: "d_open/e295_change_type_unknown.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "Unknown variable zzz",
        our_needle: "Unknown variable zzz",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E296",
        fixture: "d_open/e296_change_type_used.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "cannot modify the type of symbol",
        our_needle: "cannot modify the type of symbol",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E297",
        fixture: "d_open/e297_ramsey_model_twice.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "Several 'ramsey_model' statements cannot appear",
        our_needle: "Several 'ramsey_model' statements cannot appear",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E298",
        fixture: "d_open/e298_ramsey_model_after_policy.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "ramsey_model' statement cannot follow a 'ramsey_policy'",
        our_needle: "ramsey_model' statement cannot follow a 'ramsey_policy'",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E299",
        fixture: "d_open/e299_ramsey_policy_after_model.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "ramsey_policy' statement cannot follow a 'ramsey_model'",
        our_needle: "ramsey_policy' statement cannot follow a 'ramsey_model'",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E300",
        fixture: "d_open/e300_ramsey_policy_twice.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "Several 'ramsey_policy' statements cannot appear",
        our_needle: "Several 'ramsey_policy' statements cannot appear",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E301",
        fixture: "d_open/e301_planner_discount_model.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "ramsey_model: the 'planner_discount' option cannot be used",
        our_needle: "ramsey_model: the 'planner_discount' option cannot be used",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E302",
        fixture: "d_open/e302_planner_discount_policy.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "ramsey_policy: the 'planner_discount' option cannot be used",
        our_needle: "ramsey_policy: the 'planner_discount' option cannot be used",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E303",
        fixture: "d_open/e303_dsge_prior_weight_parameter.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "dsge_prior_weight cannot be declared as a parameter",
        our_needle: "dsge_prior_weight cannot be declared as a parameter",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E304",
        fixture: "d_open/e304_includepath_missing_dir.mod",
        kind: HonestyKind::Error {
            workspace_only: true,
        },
        their_needle: "does not evaluate to a valid directory",
        our_needle: "does not evaluate to a valid directory",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E305",
        fixture: "d_open/e305_includepath_not_string.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "does not evaluate to a string",
        our_needle: "does not evaluate to a string",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E306",
        fixture: "d_open/e306_load_params_missing.mod",
        kind: HonestyKind::Error {
            workspace_only: true,
        },
        their_needle: "Can't open ",
        our_needle: "Can't open ",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "W204",
        fixture: "d_open/w204_load_params_unknown.mod",
        kind: HonestyKind::Warning,
        their_needle: "Unknown symbol zzz in ",
        our_needle: "Unknown symbol zzz in ",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E307",
        fixture: "d_open/e307_trend_twice.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "Trend variable A was declared twice",
        our_needle: "Trend variable A was declared twice",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E308",
        fixture: "d_open/e308_trend_listed_twice.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "listed more than once as following a trend",
        our_needle: "listed more than once as following a trend",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E309",
        fixture: "d_open/e309_deflator_nonstationary.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "deflator contains a non-stationary endogenous",
        our_needle: "deflator contains a non-stationary endogenous",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E310",
        fixture: "d_open/e310_trend_outside_model.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "because it is a trend variable",
        our_needle: "because it is a trend variable",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E311",
        fixture: "d_open/e311_fis_not_endo.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "filter_initial_state: rho should be an endogenous or exogenous",
        our_needle: "filter_initial_state: rho should be an endogenous or exogenous",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E312",
        fixture: "d_open/e312_fis_exo_no_lag.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "must be provided with a lag",
        our_needle: "must be provided with a lag",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E313",
        fixture: "d_open/e313_fis_dup.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "filter_initial_state: (y, 0) declared twice",
        our_needle: "filter_initial_state: (y, 0) declared twice",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E314",
        fixture: "d_open/e314_fis_lag_mismatch.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "does not appear in the model with the lag",
        our_needle: "does not appear in the model with the lag",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E315",
        fixture: "d_open/e315_optim_weights_dup.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "optim_weights: y declared twice",
        our_needle: "optim_weights: y declared twice",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E316",
        fixture: "d_open/e316_optim_weights_pair_dup.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "optim_weights: pair of variables",
        our_needle: "optim_weights: pair of variables",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E317",
        fixture: "d_open/e317_optim_weights_not_endo.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "is not endogenous.",
        our_needle: "is not endogenous.",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E318",
        fixture: "d_open/e318_ramsey_constraints_two.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "two constraints for variable",
        our_needle: "two constraints for variable",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E319",
        fixture: "d_open/e319_ramsey_constraints_not_inequality.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "This expression is not an inequality",
        our_needle: "This expression is not an inequality",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E320",
        fixture: "d_open/e320_ramsey_constraints_bad_bound.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "Bounds must not contain any endogenous or exogenous",
        our_needle: "Bounds must not contain any endogenous or exogenous",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E321",
        fixture: "d_open/e321_ramsey_constraints_chain.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "Ramsey constraint has an incorrect form:",
        our_needle: "Ramsey constraint has an incorrect form:",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E322",
        fixture: "d_open/e322_extfun_no_name.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "The 'name' option must be passed to external_function",
        our_needle: "The 'name' option must be passed to external_function",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E323",
        fixture: "d_open/e323_extfun_empty_name.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "An argument must be passed to the 'name' option",
        our_needle: "An argument must be passed to the 'name' option",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E324",
        fixture: "d_open/e324_extfun_second_named.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "provided to the external_function command, the first",
        our_needle: "provided to the external_function command, the first",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E325",
        fixture: "d_open/e325_extfun_second_bare.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "provided in the top-level function, the first",
        our_needle: "provided in the top-level function, the first",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E326",
        fixture: "d_open/e326_extfun_nargs_mismatch.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "number of arguments passed to the external_function()",
        our_needle: "number of arguments passed to the external_function()",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E327",
        fixture: "d_open/e327_extfun_first_deriv_mismatch.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "first derivative function passed to the external_function()",
        our_needle: "first derivative function passed to the external_function()",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E328",
        fixture: "d_open/e328_extfun_first_top_second_named.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "second derivative cannot be provided by any other external",
        our_needle: "second derivative cannot be provided by any other external",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E329",
        fixture: "d_open/e329_init2shocks_dup.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "enogenous variable",
        our_needle: "enogenous variable",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E330",
        fixture: "d_open/e330_init2shocks_first_not_endo.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "init2shocks: rho should be an endogenous variable",
        our_needle: "init2shocks: rho should be an endogenous variable",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E331",
        fixture: "d_open/e331_init2shocks_second_not_exo.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "init2shocks: rho should be an exogenous variable",
        our_needle: "init2shocks: rho should be an exogenous variable",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E332",
        fixture: "d_open/e332_homotopy_not_param.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "homotopy_val: y should be a parameter or exogenous",
        our_needle: "homotopy_val: y should be a parameter or exogenous",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E333",
        fixture: "d_open/e333_shock_groups_not_exo.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "shock_groups: rho should be an exogenous variable",
        our_needle: "shock_groups: rho should be an exogenous variable",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E058",
        fixture: "d_open/e058_fis_undeclared.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "Unknown symbol: zzz",
        our_needle: "in filter_initial_state is not declared",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E058",
        fixture: "d_open/e058_init2shocks_undeclared.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "Unknown symbol: zzz",
        our_needle: "in init2shocks is not declared",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E058",
        fixture: "d_open/e058_homotopy_undeclared.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "Unknown symbol: zzz",
        our_needle: "in homotopy_setup is not declared",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E058",
        fixture: "d_open/e058_shock_groups_undeclared.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "Unknown symbol: zzz",
        our_needle: "in shock_groups is not declared",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E030",
        fixture: "d_gap/e030_trend_mixed.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "declared twice with different types",
        our_needle: "declared twice with different types",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E030",
        fixture: "d_gap/e030_trend_and_var.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "declared twice with different types",
        our_needle: "declared twice with different types",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E030",
        fixture: "d_gap/e030_epilogue_and_var.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "declared twice with different types",
        our_needle: "declared twice with different types",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "W031",
        fixture: "d_gap/w031_trend_same_kind.mod",
        kind: HonestyKind::Warning,
        their_needle: "Symbol A declared twice",
        our_needle: "Symbol A declared twice",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "W031",
        fixture: "d_gap/w031_epilogue_dup.mod",
        kind: HonestyKind::Warning,
        their_needle: "Symbol foo declared twice",
        our_needle: "Symbol foo declared twice",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E001",
        fixture: "d_gap/e001_dsge_prior_weight_use.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "unexpected DSGE_PRIOR_WEIGHT",
        our_needle: "reserved preprocessor symbol",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E001",
        fixture: "d_gap/e001_dsge_prior_weight_slot.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "unexpected DSGE_PRIOR_WEIGHT",
        our_needle: "reserved preprocessor symbol",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E287",
        fixture: "d_gap/w031_epilogue_dup.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "in the 'epilogue' block, variable",
        our_needle: "in the 'epilogue' block, variable 'foo' is declared twice",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E307",
        fixture: "d_gap/w031_trend_same_kind.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "Trend variable A was declared twice",
        our_needle: "Trend variable A was declared twice",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "W031",
        fixture: "d_open/e287_epilogue_dup.mod",
        kind: HonestyKind::Warning,
        their_needle: "Symbol foo declared twice",
        our_needle: "Symbol foo declared twice",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "W031",
        fixture: "d_open/e307_trend_twice.mod",
        kind: HonestyKind::Warning,
        their_needle: "Symbol A declared twice",
        our_needle: "Symbol A declared twice",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "W031",
        fixture: "d_open/e308_trend_listed_twice.mod",
        kind: HonestyKind::Warning,
        their_needle: "Symbol y declared twice",
        our_needle: "Symbol y declared twice",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "W031",
        fixture: "d_open/e309_deflator_nonstationary.mod",
        kind: HonestyKind::Warning,
        their_needle: "Symbol z declared twice",
        our_needle: "Symbol z declared twice",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "W031",
        fixture: "d_open/e309_deflator_nonstationary.mod",
        kind: HonestyKind::Warning,
        their_needle: "Symbol w declared twice",
        our_needle: "Symbol w declared twice",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "W031",
        fixture: "d_open/e326_extfun_nargs_mismatch.mod",
        kind: HonestyKind::Warning,
        their_needle: "Symbol foo declared twice",
        our_needle: "Symbol foo declared twice",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "W031",
        fixture: "d_open/e327_extfun_first_deriv_mismatch.mod",
        kind: HonestyKind::Warning,
        their_needle: "Symbol foo declared twice",
        our_needle: "Symbol foo declared twice",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "W150",
        fixture: "d_open/e298_ramsey_model_after_policy.mod",
        kind: HonestyKind::Warning,
        their_needle: "statement is deprecated",
        our_needle: "'ramsey_policy' is deprecated",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "W150",
        fixture: "d_open/e299_ramsey_policy_after_model.mod",
        kind: HonestyKind::Warning,
        their_needle: "statement is deprecated",
        our_needle: "'ramsey_policy' is deprecated",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "W150",
        fixture: "d_open/e300_ramsey_policy_twice.mod",
        kind: HonestyKind::Warning,
        their_needle: "statement is deprecated",
        our_needle: "'ramsey_policy' is deprecated",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "W150",
        fixture: "d_open/e302_planner_discount_policy.mod",
        kind: HonestyKind::Warning,
        their_needle: "statement is deprecated",
        our_needle: "'ramsey_policy' is deprecated",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E030",
        fixture: "d_scope/e030_extfun_deriv_and_var.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "declared twice with different types",
        our_needle: "declared twice with different types",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "W031",
        fixture: "d_scope/w031_extfun_deriv_same_name.mod",
        kind: HonestyKind::Warning,
        their_needle: "Symbol foo declared twice",
        our_needle: "Symbol foo declared twice",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "W031",
        fixture: "d_open/e328_extfun_first_top_second_named.mod",
        kind: HonestyKind::Warning,
        their_needle: "Symbol foo declared twice",
        our_needle: "Symbol foo declared twice",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E334",
        fixture: "d_extfun/e334_extfun_same_function.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "If the Jacobian and Hessian are provided by the same function",
        our_needle: "If the Jacobian and Hessian are provided by the same function",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "W031",
        fixture: "d_extfun/e334_extfun_same_function.mod",
        kind: HonestyKind::Warning,
        their_needle: "Symbol bar declared twice",
        our_needle: "Symbol bar declared twice",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E335",
        fixture: "d_surgery/e335_tag_not_found.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "The equations specified by",
        our_needle: "The equations specified by name=nosuchtag were not found.",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E336",
        fixture: "d_surgery/e336_no_lhs_variable.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "does not have a single variable on its left-hand side",
        our_needle: "does not have a single variable on its left-hand side",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E337",
        fixture: "d_surgery/e337_excluded_twice.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "was excluded twice via a model_remove or model_replace statement",
        our_needle: "was excluded twice via a model_remove or model_replace statement",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E256",
        fixture: "d_surgery/e256_tag_twice_surgery.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "cannot be used twice for the same equation",
        our_needle: "cannot be used twice for the same equation",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E208",
        fixture: "d_surgery/e208_static_after_remove.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "the number of equations marked [static] must be equal to the number of",
        our_needle: "the number of equations marked [static] must be equal to the number of",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E314",
        fixture: "d_surgery/e314_filter_dropped.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "does not appear in the model with the lag -1",
        our_needle: "does not appear in the model with the lag -1",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E020",
        fixture: "d_surgery/e020_dropped_equation_name.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "Unknown symbol: zzz",
        our_needle: "Undeclared identifier 'zzz'",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E251",
        fixture: "d_surgery/e251_planner_exogenous.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "You cannot include exogenous variables (or variables of undeclared type) in the planner objective",
        our_needle: "You cannot include exogenous variables (or variables of undeclared type) in the planner objective",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E317",
        fixture: "d_surgery/e317_after_removal.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "c is not endogenous.",
        our_needle: "c is not endogenous.",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E001",
        fixture: "d_surgery/e001_double_quoted_equation_tag.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "character unrecognized by lexer",
        our_needle: "Double-quoted string in the .mod file",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E001",
        fixture: "d_surgery/e001_double_quoted_bvar.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "character unrecognized by lexer",
        our_needle: "Double-quoted string in the .mod file",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E001",
        fixture: "d_surgery/e001_double_quoted_shock_group.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "character unrecognized by lexer",
        our_needle: "Double-quoted string in the .mod file",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E338",
        fixture: "d_ms/e338_data_no_file.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "The file or series option must be passed to the data statement.",
        our_needle: "The file or series option must be passed to the data statement.",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E339",
        fixture: "d_ms/e339_data_file_and_series.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "The file and series options cannot be used simultaneously",
        our_needle: "The file and series options cannot be used simultaneously",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E340",
        fixture: "d_ms/e340_data_nobs_zero.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "The nobs option of the data statement only accepts positive integers.",
        our_needle: "The nobs option of the data statement only accepts positive integers.",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E341",
        fixture: "d_ms/e341_ms_estimation_missing.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "you must pass the datafile and initial_year options.",
        our_needle: "you must pass the datafile and initial_year options.",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E342",
        fixture: "d_ms/e342_conditional_forecast_no_parameter_set.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "You must pass the `parameter_set` option to conditional_forecast",
        our_needle: "You must pass the `parameter_set` option to conditional_forecast",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E343",
        fixture: "d_ms/e343_cfp_count_mismatch.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "number of periods is different from number of shock values",
        our_needle: "number of periods is different from number of shock values",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E344",
        fixture: "d_ms/e344_cfp_var_twice.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "shocks/conditional_forecast_paths: variable Pie declared twice",
        our_needle: "variable Pie declared twice",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E345",
        fixture: "d_ms/e345_markov_switching_option_missing.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "A 'chain' option must be passed to the 'markov_switching' statement.",
        our_needle: "A 'chain' option must be passed to the 'markov_switching' statement.",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E346",
        fixture: "d_ms/e346_markov_switching_chain_zero.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "The value passed to the chain option must be greater than zero.",
        our_needle: "The value passed to the chain option must be greater than zero.",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E347",
        fixture: "d_ms/e347_markov_switching_regimes_zero.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "The value passed to the number_of_regimes option must be greater than zero.",
        our_needle: "The value passed to the number_of_regimes option must be greater than zero.",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E348",
        fixture: "d_ms/e348_markov_switching_chain_order.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "takes consecutive integers beginning at 1.",
        our_needle: "takes consecutive integers beginning at 1.",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E349",
        fixture: "d_ms/e349_markov_switching_parameters_type.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "must be parameters. Caused by: Pie",
        our_needle: "must be parameters. Caused by: Pie",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E350",
        fixture: "d_ms/e350_markov_switching_restrictions_form.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "restrictions in the subsample statement must be specified in the form",
        our_needle: "restrictions in the subsample statement must be specified in the form",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E351",
        fixture: "d_ms/e351_markov_switching_regime_beyond.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "the regimes specified in the restrictions option must be",
        our_needle: "the regimes specified in the restrictions option must be",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E352",
        fixture: "d_ms/e352_markov_switching_restriction_twice.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "two restrictions were given for: 1, 2",
        our_needle: "two restrictions were given for: 1, 2",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E353",
        fixture: "d_ms/e353_markov_switching_probability_gt_one.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "the transition probability, 1.5 must be less than 1",
        our_needle: "the transition probability, 1.5 must be less than 1",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E354",
        fixture: "d_ms/e354_markov_switching_sums.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "When all transitions probabilities are specified for a certain regime, they must sum to 1",
        our_needle: "they must sum to 1",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E355",
        fixture: "d_ms/e355_markov_switching_partial_sum.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "When transition probabilites are not specified for every regime",
        our_needle: "their sum must be < 1",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E356",
        fixture: "d_ms/e356_svar_identification_twice.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "You may only have one svar_identification block in your .mod file.",
        our_needle: "You may only have one svar_identification block in your .mod file.",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E357",
        fixture: "d_ms/e357_svar_identification_two_cholesky.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "you may only have one of upper_cholesky and lower_cholesky.",
        our_needle: "you may only have one of upper_cholesky and lower_cholesky.",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E358",
        fixture: "d_ms/e358_svar_identification_lag_twice.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "lag 0 used more than once.",
        our_needle: "lag 0 used more than once.",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E359",
        fixture: "d_ms/e359_svar_identification_equation_twice.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "equation number 1 referenced more than once under a single lag.",
        our_needle: "equation number 1 referenced more than once under a single lag.",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E360",
        fixture: "d_ms/e360_svar_identification_equation_zero.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "equation numbers must be greater than or equal to 1.",
        our_needle: "equation numbers must be greater than or equal to 1.",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E361",
        fixture: "d_ms/e361_svar_identification_name_twice.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "Pie restriction added twice.",
        our_needle: "Pie restriction added twice.",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E362",
        fixture: "d_ms/e362_svar_identification_qi_ri.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "a single restrictions must affect either Qi or Ri, but not both",
        our_needle: "must affect either Qi or Ri, but not both",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E363",
        fixture: "d_ms/e363_svar_none_of_three.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "You must pass one of 'coefficients', 'variances', or 'constants'.",
        our_needle: "You must pass one of 'coefficients', 'variances', or 'constants'.",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E364",
        fixture: "d_ms/e364_svar_two_of_three.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "You may only pass one of 'coefficients', 'variances', or 'constants'.",
        our_needle: "You may only pass one of 'coefficients', 'variances', or 'constants'.",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E365",
        fixture: "d_ms/e365_svar_chain_missing.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "A 'chain' option must be passed to the 'svar' statement.",
        our_needle: "A 'chain' option must be passed to the 'svar' statement.",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E366",
        fixture: "d_ms/e366_svar_chain_zero.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "The value passed to the 'chain' option must be greater than zero.",
        our_needle: "The value passed to the 'chain' option must be greater than zero.",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E367",
        fixture: "d_ms/e367_svar_equations_zero.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "The value(s) passed to the 'equations' option must be greater than zero.",
        our_needle: "The value(s) passed to the 'equations' option must be greater than zero.",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E368",
        fixture: "d_ms/e368_ms_compute_probabilities_two.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "You may only pass one of real_time_smoothed and filtered_probabilities to ms_compute_probabilities.",
        our_needle: "You may only pass one of real_time_smoothed and filtered_probabilities to ms_compute_probabilities.",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E369",
        fixture: "d_ms/e369_ms_irf_two.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "You may only pass one of regime, regimes and filtered_probabilities to ms_irf",
        our_needle: "You may only pass one of regime, regimes and filtered_probabilities to ms_irf",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E370",
        fixture: "d_ms/e370_ms_forecast_two.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "You may only pass one of regime and regimes to ms_forecast",
        our_needle: "You may only pass one of regime and regimes to ms_forecast",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E371",
        fixture: "d_ms/e371_ms_variance_decomposition_two.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "You may only pass one of regime, regimes and filtered_probabilities to ms_variance_decomposition",
        our_needle: "You may only pass one of regime, regimes and filtered_probabilities to ms_variance_decomposition",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E372",
        fixture: "d_ms/e372_prior_no_shape.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "You must pass the shape option to the prior statement.",
        our_needle: "You must pass the shape option to the prior statement.",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E373",
        fixture: "d_ms/e373_prior_no_mean_or_mode.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "You must pass at least one of mean and mode to the prior statement.",
        our_needle: "You must pass at least one of mean and mode to the prior statement.",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E374",
        fixture: "d_ms/e374_prior_stdev_and_variance.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "You must pass exactly one of stdev and variance to the prior statement.",
        our_needle: "You must pass exactly one of stdev and variance to the prior statement.",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E375",
        fixture: "d_ms/e375_prior_domain_two_values.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "You must pass exactly two values to the domain option.",
        our_needle: "You must pass exactly two values to the domain option.",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E376",
        fixture: "d_ms/e376_joint_prior_domain_four_values.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "You must pass exactly four values to the domain option.",
        our_needle: "You must pass exactly four values to the domain option.",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E377",
        fixture: "d_ms/e377_joint_prior_one_name.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "you must pass at least two parameters to the joint prior statement",
        our_needle: "you must pass at least two parameters to the joint prior statement",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E378",
        fixture: "d_ms/e378_prior_head_not_parameter.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "Pie is not a parameter",
        our_needle: "Pie is not a parameter",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E379",
        fixture: "d_ms/e379_prior_corr_mixed_types.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "A and B must be of the same type. In your case, Pie and eps",
        our_needle: "A and B must be of the same type. In your case, Pie and eps",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E058",
        fixture: "d_ms/e058_identification_unknown.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "Unknown symbol: nosuchvar.",
        our_needle: "in svar_identification is not declared.",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E058",
        fixture: "d_ms/e058_cfp_var_undeclared.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "Unknown symbol: nosuch.",
        our_needle: "in conditional_forecast_paths is not declared.",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E058",
        fixture: "d_ms/e058_prior_std_undeclared.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "Unknown symbol: nosuchvar.",
        our_needle: "in prior is not declared.",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E317",
        fixture: "d_ms/e317_cfp_var_not_endogenous.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "eps is not endogenous.",
        our_needle: "eps is not endogenous.",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E059",
        fixture: "d_ms/e059_prior_std_not_parameter.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "alpha is neither endogenous or exogenous.",
        our_needle: "alpha is neither endogenous or exogenous.",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E239",
        fixture: "d_ms/e239_undeclared_irf.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "ms_irf: Variable zzz was not declared.",
        our_needle: "ms_irf: Variable zzz was not declared.",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E240",
        fixture: "d_ms/e240_irf_not_endogenous.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "ms_irf: Variable eps is not one of {endogenous}",
        our_needle: "ms_irf: Variable eps is not one of {endogenous}",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E239",
        fixture: "d_ms/e239_pcf_undeclared.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "plot_conditional_forecast: Variable zzz was not declared.",
        our_needle: "plot_conditional_forecast: Variable zzz was not declared.",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E240",
        fixture: "d_ms/e240_pcf_not_endogenous.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "plot_conditional_forecast: Variable eps is not one of {endogenous}",
        our_needle: "plot_conditional_forecast: Variable eps is not one of {endogenous}",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E227",
        fixture: "d_ms/e227_data_after.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "The estimation statement requires a data file to be supplied via the datafile option.",
        our_needle: "The estimation statement requires a data file to be supplied via the datafile option.",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E227",
        fixture: "d_ms/e227_two_estimations.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "The estimation statement requires a data file to be supplied via the datafile option.",
        our_needle: "The estimation statement requires a data file to be supplied via the datafile option.",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E271",
        fixture: "d_ms/kept_extras.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "option ms.freq declared twice",
        our_needle: "option freq declared twice",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E317",
        fixture: "d_ms/e317_prior_std_exo_det.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "dve is an exogenous deterministic.",
        our_needle: "dve is an exogenous deterministic.",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E317",
        fixture: "d_ms/e317_prior_corr_exo_det.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "dve is an exogenous deterministic.",
        our_needle: "dve is an exogenous deterministic.",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E317",
        fixture: "d_ms/e317_prior_std_exo_det_subsample.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "dve is an exogenous deterministic.",
        our_needle: "dve is an exogenous deterministic.",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E058",
        fixture: "d_ms/e058_prior_corr_two_undeclared.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "Unknown symbol: nosuch1.",
        our_needle: "in prior is not declared.",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E059",
        fixture: "d_ms/e059_prior_corr_two_params.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "alpha is neither endogenous or exogenous.",
        our_needle: "alpha is neither endogenous or exogenous.",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E001",
        fixture: "d_ms/e001_cfp_empty.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "unexpected END, expecting VAR",
        our_needle: "conditional_forecast_paths",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E001",
        fixture: "d_ms/e001_cfp_endogenize.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "unexpected ENDOGENIZE, expecting VAR",
        our_needle: "conditional_forecast_paths",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E001",
        fixture: "d_ms/e001_cfp_exogenize.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "unexpected EXOGENIZE, expecting VAR",
        our_needle: "conditional_forecast_paths",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E001",
        fixture: "d_ms/e001_cfp_periods_empty.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "unexpected ';', expecting DATE or INT_NUMBER",
        our_needle: "periods",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E001",
        fixture: "d_ms/e001_cfp_values_empty.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "unexpected ';'",
        our_needle: "values",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E001",
        fixture: "d_ms/e001_conditional_forecast_controlled_varexo_int.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "unexpected INT_NUMBER",
        our_needle: "conditional_forecast",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E001",
        fixture: "d_ms/e001_conditional_forecast_datafile.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "unexpected DATAFILE",
        our_needle: "conditional_forecast",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E001",
        fixture: "d_ms/e001_conditional_forecast_empty_list.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "unexpected ')'",
        our_needle: "at least one option",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E001",
        fixture: "d_ms/e001_conditional_forecast_no_list.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "unexpected ';', expecting '('",
        our_needle: "conditional_forecast",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E001",
        fixture: "d_ms/e001_conditional_forecast_parameter_set_int.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "unexpected INT_NUMBER",
        our_needle: "conditional_forecast",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E001",
        fixture: "d_ms/e001_conditional_forecast_unknown_option.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "unexpected IDENTIFIER",
        our_needle: "conditional_forecast",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E001",
        fixture: "d_ms/e001_data_empty_list.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "unexpected ')'",
        our_needle: "at least one option",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E001",
        fixture: "d_ms/e001_data_first_obs_integer.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "unexpected INT_NUMBER, expecting DATE",
        our_needle: "data",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E001",
        fixture: "d_ms/e001_data_no_list.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "unexpected ';', expecting '('",
        our_needle: "option list",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E001",
        fixture: "d_ms/e001_data_nobs_negative.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "unexpected MINUS",
        our_needle: "data",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E001",
        fixture: "d_ms/e001_data_series_vector.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "unexpected '['",
        our_needle: "data",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E001",
        fixture: "d_ms/e001_data_unknown_option.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "unexpected IDENTIFIER",
        our_needle: "data",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E001",
        fixture: "d_ms/e001_dotted_head_bare.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "unexpected ';', expecting EQUAL or '.'",
        our_needle: "alpha",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E001",
        fixture: "d_ms/e001_dotted_tail_assign.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "unexpected EQUAL, expecting '.'",
        our_needle: "alpha",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E001",
        fixture: "d_ms/e001_dotted_tail_unknown.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "unexpected '(', expecting '.'",
        our_needle: "alpha",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E001",
        fixture: "d_ms/e001_dotted_two_level_tail.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "unexpected IDENTIFIER, expecting OPTIONS or PRIOR",
        our_needle: "alpha",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E001",
        fixture: "d_ms/e001_dsample_list.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "unexpected '(', expecting INT_NUMBER",
        our_needle: "dsample",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E001",
        fixture: "d_ms/e001_identification_empty.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "unexpected END, expecting RESTRICTION",
        our_needle: "svar_identification",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E001",
        fixture: "d_ms/e001_identification_lag_negative.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "unexpected MINUS, expecting INT_NUMBER",
        our_needle: "exclusion lag",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E001",
        fixture: "d_ms/e001_identification_lag_no_equation.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "unexpected END, expecting EQUATION",
        our_needle: "exclusion lag",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E001",
        fixture: "d_ms/e001_identification_stray_equation.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "unexpected EQUATION",
        our_needle: "row of the block's own list",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E001",
        fixture: "d_ms/e001_joint_prior_domain_empty.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "unexpected ']'",
        our_needle: "[…].prior",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E001",
        fixture: "d_ms/e001_joint_prior_scalar_mean.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "unexpected FLOAT_NUMBER, expecting '['",
        our_needle: "[…].prior",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E001",
        fixture: "d_ms/e001_markov_switching_chain_fraction.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "unexpected FLOAT_NUMBER",
        our_needle: "markov_switching",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E001",
        fixture: "d_ms/e001_markov_switching_chain_negative.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "unexpected MINUS, expecting INT_NUMBER",
        our_needle: "markov_switching",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E001",
        fixture: "d_ms/e001_markov_switching_empty_list.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "unexpected ')'",
        our_needle: "at least one option",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E001",
        fixture: "d_ms/e001_markov_switching_no_list.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "unexpected ';', expecting '('",
        our_needle: "markov_switching",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E001",
        fixture: "d_ms/e001_markov_switching_unknown_option.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "unexpected IDENTIFIER",
        our_needle: "markov_switching",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E001",
        fixture: "d_ms/e001_ms_estimation_empty_list.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "unexpected ')'",
        our_needle: "at least one option",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E001",
        fixture: "d_ms/e001_ms_forecast_unknown_option.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "unexpected IDENTIFIER",
        our_needle: "ms_forecast",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E001",
        fixture: "d_ms/e001_ms_irf_empty_list.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "unexpected ')'",
        our_needle: "at least one option",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E001",
        fixture: "d_ms/e001_ms_irf_regime_fraction.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "unexpected FLOAT_NUMBER",
        our_needle: "ms_irf",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E001",
        fixture: "d_ms/e001_ms_irf_unknown_option.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "unexpected IDENTIFIER",
        our_needle: "ms_irf",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E001",
        fixture: "d_ms/e001_plot_cf_no_symbols.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "unexpected ';'",
        our_needle: "plot_conditional_forecast",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E001",
        fixture: "d_ms/e001_prior_domain_empty.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "unexpected ']'",
        our_needle: "prior",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E001",
        fixture: "d_ms/e001_prior_unknown_option.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "unexpected IDENTIFIER",
        our_needle: "prior",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E001",
        fixture: "d_ms/e001_rplot_option_list.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "unexpected '('",
        our_needle: "rplot",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E001",
        fixture: "d_ms/e001_sbvar_unknown_option.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "unexpected IDENTIFIER",
        our_needle: "sbvar",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E001",
        fixture: "d_ms/e001_sgic_option_list.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "unexpected '(', expecting ';'",
        our_needle: "svar_global_identification_check",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E001",
        fixture: "d_ms/e001_smoother2histval_periods.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "unexpected PERIODS",
        our_needle: "smoother2histval",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E001",
        fixture: "d_ms/e001_svar_chain_fraction.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "unexpected FLOAT_NUMBER",
        our_needle: "svar",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E001",
        fixture: "d_ms/e001_svar_constants_flag.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "unexpected CONSTANTS",
        our_needle: "svar",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E001",
        fixture: "d_ms/e001_svar_empty_list.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "unexpected ')', expecting CHAIN",
        our_needle: "at least one option",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E001",
        fixture: "d_ms/e001_svar_equations_empty.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "unexpected ']'",
        our_needle: "svar",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E001",
        fixture: "d_ms/e001_svar_equations_negative.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "unexpected MINUS",
        our_needle: "svar",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E001",
        fixture: "d_ms/e001_svar_no_list.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "unexpected ';', expecting '('",
        our_needle: "svar",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E001",
        fixture: "d_ms/e001_svar_unknown_option.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "unexpected IDENTIFIER",
        our_needle: "svar",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E001",
        fixture: "d_ms/e001_timed_assignment.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "unexpected '(', expecting EQUAL or '.'",
        our_needle: "'y'",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E001",
        fixture: "d_ms/e001_var_remove_option_list.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "unexpected '('",
        our_needle: "var_remove",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E378",
        fixture: "d_ms/e378_assignment_not_parameter.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "y is not a parameter",
        our_needle: "y is not a parameter",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E378",
        fixture: "d_ms/e378_exogenous_assignment.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "e is not a parameter",
        our_needle: "e is not a parameter",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E001",
        fixture: "d_ms/e356_identification_lag_no_equation.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "unexpected EXCLUSION, expecting EQUATION",
        our_needle: "exclusion lag",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "W201",
        fixture: "d_ms/quiet_option_shapes.mod",
        kind: HonestyKind::Warning,
        their_needle: "restriction_fname is now deprecated",
        our_needle: "restriction_fname is now deprecated",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "W022",
        fixture: "d_ms/quiet_option_shapes.mod",
        kind: HonestyKind::Warning,
        their_needle: "not used in the model",
        our_needle: "not used in the model",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "W022",
        fixture: "d_ms/quiet_shape_neighbours.mod",
        kind: HonestyKind::Warning,
        their_needle: "not used in the model",
        our_needle: "not used in the model",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "W022",
        fixture: "d_ms/quiet_handed_over_statement_shapes.mod",
        kind: HonestyKind::Warning,
        their_needle: "not used in the model",
        our_needle: "not used in the model",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "W022",
        fixture: "d_ms/e001_native_assign_pair.mod",
        kind: HonestyKind::Warning,
        their_needle: "not used in the model",
        our_needle: "not used in the model",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "W022",
        fixture: "d_ms/native_assign_heads.mod",
        kind: HonestyKind::Warning,
        their_needle: "not used in the model",
        our_needle: "not used in the model",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E367",
        fixture: "d_ms/e001_svar_equations_scalar_zero.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "must be greater than zero",
        our_needle: "must be greater than zero",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E001",
        fixture: "d_ms/e001_data_controlled_paren_int.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "unexpected INT_NUMBER",
        our_needle: "conditional_forecast",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E001",
        fixture: "d_ms/e001_data_file_integer.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "unexpected INT_NUMBER",
        our_needle: "data",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E001",
        fixture: "d_ms/e001_data_series_integer.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "unexpected INT_NUMBER",
        our_needle: "data",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E001",
        fixture: "d_ms/e001_data_series_quoted.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "unexpected QUOTED_STRING",
        our_needle: "data",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E001",
        fixture: "d_ms/e001_sbvar_restriction_fname_quoted.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "unexpected QUOTED_STRING",
        our_needle: "sbvar",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E001",
        fixture: "d_ms/e001_sbvar_restriction_fname_integer.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "unexpected INT_NUMBER",
        our_needle: "sbvar",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E001",
        fixture: "d_ms/e001_ms_estimation_datafile_integer.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "unexpected INT_NUMBER",
        our_needle: "ms_estimation",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E001",
        fixture: "d_ms/e001_ms_irf_file_tag_integer.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "unexpected INT_NUMBER",
        our_needle: "ms_irf",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E001",
        fixture: "d_ms/e001_markov_switching_shape_before_chain.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "unexpected IDENTIFIER",
        our_needle: "markov_switching",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E001",
        fixture: "d_ms/e001_markov_switching_shape_before_missing.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "unexpected IDENTIFIER",
        our_needle: "markov_switching",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E001",
        fixture: "d_ms/e001_ms_estimation_shape_before_gate.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "unexpected IDENTIFIER",
        our_needle: "ms_estimation",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "E001",
        fixture: "d_ms/e001_conditional_forecast_shape_before_set.mod",
        kind: HonestyKind::Error {
            workspace_only: false,
        },
        their_needle: "unexpected IDENTIFIER",
        our_needle: "conditional_forecast",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "W022",
        fixture: "d_ms/quiet_native_statement_boundaries.mod",
        kind: HonestyKind::Warning,
        their_needle: "not used in the model",
        our_needle: "not used in the model",
        stage: JsonStage::Check,
    },
    HonestyRow {
        code: "W022",
        fixture: "d_ms/quiet_skipped_block_bodies.mod",
        kind: HonestyKind::Warning,
        their_needle: "not used in the model",
        our_needle: "not used in the model",
        stage: JsonStage::Check,
    },
];

struct ClashQuiet {
    code: &'static str,
    fixture: &'static str,
}

const HONESTY_CLASH_QUIET: &[ClashQuiet] = &[
    ClashQuiet {
        code: "E178",
        fixture: "occbin/square.mod",
    },
    ClashQuiet {
        code: "E179",
        fixture: "occbin/square.mod",
    },
    ClashQuiet {
        code: "E104",
        fixture: "clash/e104_two_planner_osr_quiet.mod",
    },
    ClashQuiet {
        code: "E026",
        fixture: "clash/e026_varexo_det_alone_quiet.mod",
    },
    ClashQuiet {
        code: "E027",
        fixture: "w100/w100_ok.mod",
    },
    ClashQuiet {
        code: "E028",
        fixture: "clash/e028_identification_alone_quiet.mod",
    },
    ClashQuiet {
        code: "E113",
        fixture: "clash/e113_shock_paths_alone_quiet.mod",
    },
];

fn copilot_mod(archive_dir: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(".agents/skills/dynare-copilot/references/model-archive")
        .join(archive_dir)
        .join(format!("{archive_dir}.mod"))
}

fn read_mod(archive_dir: &str) -> String {
    std::fs::read_to_string(copilot_mod(archive_dir))
        .unwrap_or_else(|e| panic!("fixture missing: {e}"))
        .replace("\r\n", "\n")
}

fn fixture(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(rel)
}

fn honesty_mod_path(rel: &str) -> PathBuf {
    let path = fixture(rel);
    if path.is_dir() {
        path.join("main.mod")
    } else {
        path
    }
}

fn read_path(path: &Path) -> String {
    std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("fixture missing: {e}"))
        .replace("\r\n", "\n")
}

fn spawn(
    text: &str,
    path: &Path,
    pp: &Path,
    stage: JsonStage,
) -> dygnosis::preprocessor::PreprocessorResult {
    let source_dir = path.parent().map(Path::to_path_buf);
    run_preprocessor(
        text,
        pp,
        source_dir.as_deref(),
        Duration::from_secs(30),
        stage,
    )
}

fn they_mention(result: &dygnosis::preprocessor::PreprocessorResult, needle: &str) -> bool {
    result.raw_stderr.contains(needle)
        || result.raw_stdout.contains(needle)
        || result
            .diagnostics
            .iter()
            .any(|d| d.message.contains(needle))
}

fn assert_no_p_digits(diags: &[Diagnostic], label: &str) {
    for d in diags {
        let rest = d.code.strip_prefix('P').unwrap_or("");
        assert!(
            rest.is_empty() || !rest.chars().all(|c| c.is_ascii_digit()),
            "{label} must not contain P-digit code, got {}",
            d.code
        );
    }
}

fn assert_no_error(own: &[Diagnostic], label: &str) {
    let errors: Vec<_> = own
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .map(|d| d.code.as_str())
        .collect();
    assert!(
        errors.is_empty(),
        "{label} must not emit Error, got {errors:?}"
    );
}

#[test]
fn accepted_archives_emit_no_error() {
    let Some(pp) = find_preprocessor(None) else {
        eprintln!("skipping honesty: dynare-preprocessor not found");
        return;
    };
    for name in ACCEPT_ARCHIVES {
        assert!(
            !NAMED_HOLES.contains(name),
            "named hole {name} must not be in the no-Error loop"
        );
        let text = read_mod(name);
        let path = copilot_mod(name);
        let path_str = path.to_str().expect("utf-8 path");
        let result = spawn(&text, &path, &pp, JsonStage::Check);
        assert!(
            result.success,
            "{name} should be accepted: {:?}",
            result.diagnostics
        );
        assert_no_error(&analyze(&parse(&text)), &format!("{name} analyze()"));
        assert_no_error(&check_file(&text, path_str), &format!("{name} check_file"));
        assert_no_p_digits(&analyze(&parse(&text)), &format!("{name} analyze()"));
        assert_no_p_digits(&check_file(&text, path_str), &format!("{name} check_file"));
    }
}

#[test]
fn named_holes_are_absent_from_no_error_loop() {
    assert!(
        NAMED_HOLES.is_empty(),
        "0.2.0 closed the 0.1.1 named holes; got {NAMED_HOLES:?}"
    );
    for hole in NAMED_HOLES {
        assert!(
            !ACCEPT_ARCHIVES.contains(hole),
            "{hole} must stay off the accepted-archive no-Error loop"
        );
    }
}

#[test]
fn equation_count_is_warning_they_accept() {
    let Some(pp) = find_preprocessor(None) else {
        eprintln!("skipping honesty: dynare-preprocessor not found");
        return;
    };
    let path = fixture("e010/e010_extra.mod");
    let path_str = path.to_str().expect("utf-8 path");
    let text = read_path(&path);
    let result = spawn(&text, &path, &pp, JsonStage::Check);
    assert!(
        result.success,
        "e010_extra.mod should be accepted: {:?}",
        result.diagnostics
    );
    let own = check_file(&text, path_str);
    let w013 = own
        .iter()
        .find(|d| d.code == "W013")
        .expect("own W013 on e010_extra.mod");
    assert_eq!(w013.severity, Severity::Warning);
    assert_no_p_digits(&own, "e010_extra.mod check_file");
}

#[test]
fn linear_log_is_warning_they_accept() {
    let Some(pp) = find_preprocessor(None) else {
        eprintln!("skipping honesty: dynare-preprocessor not found");
        return;
    };
    let path = fixture("d_check/w140_linear_log.mod");
    let path_str = path.to_str().expect("utf-8 path");
    let text = read_path(&path);
    let result = spawn(&text, &path, &pp, JsonStage::Check);
    assert!(
        result.success,
        "w140_linear_log.mod should be accepted: stderr {:?} diags {:?}",
        result.raw_stderr, result.diagnostics
    );
    let own = check_file(&text, path_str);
    let w140 = own
        .iter()
        .find(|d| d.code == "W140")
        .expect("own W140 on w140_linear_log.mod");
    assert_eq!(w140.severity, Severity::Warning);
    assert!(
        own.iter().all(|d| d.code != "E210" && d.code != "E211"),
        "log in model(linear) must not be E210/E211, got {:?}",
        own.iter().map(|d| d.code.as_str()).collect::<Vec<_>>()
    );
    assert_no_p_digits(&own, "w140_linear_log.mod check_file");
}

#[test]
fn same_ground_warning_absent_on_quiet_archive() {
    let Some(pp) = find_preprocessor(None) else {
        eprintln!("skipping honesty: dynare-preprocessor not found");
        return;
    };
    let text = read_mod("trend_rbc_gov_inv");
    let path = copilot_mod("trend_rbc_gov_inv");
    let path_str = path.to_str().expect("utf-8 path");
    let result = spawn(&text, &path, &pp, JsonStage::Check);
    assert!(
        result.success,
        "trend_rbc_gov_inv should be accepted: {:?}",
        result.diagnostics
    );
    let own = check_file(&text, path_str);
    for code in SAME_GROUND_WARNINGS {
        assert!(
            own.iter().all(|d| d.code != *code),
            "quiet archive must not emit same-ground Warning {code}, got {:?}",
            own.iter().map(|d| d.code.as_str()).collect::<Vec<_>>()
        );
    }
}

#[test]
fn honesty_fire_table() {
    let Some(pp) = find_preprocessor(None) else {
        eprintln!("skipping honesty: dynare-preprocessor not found");
        return;
    };
    let mut failures: Vec<String> = Vec::new();
    for row in HONESTY_FIRE {
        let path = honesty_mod_path(row.fixture);
        let path_str = path.to_str().expect("utf-8 path");
        let text = read_path(&path);
        let result = spawn(&text, &path, &pp, row.stage);
        let own_file = check_file(&text, path_str);
        let own_codes: Vec<&str> = own_file.iter().map(|d| d.code.as_str()).collect();
        if own_file.iter().any(|d| {
            let rest = d.code.strip_prefix('P').unwrap_or("");
            !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit())
        }) {
            failures.push(format!(
                "{} check_file has P-digit code: {own_codes:?}",
                row.fixture
            ));
        }
        let has_own = |diags: &[Diagnostic]| diags.iter().any(|d| d.code == row.code);
        match row.kind {
            HonestyKind::Error { workspace_only } => {
                if result.success {
                    failures.push(format!(
                        "{} should be refused at {:?}: stdout {:?} stderr {:?} diags {:?}",
                        row.fixture,
                        row.stage,
                        result.raw_stdout,
                        result.raw_stderr,
                        result.diagnostics
                    ));
                }
                if !has_own(&own_file) {
                    failures.push(format!(
                        "{} check_file must emit {}, got {own_codes:?}",
                        row.fixture, row.code
                    ));
                } else if !own_file
                    .iter()
                    .any(|d| d.code == row.code && d.message.contains(row.our_needle))
                {
                    failures.push(format!(
                        "{} own {} missing {:?} on every row: {:?}",
                        row.fixture,
                        row.code,
                        row.our_needle,
                        own_file
                            .iter()
                            .filter(|d| d.code == row.code)
                            .map(|d| d.message.as_str())
                            .collect::<Vec<_>>()
                    ));
                }
                if !workspace_only {
                    let own_analyze = analyze(&parse(&text));
                    if !has_own(&own_analyze) {
                        failures.push(format!(
                            "{} analyze() must emit {}, got {:?}",
                            row.fixture,
                            row.code,
                            own_analyze
                                .iter()
                                .map(|d| d.code.as_str())
                                .collect::<Vec<_>>()
                        ));
                    }
                }
                if !they_mention(&result, row.their_needle) {
                    failures.push(format!(
                        "{} they should mention {:?}, stdout {:?} stderr {:?} diags {:?}",
                        row.fixture,
                        row.their_needle,
                        result.raw_stdout,
                        result.raw_stderr,
                        result.diagnostics
                    ));
                }
            }
            HonestyKind::Warning => {
                if !they_mention(&result, row.their_needle) {
                    failures.push(format!(
                        "{} they should WARN with {:?}, stdout {:?} stderr {:?} diags {:?}",
                        row.fixture,
                        row.their_needle,
                        result.raw_stdout,
                        result.raw_stderr,
                        result.diagnostics
                    ));
                }
                match own_file.iter().find(|d| d.code == row.code) {
                    None => failures.push(format!(
                        "{} own must emit {}, got {own_codes:?}",
                        row.fixture, row.code
                    )),
                    Some(ours) => {
                        if ours.severity != Severity::Warning {
                            failures.push(format!(
                                "{} {} must be Warning, got {:?}",
                                row.fixture, row.code, ours.severity
                            ));
                        }
                        if !own_file
                            .iter()
                            .any(|d| d.code == row.code && d.message.contains(row.our_needle))
                        {
                            failures.push(format!(
                                "{} own {} missing {:?} on every row",
                                row.fixture, row.code, row.our_needle
                            ));
                        }
                    }
                }
            }
        }
    }
    assert!(
        failures.is_empty(),
        "honesty fire table failed:\n{}",
        failures.join("\n")
    );
}

#[test]
fn extra_cycle_warning_is_library_only() {
    let path = fixture("e060/cycle/a.mod");
    let path_str = path.to_str().expect("utf-8 path");
    let text = read_path(&path);
    let analyzed = analyze(&parse(&text));
    assert!(
        analyzed.iter().all(|d| d.code != "W062"),
        "analyze() must not emit W062, got {:?}",
        analyzed.iter().map(|d| d.code.as_str()).collect::<Vec<_>>()
    );
    let own = check_file(&text, path_str);
    assert!(
        own.iter()
            .any(|d| d.code == "W062" && d.severity == Severity::Warning),
        "check_file on cycle a.mod must emit W062 Warning, got {:?}",
        own.iter().map(|d| d.code.as_str()).collect::<Vec<_>>()
    );
    assert_no_p_digits(&own, "e060/cycle check_file");
}

#[test]
fn e103_planner_objective_without_optim_weights_is_quiet() {
    let Some(pp) = find_preprocessor(None) else {
        eprintln!("skipping honesty: dynare-preprocessor not found");
        return;
    };
    let path = fixture("w100/w103_planner.mod");
    let path_str = path.to_str().expect("utf-8 path");
    let text = read_path(&path);
    let result = spawn(&text, &path, &pp, JsonStage::Check);
    assert!(
        result.success,
        "osr + osr_params + planner_objective should be accepted: stderr {:?} diags {:?}",
        result.raw_stderr, result.diagnostics
    );
    let own = check_file(&text, path_str);
    assert!(
        own.iter().all(|d| d.code != "E103"),
        "no E103 when planner_objective is present, got {:?}",
        own.iter().map(|d| d.code.as_str()).collect::<Vec<_>>()
    );
    assert_no_p_digits(&own, "w103_planner.mod check_file");
}

#[test]
fn occbin_square_is_quiet() {
    let Some(pp) = find_preprocessor(None) else {
        eprintln!("skipping honesty: dynare-preprocessor not found");
        return;
    };
    let path = fixture("occbin/square.mod");
    let path_str = path.to_str().expect("utf-8 path");
    let text = read_path(&path);
    let result = spawn(&text, &path, &pp, JsonStage::Check);
    assert!(
        result.success,
        "occbin/square.mod should be accepted at check: {:?}",
        result.diagnostics
    );
    let transformed = spawn(&text, &path, &pp, JsonStage::Transform);
    assert!(
        transformed.success,
        "occbin/square.mod should be accepted at transform: stdout {:?} stderr {:?}",
        transformed.raw_stdout, transformed.raw_stderr
    );
    let occbin_errors = [
        "E170", "E171", "E172", "E173", "E174", "E175", "E176", "E177", "E178", "E179", "E180",
        "E181", "E182", "E183", "E184", "E185",
    ];
    for own in [&analyze(&parse(&text)), &check_file(&text, path_str)] {
        let errors: Vec<_> = own
            .iter()
            .filter(|d| d.severity == Severity::Error && occbin_errors.contains(&d.code.as_str()))
            .map(|d| d.code.as_str())
            .collect();
        assert!(
            errors.is_empty(),
            "square.mod must not emit OccBin Error, got {errors:?}"
        );
    }
}

#[test]
fn clash_quiet_at_transform() {
    let Some(pp) = find_preprocessor(None) else {
        eprintln!("skipping honesty: dynare-preprocessor not found");
        return;
    };
    let mut failures: Vec<String> = Vec::new();
    for row in HONESTY_CLASH_QUIET {
        let path = fixture(row.fixture);
        let path_str = path.to_str().expect("utf-8 path");
        let text = read_path(&path);
        let result = spawn(&text, &path, &pp, JsonStage::Transform);
        if !result.success {
            failures.push(format!(
                "{} should be accepted at transform for {}: stdout {:?} stderr {:?}",
                row.fixture, row.code, result.raw_stdout, result.raw_stderr
            ));
        }
        let own = check_file(&text, path_str);
        if own.iter().any(|d| d.code == row.code) {
            failures.push(format!(
                "{} must not emit {}, got {:?}",
                row.fixture,
                row.code,
                own.iter().map(|d| d.code.as_str()).collect::<Vec<_>>()
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "clash quiet transform failed:\n{}",
        failures.join("\n")
    );
}

/// Official messages we knowingly do not mirror on a surface file, with the
/// reason. Anything else must be claimed by a `HONESTY_FIRE` needle.
const SURFACE_WAIVERS: &[(&str, &str)] = &[];

/// Every official ERROR/WARNING line on a D-open / D-gap / D-extfun surface
/// file must be claimed by a needle in `HONESTY_FIRE` for that fixture (or
/// waived above).
/// This is the direction honesty rows cannot see: they check the codes we know
/// about, this checks the messages they print. Official lines that carry no
/// `ERROR: ` / `WARNING: ` prefix are out of scope here.
fn official_message_lines(result: &dygnosis::preprocessor::PreprocessorResult) -> Vec<String> {
    result
        .raw_stdout
        .lines()
        .chain(result.raw_stderr.lines())
        .filter(|line| line.starts_with("ERROR: ") || line.starts_with("WARNING: "))
        .map(|line| line.trim().to_string())
        .collect()
}

#[test]
fn surface_matrix_claims_every_official_message() {
    let Some(pp) = find_preprocessor(None) else {
        eprintln!("skipping honesty: dynare-preprocessor not found");
        return;
    };
    let mut fixtures: Vec<&str> = HONESTY_FIRE
        .iter()
        .map(|row| row.fixture)
        .filter(|fixture| {
            fixture.starts_with("d_open/")
                || fixture.starts_with("d_gap/")
                || fixture.starts_with("d_extfun/")
                || fixture.starts_with("d_surgery/")
                || fixture.starts_with("d_ms/")
        })
        .collect();
    fixtures.sort_unstable();
    fixtures.dedup();

    let mut failures = Vec::new();
    for fixture in fixtures {
        let path = honesty_mod_path(fixture);
        let text = read_path(&path);
        let result = spawn(&text, &path, &pp, JsonStage::Check);
        let claims: Vec<&str> = HONESTY_FIRE
            .iter()
            .filter(|row| row.fixture == fixture)
            .map(|row| row.their_needle)
            .collect();
        for line in official_message_lines(&result) {
            if claims.iter().any(|needle| line.contains(needle)) {
                continue;
            }
            if SURFACE_WAIVERS
                .iter()
                .any(|(waived, needle)| *waived == fixture && line.contains(needle))
            {
                continue;
            }
            failures.push(format!("{fixture}: unclaimed official message {line:?}"));
        }
    }
    assert!(
        failures.is_empty(),
        "surface matrix failed:\n{}",
        failures.join("\n")
    );
}
