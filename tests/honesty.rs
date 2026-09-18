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

const SAME_GROUND_WARNINGS: &[&str] = &["W022", "W031", "W042", "W121", "W131", "W150", "W170", "W200"];

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
                } else {
                    let ours = own_file
                        .iter()
                        .find(|d| d.code == row.code)
                        .expect(row.code);
                    if !ours.message.contains(row.our_needle) {
                        failures.push(format!(
                            "{} own {} missing {:?}: {}",
                            row.fixture, row.code, row.our_needle, ours.message
                        ));
                    }
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
                        if !ours.message.contains(row.our_needle) {
                            failures.push(format!(
                                "{} own {} missing {:?}: {}",
                                row.fixture, row.code, row.our_needle, ours.message
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
