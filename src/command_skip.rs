//! Parse skip for Dynare command statements (option lists are not assignments).

use crate::lexer::{Token, TokenKind};
use crate::span::Span;

/// 0.1 command names that are not in `COMMAND_OPTIONS`.
const PARSE_SKIP_EXTRAS: &[&str] = &["dynasave", "dynatype", "model_diagnostics"];

/// 0.9 P-hank: the heterogeneity family is parsed, not skipped.
const PARSED_FAMILY: &[&str] = &[
    "heterogeneity_compute_steady_state",
    "heterogeneity_dimension",
    "heterogeneity_load_steady_state",
    "heterogeneity_simulate",
    "heterogeneity_solve",
];

/// The pin's `DynareFlex.ll` `INITIAL` keyword set (`9c61fb6e`): every spelling that
/// makes a line enter a Dynare statement on its own. A top-level line whose head is
/// neither one of these nor a declared symbol is native MATLAB text, and 7.1 makes
/// no language claim on it.
const PIN_STATEMENT_KEYWORDS: &[&str] = &[
    "bvar_density",
    "bvar_forecast",
    "bvar_irf",
    "calib_smoother",
    "change_type",
    "check",
    "compilation_setup",
    "conditional_forecast",
    "conditional_forecast_paths",
    "corr",
    "data",
    "database",
    "deterministic_trends",
    "discretionary_policy",
    "dsample",
    "dynasave",
    "dynatype",
    "endval",
    "epilogue",
    "estimated_params",
    "estimated_params_bounds",
    "estimated_params_init",
    "estimated_params_remove",
    "estimation",
    "evaluate_planner_objective",
    "extended_path",
    "external_function",
    "filter_initial_state",
    "forecast",
    "generate_irfs",
    "heterogeneity_compute_steady_state",
    "heterogeneity_dimension",
    "heterogeneity_load_steady_state",
    "heterogeneity_simulate",
    "heterogeneity_solve",
    "heteroskedastic_shocks",
    "histval",
    "histval_file",
    "homotopy_setup",
    "identification",
    "init2shocks",
    "initial_condition_decomposition",
    "initval",
    "initval_file",
    "irf_calibration",
    "load_params_and_steady_state",
    "log_trend_var",
    "markov_switching",
    "matched_irfs",
    "matched_irfs_weights",
    "matched_moments",
    "method_of_moments",
    "model",
    "model_comparison",
    "model_diagnostics",
    "model_info",
    "model_local_variable",
    "model_options",
    "model_remove",
    "model_replace",
    "moment_calibration",
    "ms_compute_mdd",
    "ms_compute_probabilities",
    "ms_estimation",
    "ms_forecast",
    "ms_irf",
    "ms_simulation",
    "ms_variance_decomposition",
    "mshocks",
    "observation_trends",
    "occbin_constraints",
    "occbin_graph",
    "occbin_setup",
    "occbin_solver",
    "occbin_write_regimes",
    "optim_weights",
    "osr",
    "osr_params",
    "osr_params_bounds",
    "pac_model",
    "pac_target_info",
    "parameters",
    "perfect_foresight_controlled_paths",
    "perfect_foresight_setup",
    "perfect_foresight_solver",
    "perfect_foresight_with_expectation_errors_setup",
    "perfect_foresight_with_expectation_errors_solver",
    "planner_objective",
    "plot_conditional_forecast",
    "plot_shock_decomposition",
    "posterior_function",
    "predetermined_variables",
    "prior_function",
    "priors",
    "ramsey_constraints",
    "ramsey_model",
    "ramsey_policy",
    "realtime_shock_decomposition",
    "resid",
    "rplot",
    "save_params_and_steady_state",
    "sbvar",
    "sensitivity",
    "set_time",
    "shock_decomposition",
    "shock_groups",
    "shock_paths",
    "shocks",
    "simul",
    "smoother2histval",
    "squeeze_shock_decomposition",
    "std",
    "steady",
    "steady_state_model",
    "stoch_simul",
    "svar",
    "svar_global_identification_check",
    "svar_identification",
    "trend_component_model",
    "trend_var",
    "var",
    "var_expectation_model",
    "var_model",
    "var_remove",
    "varexo",
    "varexo_det",
    "varexobs",
    "varobs",
    "verbatim",
    "write_latex_dynamic_model",
    "write_latex_original_model",
    "write_latex_static_model",
    "write_latex_steady_state_model",
];

/// Whether `name` is one of the pin's statement keywords.
pub(crate) fn is_pin_statement_keyword(name: &str) -> bool {
    PIN_STATEMENT_KEYWORDS
        .iter()
        .any(|kw| name.eq_ignore_ascii_case(kw))
}

/// Catalog command or 0.1 extra. A command *statement* is this name plus `(` or `;`, not `=`.
pub(crate) fn is_parse_skip_command(name: &str) -> bool {
    if PARSED_FAMILY.iter().any(|c| name.eq_ignore_ascii_case(c)) {
        return false;
    }
    crate::catalog::is_known_command(name)
        || PARSE_SKIP_EXTRAS
            .iter()
            .any(|c| name.eq_ignore_ascii_case(c))
}

fn skip_balanced(tokens: &[Token], mut i: usize, open: TokenKind, close: TokenKind) -> usize {
    if i >= tokens.len() || tokens[i].kind != open {
        return i;
    }
    let mut depth = 1;
    i += 1;
    while i < tokens.len() && depth > 0 {
        if tokens[i].kind == open {
            depth += 1;
        } else if tokens[i].kind == close {
            depth -= 1;
        }
        i += 1;
    }
    i
}

/// One span per skip-command token whose next token is `(` or `;` (not `=`),
/// covering optional balanced `(…)` through the closing `;`.
pub(crate) fn command_stmt_spans(tokens: &[Token], src: &str) -> Vec<Span> {
    let mut spans = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        let tok = &tokens[i];
        if tok.kind == TokenKind::Ident && is_parse_skip_command(tok.text(src)) {
            let next_kind = tokens.get(i + 1).map(|t| t.kind);
            if next_kind == Some(TokenKind::LParen) || next_kind == Some(TokenKind::Semi) {
                let mut j = i + 1;
                if next_kind == Some(TokenKind::LParen) {
                    j = skip_balanced(tokens, j, TokenKind::LParen, TokenKind::RParen);
                }
                if tokens.get(j).is_some_and(|t| t.kind == TokenKind::Semi) {
                    spans.push(Span {
                        start: tok.span.start,
                        end: tokens[j].span.end,
                    });
                    i = j + 1;
                    continue;
                }
            }
        }
        i += 1;
    }
    spans
}
