//! Native recursive-descent parser over the token stream.

use std::collections::{hash_map::Entry, BTreeMap, HashMap};
use std::ops::Range;

use crate::expr::{BinOp, ExprId, ExprKind, UnOp};
use crate::intern::{Interner, Name};
use crate::lexer::{tokenize, Token, TokenKind};
use crate::macro_expand::expand_macros_full;
use crate::model::{
    Assignment, CalibrationRange, ConditionalForecastPath, ConditionalForecastPaths, DataStatement,
    Decl, DottedHead, DottedKind, DottedStatement, Equation, EstimationStatement, FamilyOption,
    FamilyValueKind, Init2ShocksBlock, Init2ShocksRow, IrfCalibrationBlock, IrfCalibrationRow,
    MatchedIrfsBlock, MatchedIrfsRow, MatchedIrfsWeight, MatchedIrfsWeightsBlock, MatchedMoment,
    Model, MomStatement, MomSyntax, MomentCalibrationBlock, MomentCalibrationRow, MsStatement,
    OptimWeight, ParseIssue, ParseIssueKind, ShapeRefuse, ShockGroup, ShockGroupBlock,
    SvarEquation, SvarIdentification, SvarIdentificationElement,
};
use crate::model::{
    ChangeTypeKind, ChangeTypeStmt, CommandSymbol, Complementarity, ComplementarityTriple,
    DeprecatedOption, DerivSpec, EquationSurgery, EstimatedParam, EstimatedParamKind,
    EstimationDsgeVarStmt, ExternalFunctionStmt, GenerateIrfsElement, HeterogeneityCommand,
    HeterogeneityCommandKind, HeterogeneityDimension, HeterogeneityOption, HeterogeneousModelBlock,
    HistvalEntry, HomotopyRow, IncludeDirective, IncludePathDirective, MacroDirective, MacroInterp,
    NonstationaryVar, ObservedVar, OccbinConstraint, OccbinExpr, OsrBound, PolicyCommand,
    PolicyCommandStatement, RamseyConstraint, RemovedEquation, ShockKind, ShockStmt,
    ShocksSemiFamily, SurgeryExit, SurgeryKind, TrendVar, VarRemovedName,
};
use crate::span::Span;

mod pac_parser;
mod shock_parser;

#[derive(Clone, Debug)]
enum FoldKey {
    Number(u64),
    Ident(Name, i32),
    Neg(Box<FoldKey>),
    Binary(BinOp, Box<FoldKey>, Box<FoldKey>),
    Call(Name, Vec<FoldKey>),
    SteadyState(Box<FoldKey>),
    Expectation(i32, Box<FoldKey>),
    Other(ExprId),
}

impl PartialEq for FoldKey {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Number(a), Self::Number(b)) => a == b,
            (Self::Ident(a, at), Self::Ident(b, bt)) => a == b && at == bt,
            (Self::Neg(a), Self::Neg(b)) | (Self::SteadyState(a), Self::SteadyState(b)) => a == b,
            (Self::Binary(aop, al, ar), Self::Binary(bop, bl, br)) => {
                aop == bop
                    && ((al == bl && ar == br)
                        || (matches!(aop, BinOp::Add | BinOp::Mul) && al == br && ar == bl))
            }
            (Self::Call(an, aa), Self::Call(bn, ba)) => an == bn && aa == ba,
            (Self::Expectation(as_, a), Self::Expectation(bs, b)) => as_ == bs && a == b,
            (Self::Other(a), Self::Other(b)) => a == b,
            _ => false,
        }
    }
}

impl Eq for FoldKey {}

impl FoldKey {
    fn number(value: f64) -> Self {
        Self::Number(if value == 0.0 { 0 } else { value.to_bits() })
    }

    fn is_zero(&self) -> bool {
        matches!(self, Self::Number(0))
    }

    fn is_one(&self) -> bool {
        *self == Self::number(1.0)
    }

    fn neg(value: Self) -> Self {
        match value {
            Self::Number(bits) => Self::number(-f64::from_bits(bits)),
            Self::Neg(inner) => *inner,
            other => Self::Neg(Box::new(other)),
        }
    }

    fn add(left: Self, right: Self) -> Self {
        if left.is_zero() {
            return right;
        }
        if right.is_zero() {
            return left;
        }
        if let Self::Neg(inner) = &right {
            return Self::sub(left, (**inner).clone());
        }
        if let Self::Neg(inner) = &left {
            return Self::sub(right, (**inner).clone());
        }
        if let Self::Binary(BinOp::Sub, x, y) = &left {
            if **y == right {
                return (**x).clone();
            }
        }
        if let Self::Binary(BinOp::Sub, x, y) = &right {
            if **y == left {
                return (**x).clone();
            }
        }
        Self::Binary(BinOp::Add, Box::new(left), Box::new(right))
    }

    fn sub(left: Self, right: Self) -> Self {
        if right.is_zero() {
            return left;
        }
        if left.is_zero() {
            return Self::neg(right);
        }
        if left == right {
            return Self::number(0.0);
        }
        if let Self::Neg(inner) = &right {
            return Self::add(left, (**inner).clone());
        }
        if let Self::Binary(BinOp::Add, x, y) = &left {
            if **x == right {
                return (**y).clone();
            }
            if **y == right {
                return (**x).clone();
            }
        }
        Self::Binary(BinOp::Sub, Box::new(left), Box::new(right))
    }

    fn mul(left: Self, right: Self) -> Self {
        if left.is_zero() || right.is_zero() {
            return Self::number(0.0);
        }
        if left.is_one() {
            return right;
        }
        if right.is_one() {
            return left;
        }
        Self::Binary(BinOp::Mul, Box::new(left), Box::new(right))
    }
}

pub fn parse(text: &str) -> Model {
    let source = normalize_newlines(text);
    let raw_tokens = tokenize(&source);
    let (includes, includepaths, macro_directives, macro_interps) =
        collect_include_dirs(&source, &raw_tokens);
    let (tokens, macro_type_errors) = expand_macros_full(&source, raw_tokens);
    let (mut model, _ranges) = parse_expanded(&source, tokens);
    model.includes = includes;
    model.includepaths = includepaths;
    model.macro_directives = macro_directives;
    model.macro_interps = macro_interps;
    model.macro_type_errors = macro_type_errors;
    model
}

pub(crate) struct EquationTokenRanges {
    pub aggregate: Vec<Range<usize>>,
    pub heterogeneous: Vec<Vec<Range<usize>>>,
}

pub(crate) fn parse_expanded(src: &str, tokens: Vec<Token>) -> (Model, EquationTokenRanges) {
    let mut p = Parser {
        src,
        tokens,
        i: 0,
        intern: Interner::default(),
        model: Model::default(),
        eq_token_ranges: Vec::new(),
        hetero_eq_token_ranges: Vec::new(),
        verbatim_ranges: Vec::new(),
        symbol_list_id: 1,
        in_model: false,
        in_equation_body: false,
    };
    p.parse_file();
    p.record_double_quoted_strings();
    p.model
        .exogenous
        .extend(p.model.deterministic_exogenous.iter().cloned());
    p.model
        .exogenous
        .sort_by_key(|d| (d.span.start, d.span.end));
    p.record_keyword_typos();
    p.record_missing_assign_semis();
    let Parser {
        intern,
        mut model,
        eq_token_ranges,
        hetero_eq_token_ranges,
        ..
    } = p;
    model.source = src.to_string();
    model.intern = intern;
    (
        model,
        EquationTokenRanges {
            aggregate: eq_token_ranges,
            heterogeneous: hetero_eq_token_ranges,
        },
    )
}

struct TopOption {
    ident: String,
    span: Span,
    eq: bool,
    value_lex: String,
    value_span: Span,
}

/// One option value read for a family statement record.
struct FamilyValue {
    kind: FamilyValueKind,
    span: Span,
    text: String,
    names: Vec<(Name, Span)>,
    /// Token offset after the value.
    next: usize,
}

impl FamilyValue {
    /// A bare flag: no value text. `span` is the option name's span, matching
    /// `FamilyOption::value_span`'s documented meaning.
    fn empty(span: Span, next: usize) -> Self {
        Self {
            kind: FamilyValueKind::Flag,
            span,
            text: String::new(),
            names: Vec::new(),
            next,
        }
    }
}
struct ParsedTag {
    static_tag: bool,
    dynamic_tag: bool,
    flags: Vec<String>,
    map: BTreeMap<String, String>,
    twice: Vec<(String, Span)>,
}

fn parse_int_lexeme(lex: &str) -> Option<i32> {
    lex.parse::<i32>().ok()
}

fn change_type_kind(lex: &str) -> Option<ChangeTypeKind> {
    if lex.eq_ignore_ascii_case("parameters") {
        Some(ChangeTypeKind::Parameters)
    } else if lex.eq_ignore_ascii_case("var") {
        Some(ChangeTypeKind::Var)
    } else if lex.eq_ignore_ascii_case("varexo_det") {
        Some(ChangeTypeKind::VarexoDet)
    } else if lex.eq_ignore_ascii_case("varexo") {
        Some(ChangeTypeKind::Varexo)
    } else {
        None
    }
}

/// Commands whose `(?)` option list may carry `with_epilogue`.
fn is_decomposition_command(cmd: &str) -> bool {
    cmd.eq_ignore_ascii_case("shock_decomposition")
        || cmd.eq_ignore_ascii_case("realtime_shock_decomposition")
        || cmd.eq_ignore_ascii_case("initial_condition_decomposition")
}

fn is_trailing_symbol_command(cmd: &str) -> bool {
    cmd.eq_ignore_ascii_case("stoch_simul")
        || cmd.eq_ignore_ascii_case("estimation")
        || cmd.eq_ignore_ascii_case("calib_smoother")
        || cmd.eq_ignore_ascii_case("ms_irf")
        || cmd.eq_ignore_ascii_case("plot_conditional_forecast")
        || cmd.eq_ignore_ascii_case("forecast")
        || cmd.eq_ignore_ascii_case("rplot")
        || cmd.eq_ignore_ascii_case("dynasave")
        || cmd.eq_ignore_ascii_case("dynatype")
        || cmd.eq_ignore_ascii_case("shock_decomposition")
        || cmd.eq_ignore_ascii_case("realtime_shock_decomposition")
        || cmd.eq_ignore_ascii_case("initial_condition_decomposition")
        || cmd.eq_ignore_ascii_case("plot_shock_decomposition")
        || cmd.eq_ignore_ascii_case("squeeze_shock_decomposition")
}

fn handed_option_command(cmd: &str) -> bool {
    cmd.eq_ignore_ascii_case("forecast")
        || cmd.eq_ignore_ascii_case("shock_decomposition")
        || cmd.eq_ignore_ascii_case("realtime_shock_decomposition")
        || cmd.eq_ignore_ascii_case("initial_condition_decomposition")
        || cmd.eq_ignore_ascii_case("plot_shock_decomposition")
}

/// The internal option name 7.2's twice-refusal prints (`option check.tolf
/// declared twice` on a repeated `tolf`; `DynareBison.yy` maps each `o_` rule).
/// `heterogeneity_solve` and `heterogeneity_simulate` options map to their
/// written names.
fn hetero_option_path(name: &str) -> String {
    match name.to_ascii_lowercase().as_str() {
        "filename" => "steady_state_file_name".to_string(),
        "variable" => "steady_state_variable_name".to_string(),
        "tolf" => "check.tolf".to_string(),
        "forward_max_iter" => "forward.max_iter".to_string(),
        "forward_tol" => "forward.tol".to_string(),
        "forward_check_every" => "forward.check_every".to_string(),
        "forward_verbosity" => "forward.verbosity".to_string(),
        "time_iteration_max_iter" => "time_iteration.max_iter".to_string(),
        "time_iteration_tol" => "time_iteration.tol".to_string(),
        "time_iteration_learning_rate" => "time_iteration.learning_rate".to_string(),
        "time_iteration_verbosity" => "time_iteration.verbosity".to_string(),
        "time_iteration_solver_tolf" => "time_iteration.solver_tolf".to_string(),
        "time_iteration_solver_tolx" => "time_iteration.solver_tolx".to_string(),
        "time_iteration_solver_factor" => "time_iteration.solver_factor".to_string(),
        "time_iteration_solver_max_iter" => "time_iteration.solver_max_iter".to_string(),
        "time_iteration_solver_stop_on_error" => "time_iteration.solver_stop_on_error".to_string(),
        "time_iteration_early_stopping" => "time_iteration.early_stopping".to_string(),
        "calibration_tolf" => "calibration.tolf".to_string(),
        "calibration_max_iter" => "calibration.max_iter".to_string(),
        "calibration_verbosity" => "calibration.verbosity".to_string(),
        "calibration_target_equations" => "calibration.target_equations".to_string(),
        "tex" => "TeX".to_string(),
        "irf_plot_threshold" => "impulse_responses.plot_threshold".to_string(),
        "print" | "noprint" => "noprint".to_string(),
        other => other.to_string(),
    }
}

fn date_option_consumer(command: &str, option: &str) -> bool {
    let name = option.to_ascii_lowercase();
    let first_last_obs = matches!(name.as_str(), "first_obs" | "last_obs");
    let simulation_bound = matches!(
        name.as_str(),
        "first_simulation_period" | "last_simulation_period"
    );
    let plot_date = matches!(name.as_str(), "plot_init_date" | "plot_end_date");
    if command.eq_ignore_ascii_case("data") {
        first_last_obs
    } else if command.eq_ignore_ascii_case("histval_file") {
        first_last_obs || name == "first_simulation_period"
    } else if command.eq_ignore_ascii_case("initval_file") {
        first_last_obs || simulation_bound
    } else if command.eq_ignore_ascii_case("perfect_foresight_setup")
        || command.eq_ignore_ascii_case("perfect_foresight_with_expectation_errors_setup")
    {
        simulation_bound
    } else if command.eq_ignore_ascii_case("plot_shock_decomposition")
        || command.eq_ignore_ascii_case("initial_condition_decomposition")
    {
        plot_date
    } else {
        false
    }
}

/// Flex returns these 7.2 option words as tokens in a trailing symbol list.
/// The 03b handoff found them being misreported as undeclared names.
fn reserved_trailing_option_error(word: &str) -> Option<&'static str> {
    Some(match word.to_ascii_lowercase().as_str() {
        "nograph" => "syntax error, unexpected NOGRAPH",
        "conf_sig" => "syntax error, unexpected CONF_SIG",
        "periods" => "syntax error, unexpected PERIODS",
        "datafile" => "syntax error, unexpected DATAFILE",
        "type" => "syntax error, unexpected TYPE",
        "detail_plot" => "syntax error, unexpected DETAIL_PLOT",
        "colormap" => "syntax error, unexpected COLORMAP",
        "with_epilogue" => "syntax error, unexpected WITH_EPILOGUE",
        "parameter_set" => "syntax error, unexpected PARAMETER_SET",
        "graph_format" => "syntax error, unexpected GRAPH_FORMAT",
        "nodisplay" => "syntax error, unexpected NODISPLAY",
        "fig_name" => "syntax error, unexpected FIG_NAME",
        "first_obs" => "syntax error, unexpected FIRST_OBS",
        "last_obs" => "syntax error, unexpected LAST_OBS",
        "init_state" => "syntax error, unexpected INIT_STATE",
        "nobs" => "syntax error, unexpected NOBS",
        _ => return None,
    })
}

/// The three words the pin's grammar keys a dotted statement's body on.
fn is_dotted_body_word(word: &str) -> bool {
    word.eq_ignore_ascii_case("prior")
        || word.eq_ignore_ascii_case("options")
        || word.eq_ignore_ascii_case("subsamples")
}

/// One family `;` statement whose grammar production requires its `(…)` list.
/// `plot_conditional_forecast` requires the trailing symbol list instead.
fn requires_option_list(cmd: &str) -> bool {
    cmd.eq_ignore_ascii_case("data")
        || cmd.eq_ignore_ascii_case("markov_switching")
        || cmd.eq_ignore_ascii_case("svar")
        || cmd.eq_ignore_ascii_case("conditional_forecast")
}

fn requires_symbol_list(cmd: &str) -> bool {
    cmd.eq_ignore_ascii_case("plot_conditional_forecast")
}

fn top_options(tokens: &[Token], src: &str, from: usize, to: usize) -> Vec<TopOption> {
    let mut out = Vec::new();
    let mut i = from;
    let mut depth: i32 = 0;
    let end = to.min(tokens.len());
    while i < end {
        match tokens[i].kind {
            TokenKind::LParen => {
                depth += 1;
                i += 1;
            }
            TokenKind::RParen => {
                depth = depth.saturating_sub(1);
                i += 1;
            }
            TokenKind::Ident if depth == 1 => {
                let ident = tokens[i].text(src).to_string();
                let span = tokens[i].span;
                i += 1;
                let eq = i < end && tokens[i].kind == TokenKind::Eq;
                let mut value_lex = String::new();
                let mut value_span = span;
                if eq {
                    i += 1;
                    if i < end {
                        match tokens[i].kind {
                            TokenKind::Comma | TokenKind::RParen => {}
                            TokenKind::LParen => {
                                value_span = tokens[i].span;
                                let mut d = 1;
                                i += 1;
                                while i < end && d > 0 {
                                    match tokens[i].kind {
                                        TokenKind::LParen => d += 1,
                                        TokenKind::RParen => d -= 1,
                                        _ => {}
                                    }
                                    i += 1;
                                }
                            }
                            _ => {
                                value_lex = tokens[i].text(src).to_string();
                                value_span = tokens[i].span;
                                i += 1;
                            }
                        }
                    }
                }
                out.push(TopOption {
                    ident,
                    span,
                    eq,
                    value_lex,
                    value_span,
                });
            }
            _ => i += 1,
        }
    }
    out
}

const UNARY_BP: u8 = 7;

/// Static copy of Python `_KEYWORD_TYPO_MAP`.
const KEYWORD_TYPO_MAP: &[(&str, &str)] = &[
    ("mdoel", "model"),
    ("modle", "model"),
    ("modl", "model"),
    ("modelo", "model"),
    ("mdel", "model"),
    ("moel", "model"),
    ("modeel", "model"),
    ("moedl", "model"),
    ("mmodel", "model"),
    ("modell", "model"),
    ("paramters", "parameters"),
    ("parametrs", "parameters"),
    ("paramaters", "parameters"),
    ("paremeters", "parameters"),
    ("parametres", "parameters"),
    ("paraemters", "parameters"),
    ("paramteres", "parameters"),
    ("parmaeters", "parameters"),
    ("prameters", "parameters"),
    ("parmeters", "parameters"),
    ("parametes", "parameters"),
    ("parametera", "parameters"),
    ("paramter", "parameters"),
    ("parametr", "parameters"),
    ("variable", "var"),
    ("vars", "var"),
    ("vasr", "var"),
    ("varexoo", "varexo"),
    ("varrexo", "varexo"),
    ("vaarexo", "varexo"),
    ("varxeo", "varexo"),
    ("vaxero", "varexo"),
    ("varexo0", "varexo"),
    ("shokcs", "shocks"),
    ("shcoks", "shocks"),
    ("shokc", "shocks"),
    ("schocks", "shocks"),
    ("shoks", "shocks"),
    ("initvla", "initval"),
    ("inival", "initval"),
    ("intivals", "initval"),
    ("initvall", "initval"),
    ("initavl", "initval"),
    ("staedy", "steady"),
    ("steday", "steady"),
];

const ASSIGN_FOLLOWERS: &[&str] = &[
    "model",
    "model_remove",
    "model_replace",
    "var",
    "var_remove",
    "varexo",
    "varexo_det",
    "parameters",
    "predetermined_variables",
    "initval",
    "endval",
    "shocks",
    "occbin_constraints",
    "steady_state_model",
    "steady",
    "check",
    "resid",
    "stoch_simul",
    "simul",
    "estimation",
    "osr",
    "calib_smoother",
    "forecast",
    "identification",
    "dynasave",
    "dynatype",
    "model_diagnostics",
    "model_info",
    "perfect_foresight_setup",
    "perfect_foresight_solver",
];

const ASSIGN_BLOCK_LIKE: &[&str] = &[
    "var",
    "varexo",
    "varexo_det",
    "parameters",
    "model",
    "predetermined_variables",
    "initval",
    "endval",
    "shocks",
    "occbin_constraints",
    "steady_state_model",
    "end",
    "log",
];

const TERMINAL_COMMANDS: &[&str] = &[
    "stoch_simul",
    "estimation",
    "simul",
    "perfect_foresight_solver",
    "ramsey_policy",
    "discretionary_policy",
    "osr",
    "sensitivity",
    "dynare_sensitivity",
    "send_endogenous_variables_to_workspace",
];

pub(crate) const BLOCK_OPENERS: &[&str] = &[
    "model",
    "initval",
    "endval",
    "histval",
    "shocks",
    "occbin_constraints",
    "steady_state_model",
    "estimated_params_init",
    "estimated_params_bounds",
    "osr_params_bounds",
    "generate_irfs",
    "epilogue",
    "filter_initial_state",
    "optim_weights",
    "ramsey_constraints",
    "init2shocks",
    "homotopy_setup",
    "shock_groups",
    // Every Dynare block keyword. `consume_until_end` / `at_block_opener` stop a
    // body at the next one. The five moment/calibration names are parsed below;
    // the rest still go through `at_skipped_block`.
    "matched_irfs",
    "matched_irfs_weights",
    "matched_moments",
    "moment_calibration",
    "irf_calibration",
    "pac_target_info",
    "priors",
    "deterministic_trends",
    "estimated_params_remove",
    "observation_trends",
    "heteroskedastic_shocks",
    "shock_paths",
    "perfect_foresight_controlled_paths",
];

/// Command words whose lexer rule is `<INITIAL>`. Four of them (`stoch_simul`,
/// `simul`, `forecast`, `identification`) also have a `<DYNARE_STATEMENT>` rule;
/// `varobs` is the fifth such word and is not here, because the declaration scan
/// already ends the list on it. At statement head, `name = …` is the keyword
/// token followed by `=`, which 7.1 refuses. Inside a block the `<INITIAL>`-only
/// spellings can be declared names; that reading is not this list.
const INITIAL_COMMANDS: &[&str] = &[
    "steady",
    "check",
    "resid",
    "stoch_simul",
    "simul",
    "estimation",
    "osr",
    "calib_smoother",
    "forecast",
    "identification",
    "dynasave",
    "dynatype",
    "model_diagnostics",
    "model_info",
    "perfect_foresight_setup",
    "perfect_foresight_solver",
    "estimated_params",
];

const PRIOR_SHAPES: &[&str] = &[
    "beta_pdf",
    "gamma_pdf",
    "normal_pdf",
    "inv_gamma_pdf",
    "inv_gamma1_pdf",
    "inv_gamma2_pdf",
    "uniform_pdf",
    "weibull_pdf",
];

#[derive(Clone, Copy)]
enum EstimatedParamsTarget {
    Params,
    Init,
    Bounds,
}

enum ExprStop {
    EqOrSemi,
    Semi,
}

pub(crate) fn normalize_newlines(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\r' && (i + 1 >= bytes.len() || bytes[i + 1] != b'\n') {
            out.push('\n');
            i += 1;
        } else {
            let ch = text[i..].chars().next().unwrap();
            out.push(ch);
            i += ch.len_utf8();
        }
    }
    out
}

fn collect_include_dirs(
    src: &str,
    tokens: &[Token],
) -> (
    Vec<IncludeDirective>,
    Vec<IncludePathDirective>,
    Vec<MacroDirective>,
    Vec<MacroInterp>,
) {
    let mut includes = Vec::new();
    let mut includepaths = Vec::new();
    let mut macro_directives = Vec::new();
    let mut macro_interps = Vec::new();
    for tok in tokens {
        match tok.kind {
            TokenKind::MacroDir => {
                let text = collapse_continuations(tok.text(src));
                if let Some(filename) = literal_include_filename(&text) {
                    includes.push(IncludeDirective {
                        filename,
                        span: tok.span,
                    });
                } else if let Some(argument) = includepath_argument(&text) {
                    includepaths.push(IncludePathDirective {
                        argument,
                        span: tok.span,
                    });
                }
                if let Some((kind, argument)) = parse_dir_kind_arg(&text) {
                    if kind != "include" {
                        macro_directives.push(MacroDirective {
                            kind,
                            argument,
                            span: tok.span,
                        });
                    }
                }
            }
            TokenKind::MacroInterp => {
                let text = tok.text(src);
                macro_interps.push(MacroInterp {
                    inner: interp_inner(text),
                    span: tok.span,
                });
            }
            _ => {}
        }
    }
    (includes, includepaths, macro_directives, macro_interps)
}

fn parse_dir_kind_arg(text: &str) -> Option<(String, Option<String>)> {
    let rest = text.trim_start().strip_prefix("@#")?;
    let rest = rest.trim_start();
    let ident = leading_ident(rest)?;
    let kind = ident.to_ascii_lowercase();
    let arg = rest[ident.len()..].trim();
    let argument = if arg.is_empty() {
        None
    } else {
        Some(arg.to_string())
    };
    Some((kind, argument))
}

fn interp_inner(text: &str) -> String {
    let rest = text.strip_prefix("@{").unwrap_or(text);
    rest.strip_suffix('}').unwrap_or(rest).to_string()
}

fn collapse_continuations(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            let mut j = i + 1;
            while j < bytes.len() && (bytes[j] == b' ' || bytes[j] == b'\t') {
                j += 1;
            }
            if j < bytes.len() && (bytes[j] == b'\n' || bytes[j] == b'\r') {
                let mut k = i;
                while k > 0 && (bytes[k - 1] == b' ' || bytes[k - 1] == b'\t') {
                    k -= 1;
                }
                while out.len() > k {
                    out.pop();
                }
                out.push(' ');
                if bytes[j] == b'\r' && j + 1 < bytes.len() && bytes[j + 1] == b'\n' {
                    j += 2;
                } else {
                    j += 1;
                }
                while j < bytes.len() && (bytes[j] == b' ' || bytes[j] == b'\t') {
                    j += 1;
                }
                i = j;
                continue;
            }
        }
        let ch = text[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

fn directive_arg<'a>(text: &'a str, keyword: &str) -> Option<&'a str> {
    let rest = text.trim_start().strip_prefix("@#")?;
    strip_word_ci(rest.trim_start(), keyword)
}

fn strip_word_ci<'a>(text: &'a str, word: &str) -> Option<&'a str> {
    if text.len() < word.len() || !text[..word.len()].eq_ignore_ascii_case(word) {
        return None;
    }
    let after = &text[word.len()..];
    if after
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        return None;
    }
    Some(after)
}

fn literal_include_filename(text: &str) -> Option<String> {
    let rest = directive_arg(text, "include")?;
    let argument = rest.trim();
    if argument.is_empty() {
        return None;
    }
    if let Some(inner) = strip_quoted(argument) {
        return Some(inner.to_string());
    }
    bare_include_literal(argument)
}

fn includepath_argument(text: &str) -> Option<String> {
    let rest = directive_arg(text, "includepath")?;
    let argument = rest.trim();
    if argument.is_empty() {
        return None;
    }
    Some(argument.to_string())
}

fn strip_quoted(s: &str) -> Option<&str> {
    let s = s.trim();
    let bytes = s.as_bytes();
    if bytes.len() < 2 {
        return None;
    }
    let q = bytes[0];
    if q != b'"' && q != b'\'' {
        return None;
    }
    if bytes[bytes.len() - 1] != q {
        return None;
    }
    let inner = &s[1..s.len() - 1];
    if inner.as_bytes().contains(&q) {
        return None;
    }
    Some(inner)
}

/// Python `_bare_include_literal`: a path, not a bare identifier / expression.
fn bare_include_literal(argument: &str) -> Option<String> {
    let raw = argument.trim();
    if raw.is_empty() {
        return None;
    }
    if raw
        .chars()
        .any(|c| c.is_whitespace() || matches!(c, '"' | '\'' | '[' | ']' | '+'))
    {
        return None;
    }
    if is_ident_only(raw) {
        return None;
    }
    Some(raw.to_string())
}

/// Whether a block opener's `(…)` held this one bare word.
fn is_flag_word(word: &Option<String>, flag: &str) -> bool {
    word.as_deref()
        .is_some_and(|lex| lex.eq_ignore_ascii_case(flag))
}

/// Inner text of a closed `$…$` token. An unclosed `$` is not a TeX name.
fn closed_tex_name(raw: &str) -> Option<String> {
    let inner = raw.strip_prefix('$')?.strip_suffix('$')?;
    Some(inner.to_string())
}

/// Contents of a closed quoted string. The quotes are not part of the metadata.
fn unquoted_string(raw: &str) -> Option<String> {
    let mut chars = raw.chars();
    let open = chars.next()?;
    if open != '\'' && open != '"' {
        return None;
    }
    let mut body: String = chars.collect();
    if !body.ends_with(open) {
        return None;
    }
    body.pop();
    Some(body)
}

fn is_ident_only(s: &str) -> bool {
    let mut chars = s.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_ascii_alphabetic() && first != '_' {
        return false;
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

struct Parser<'a> {
    src: &'a str,
    tokens: Vec<Token>,
    i: usize,
    intern: Interner,
    model: Model,
    eq_token_ranges: Vec<Range<usize>>,
    hetero_eq_token_ranges: Vec<Vec<Range<usize>>>,
    /// Token ranges of `verbatim; ? end;` bodies, whose text 7.1 passes through raw.
    verbatim_ranges: Vec<Range<usize>>,
    /// Bumped once per statement that lists names, so `CommandSymbol::list_id`
    /// groups one statement's list.
    symbol_list_id: u32,
    in_model: bool,
    /// Inside a `model` / `model_replace` body, whose rows are expressions.
    in_equation_body: bool,
}

impl Parser<'_> {
    fn parse_file(&mut self) {
        while !self.at(TokenKind::Eof) {
            // `model = 0.2;` and `steady = 0.9;` are not assignments and not
            // blocks. The `<INITIAL>` rule returns the keyword, and the grammar
            // wants `;` or `(`.
            if self.at_keyword_followed_by_eq() {
                self.record_issue(ParseIssue {
                    kind: ParseIssueKind::UnexpectedEqual,
                    span: self.tokens[self.i + 1].span,
                });
                while !self.at(TokenKind::Semi) && !self.at(TokenKind::Eof) {
                    self.bump();
                }
                self.eat(TokenKind::Semi);
                continue;
            }
            if self.at_ident_ci("var") {
                let decls = self.parse_declaration("var");
                self.model.endogenous.extend(decls);
            } else if self.at_ident_ci("varexo_det") {
                let decls = self.parse_declaration("varexo_det");
                self.model.deterministic_exogenous.extend(decls);
            } else if self.at_ident_ci("varexo") {
                let decls = self.parse_declaration("varexo");
                self.model.exogenous.extend(decls);
            } else if self.at_ident_ci("parameters") {
                let decls = self.parse_declaration("parameters");
                self.model.parameters.extend(decls);
            } else if self.at_ident_ci("model_local_variable") {
                let decls = self.parse_declaration("model_local_variable");
                self.model.model_local_variables.extend(decls);
            } else if self.at_ident_ci("predetermined_variables") {
                let decls = self.parse_declaration("predetermined_variables");
                self.model.predetermined.extend(decls);
            } else if self.at_ident_ci("model") {
                self.in_model = true;
                self.parse_model_block();
                self.in_model = false;
            } else if let Some(kind) = self.at_semi_structural_command() {
                self.parse_semi_structural_command(kind);
            } else if self.at_ident_ci("pac_target_info") {
                self.parse_pac_target_info_block();
            } else if self.at_ident_ci("deterministic_trends") {
                self.parse_deterministic_trends_block();
            } else if self.at_ident_ci("steady_state_model") {
                self.parse_ss_block();
            } else if self.at_ident_ci("initval") {
                self.parse_initval_block();
            } else if self.at_ident_ci("endval") {
                self.parse_endval_block();
            } else if self.at_ident_ci("histval") {
                self.parse_histval_block();
            } else if self.at_ident_ci("shocks") || self.at_ident_ci("mshocks") {
                self.parse_shocks_block(true);
            } else if self.at_ident_ci("heteroskedastic_shocks") {
                self.parse_shocks_block(false);
            } else if self.at_ident_ci("shock_paths") {
                self.parse_path_block(false);
            } else if self.at_ident_ci("perfect_foresight_controlled_paths") {
                self.parse_path_block(true);
            } else if self.at_ident_ci("database") && self.at_statement_boundary() {
                self.parse_database_statement();
            } else if self.at_ident_ci("set_time") && self.at_statement_boundary() {
                self.parse_set_time_statement();
            } else if self.at_ident_ci("occbin_constraints") {
                self.parse_occbin_constraints_block();
            } else if self.at_ident_ci("varobs") {
                self.parse_varobs();
            } else if self.at_ident_ci("varexobs") {
                self.parse_varexobs();
            } else if self.at_ident_ci("estimated_params_init") {
                self.parse_estimated_params_init_block();
            } else if self.at_ident_ci("estimated_params_bounds") {
                self.parse_estimated_params_bounds_block();
            } else if self.at_ident_ci("estimated_params") {
                self.parse_estimated_params_block();
            } else if self.at_ident_ci("observation_trends") {
                self.parse_observation_trends_block();
            } else if self.at_ident_ci("planner_objective") {
                self.parse_planner_objective();
            } else if self.at_ident_ci("osr_params_bounds") {
                self.parse_osr_params_bounds();
            } else if self.at_ident_ci("osr_params") {
                self.parse_osr_params();
            } else if self.at_ident_ci("generate_irfs") {
                self.parse_generate_irfs_block();
            } else if self.at_ident_ci("log_trend_var") {
                self.parse_trend_declaration(true);
            } else if self.at_ident_ci("trend_var") {
                self.parse_trend_declaration(false);
            } else if self.at_ident_ci("load_params_and_steady_state") {
                self.parse_load_params();
            } else if self.at_ident_ci("filter_initial_state") {
                self.parse_filter_initial_state_block();
            } else if self.at_ident_ci("external_function") {
                self.parse_external_function();
            } else if self.at_ident_ci("init2shocks") {
                self.parse_init2shocks_block();
            } else if self.at_ident_ci("homotopy_setup") {
                self.parse_homotopy_setup_block();
            } else if self.at_ident_ci("shock_groups") {
                self.parse_shock_groups_block();
            } else if self.at_ident_ci("moment_calibration") && self.at_command_shape(1) {
                self.parse_moment_calibration_block();
            } else if self.at_ident_ci("irf_calibration") && self.at_command_shape(1) {
                self.parse_irf_calibration_block();
            } else if self.at_ident_ci("matched_moments") && self.at_command_shape(1) {
                self.parse_matched_moments_block();
            } else if self.at_ident_ci("matched_irfs_weights") && self.at_command_shape(1) {
                self.parse_matched_irfs_weights_block();
            } else if self.at_ident_ci("matched_irfs") && self.at_command_shape(1) {
                self.parse_matched_irfs_block();
            } else if self.at_ident_ci("method_of_moments") && self.at_statement_boundary() {
                self.parse_mom_statement();
            } else if self.at_ident_ci("bvar_density")
                || self.at_ident_ci("bvar_forecast")
                || self.at_ident_ci("bvar_irf")
            {
                self.parse_bvar_statement();
            } else if self.at_ident_ci("change_type") {
                self.parse_change_type();
            } else if self.at_ident_ci("epilogue") {
                self.parse_epilogue_block();
            } else if self.at_ident_ci("optim_weights") {
                self.model.has_optim_weights = true;
                self.parse_optim_weights_block();
            } else if self.at_ident_ci("ramsey_constraints") {
                self.parse_ramsey_constraints_block();
            } else if self.at_ident_ci("model_remove") {
                self.parse_equation_surgery(false);
            } else if self.at_ident_ci("model_replace") {
                self.parse_equation_surgery(true);
            } else if self.at_ident_ci("var_remove") && self.peek_kind(1) == Some(TokenKind::Ident)
            {
                self.parse_var_remove_statement();
            } else if self.at_ms_family_command().is_some() && self.at_statement_boundary() {
                self.parse_ms_statement();
            } else if self.at_ident_ci("svar_identification")
                && self.at_command_shape(1)
                && self.at_statement_boundary()
            {
                self.parse_svar_identification_block();
            } else if self.at_ident_ci("conditional_forecast_paths")
                && self.at_command_shape(1)
                && self.at_statement_boundary()
            {
                self.parse_conditional_forecast_paths_block();
            } else if self.at_ident_ci("data")
                && self.at_command_shape(1)
                && self.at_statement_boundary()
            {
                self.parse_data_statement();
            } else if self.at_ident_ci("heterogeneity_dimension") {
                self.parse_heterogeneity_dimension();
            } else if self.at_heterogeneity_command() {
                self.parse_heterogeneity_command();
            } else if self.at_dotted_statement().is_some() && self.at_statement_boundary() {
                self.parse_dotted_statement();
            } else if self.at_handed_over_statement() && self.at_statement_boundary() {
                self.parse_handed_over_statement();
            } else if let Some(end) = self.native_statement_end() {
                self.skip_native_statement(end);
            } else if let Some(command) = self.at_policy_command() {
                self.parse_policy_command(command);
            } else if self.at_skipped_block() {
                self.record_skipped_block_opener();
                self.skip_block();
            } else if self.at_ident("end") {
                self.bump();
                self.eat(TokenKind::Semi);
            } else if self.at(TokenKind::Ident) && self.peek_kind(1) == Some(TokenKind::Eq) {
                self.parse_top_assignment();
            } else {
                self.skip_until_semi();
            }
        }
    }

    fn parse_declaration(&mut self, keyword: &str) -> Vec<Decl> {
        let start = self.current_start();
        let keyword_is_var = self.at_ident_ci("var");
        let keyword_is_varexo = self.at_ident_ci("varexo");
        let keyword_is_parameters = self.at_ident_ci("parameters");
        self.bump();
        let mut log_transform = false;
        let mut log_deflator = false;
        let mut deflator = None;
        let mut heterogeneity = None;
        if self.at(TokenKind::LParen) {
            let (log, is_log_deflator, expr, het) = self.parse_declaration_options();
            log_transform = keyword_is_var && log;
            log_deflator = is_log_deflator;
            deflator = expr;
            // `heterogeneity=` is grammatical on these three kinds only
            // (7.2 `DynareBison.yy`); the other declarations take no option list.
            if keyword_is_var || keyword_is_varexo || keyword_is_parameters {
                heterogeneity = het;
            }
        }
        let kw_range_end = self.current_start();
        let mut decls: Vec<Decl> = Vec::new();
        let mut recorded_missing = false;
        while !self.at(TokenKind::Eof) && !self.at(TokenKind::Semi) {
            if self.at(TokenKind::Latex) {
                let tok = self.bump();
                if let Some(tex) = closed_tex_name(self.lexeme(&tok)) {
                    if let Some(decl) = decls.last_mut() {
                        decl.tex_name = Some(tex);
                    }
                }
                continue;
            }
            if self.at(TokenKind::LParen) {
                let long_name = self.take_symbol_long_name();
                if let Some(text) = long_name {
                    if let Some(decl) = decls.last_mut() {
                        decl.long_name = Some(text);
                    }
                }
                continue;
            }
            if self.at(TokenKind::Comma) {
                self.bump();
                continue;
            }
            if self.at(TokenKind::Ident) {
                if self.at_decl_or_block_keyword() {
                    let next = self.tokens[self.i].span;
                    self.record_issue(ParseIssue {
                        kind: ParseIssueKind::MissingDeclSemi {
                            keyword: keyword.to_string(),
                            next_is_assign: false,
                            next_span: Some(next),
                        },
                        span: Span {
                            start,
                            end: kw_range_end,
                        },
                    });
                    recorded_missing = true;
                    break;
                }
                if self.peek_kind(1) == Some(TokenKind::Eq) {
                    let next = self.tokens[self.i + 1].span;
                    self.record_issue(ParseIssue {
                        kind: ParseIssueKind::MissingDeclSemi {
                            keyword: keyword.to_string(),
                            next_is_assign: true,
                            next_span: Some(next),
                        },
                        span: Span {
                            start,
                            end: kw_range_end,
                        },
                    });
                    recorded_missing = true;
                    break;
                }
                let tok = self.bump();
                let name = self.lexeme(&tok).to_string();
                if matches!(name.as_str(), "long_name" | "latex_name" | "long") {
                    continue;
                }
                let id = self.intern.intern(&name);
                decls.push(Decl {
                    name: id,
                    span: tok.span,
                    long_name: None,
                    tex_name: None,
                    log_transform,
                    heterogeneity,
                });
                continue;
            }
            self.bump();
        }
        if !recorded_missing && !self.at(TokenKind::Semi) {
            self.record_issue(ParseIssue {
                kind: ParseIssueKind::MissingDeclSemi {
                    keyword: keyword.to_string(),
                    next_is_assign: false,
                    next_span: None,
                },
                span: Span {
                    start,
                    end: kw_range_end,
                },
            });
        }
        self.eat(TokenKind::Semi);
        if keyword_is_parameters {
            for decl in &decls {
                if self.intern.get(decl.name) == "dsge_prior_weight"
                    && self.model.dsge_prior_weight_param.is_none()
                {
                    self.model.dsge_prior_weight_param = Some(decl.span);
                }
            }
        }
        if keyword_is_var && (log_deflator || deflator.is_some()) {
            for decl in &decls {
                self.model.nonstationary_vars.push(NonstationaryVar {
                    name: decl.name,
                    span: decl.span,
                    log_deflator,
                    log_option: log_transform,
                    deflator,
                });
            }
        }
        decls
    }

    /// Per-name partition `(long_name='…')`. Dynare stores that string on the
    /// symbol. Other keys in the same parentheses are left unread. `None` when
    /// the group has no `long_name` key. An empty quoted value is `Some("")`.
    fn take_symbol_long_name(&mut self) -> Option<String> {
        self.bump();
        let mut found: Option<String> = None;
        let mut depth = 1i32;
        while depth > 0 && !self.at(TokenKind::Eof) {
            if self.at(TokenKind::LParen) {
                depth += 1;
                self.bump();
                continue;
            }
            if self.at(TokenKind::RParen) {
                depth -= 1;
                self.bump();
                continue;
            }
            if depth == 1 && self.at(TokenKind::Ident) {
                let tok = self.bump();
                if self.lexeme(&tok) == "long_name" && self.at(TokenKind::Eq) {
                    self.bump();
                    if self.at(TokenKind::String) {
                        let value = self.bump();
                        found = Some(unquote_string(self.lexeme(&value)));
                    }
                }
                continue;
            }
            self.bump();
        }
        found
    }

    /// `var(?)` option list: `log`, `deflator=`, `log_deflator=`, other `=value`s
    /// skipped, and 7.2's `heterogeneity=<symbol>`. Once `heterogeneity` opens the
    /// list the grammar takes no second option, so the pinned parser stops on the
    /// next token with `syntax error, unexpected COMMA, expecting ')'` — recorded
    /// on that token. A `heterogeneity` that is not the list's first option keeps
    /// today's lenient reading.
    fn parse_declaration_options(&mut self) -> (bool, bool, Option<ExprId>, Option<(Name, Span)>) {
        let mut log = false;
        let mut log_deflator = false;
        let mut deflator = None;
        let mut saw_deflator = false;
        let mut heterogeneity = None;
        let mut first_option = true;
        self.bump();
        while !self.at(TokenKind::Eof) && !self.at(TokenKind::RParen) {
            if self.at(TokenKind::Ident) {
                let lex = self.lexeme(&self.tokens[self.i]).to_string();
                if lex.eq_ignore_ascii_case("heterogeneity") {
                    if !first_option {
                        self.push_bison(
                            self.tokens[self.i].span,
                            "syntax error, unexpected HETEROGENEITY, expecting DEFLATOR"
                                .to_string(),
                        );
                        self.bump();
                        continue;
                    }
                    first_option = false;
                    self.bump();
                    if self.at(TokenKind::Eq) {
                        self.bump();
                        if self.at(TokenKind::Ident) {
                            let tok = self.bump();
                            let name = self.lexeme(&tok).to_string();
                            heterogeneity = Some((self.intern.intern(&name), tok.span));
                            if !self.at(TokenKind::RParen) && !self.at(TokenKind::Eof) {
                                self.hetero_bison_refuse(self.i, Some("')'"));
                            }
                        }
                    } else {
                        self.hetero_bison_refuse(self.i, Some("EQUAL"));
                    }
                    continue;
                }
                first_option = false;
                if lex.eq_ignore_ascii_case("log") {
                    log = true;
                    self.bump();
                    continue;
                }
                if lex.eq_ignore_ascii_case("deflator") || lex.eq_ignore_ascii_case("log_deflator")
                {
                    let is_log = lex.eq_ignore_ascii_case("log_deflator");
                    self.bump();
                    if self.at(TokenKind::Eq) {
                        self.bump();
                        let expr = self.parse_expr();
                        if !saw_deflator {
                            saw_deflator = true;
                            deflator = expr;
                            log_deflator = is_log;
                        }
                    }
                    continue;
                }
                self.bump();
                if self.at(TokenKind::Eq) {
                    self.bump();
                    self.skip_option_value();
                }
                continue;
            }
            if self.at(TokenKind::LParen) {
                self.skip_balanced(TokenKind::LParen, TokenKind::RParen);
                continue;
            }
            self.bump();
        }
        self.eat(TokenKind::RParen);
        (log, log_deflator, deflator, heterogeneity)
    }

    fn skip_option_value(&mut self) {
        if self.at(TokenKind::LParen) {
            self.skip_balanced(TokenKind::LParen, TokenKind::RParen);
        } else if !self.at(TokenKind::Comma)
            && !self.at(TokenKind::RParen)
            && !self.at(TokenKind::Semi)
            && !self.at(TokenKind::Eof)
        {
            self.bump();
        }
    }

    fn parse_model_block(&mut self) {
        let start = self.bump().span.start;
        if self.at(TokenKind::LParen) {
            if let Some((dimension, dimension_span)) = self.heterogeneous_model_dimension() {
                self.parse_heterogeneous_model_body(start, dimension, dimension_span);
                return;
            }
        }
        let mut linear = false;
        if self.at(TokenKind::LParen) {
            let from = self.i;
            let opt = self.skip_balanced(TokenKind::LParen, TokenKind::RParen);
            self.record_deprecated_options_in_range(from, self.i);
            self.record_model_option_flags(from, self.i);
            self.record_option_twice(from, self.i);
            linear = self.src[opt.start as usize..opt.end as usize]
                .to_ascii_lowercase()
                .contains("linear");
        }
        let opener_end = if self.at(TokenKind::Semi) {
            self.bump().span.end
        } else {
            self.current_start()
        };
        let opener_span = Span {
            start,
            end: opener_end,
        };
        if linear {
            self.model.is_linear = true;
        }
        let body_i = self.i;
        self.in_equation_body = true;
        while !self.at(TokenKind::Eof) && !self.at_block_stop() {
            if let Some((eq, range)) = self.parse_equation_statement() {
                self.eq_token_ranges.push(range);
                self.model.equations.push(eq);
            }
        }
        self.in_equation_body = false;
        if self.at_block_end() {
            self.record_missing_final("model", body_i, self.i);
        }
        let end = self.finish_block_named("model", opener_span, body_i);
        self.model.model_block = Some(Span { start, end });
    }

    /// `(dimension, span)` when the `model(…)` option list opens a heterogeneous
    /// body: exactly `heterogeneity=<symbol>`, or that shape followed by tokens
    /// the grammar refuses — the pinned parser stops on the offending token with
    /// `syntax error, unexpected COMMA, expecting ')'`. Any other list keeps the
    /// aggregate reading.
    fn heterogeneous_model_dimension(&mut self) -> Option<(Name, Span)> {
        let first = self.tokens.get(self.i + 1)?;
        if first.kind != TokenKind::Ident
            || !self.lexeme(first).eq_ignore_ascii_case("heterogeneity")
        {
            return None;
        }
        if self.peek_kind(2) != Some(TokenKind::Eq) {
            // `model(heterogeneity)` with no value: the grammar wants EQUAL, and
            // the pinned parser stops on the next token (the closing paren).
            if self.tokens.get(self.i + 2).is_some() {
                self.hetero_bison_refuse(self.i + 2, Some("EQUAL"));
            }
            return None;
        }
        let value_index = self.i + 3;
        let (dimension_span, dimension_lex) = {
            let value = self.tokens.get(value_index)?;
            if value.kind != TokenKind::Ident {
                return None;
            }
            (value.span, self.lexeme(value).to_string())
        };
        match self.tokens.get(value_index + 1).map(|t| t.kind) {
            None | Some(TokenKind::RParen) => {}
            _ => self.hetero_bison_refuse(value_index + 1, Some("')'")),
        }
        let dimension = self.intern.intern(&dimension_lex);
        Some((dimension, dimension_span))
    }

    /// The body of `model(heterogeneity=d); … end;`. Equations go into the
    /// per-dimension record; the aggregate list, `model_block`, and `is_linear`
    /// are untouched. An empty body is the pinned `unexpected END` sentence.
    fn parse_heterogeneous_model_body(
        &mut self,
        start: u32,
        dimension: Name,
        dimension_span: Span,
    ) {
        if self.at(TokenKind::LParen) {
            self.skip_balanced(TokenKind::LParen, TokenKind::RParen);
        }
        let opener_end = if self.at(TokenKind::Semi) {
            self.bump().span.end
        } else {
            self.current_start()
        };
        let opener_span = Span {
            start,
            end: opener_end,
        };
        let body_i = self.i;
        self.in_equation_body = true;
        let mut equations = Vec::new();
        let mut ranges = Vec::new();
        while !self.at(TokenKind::Eof) && !self.at_block_stop() {
            if let Some((eq, range)) = self.parse_equation_statement() {
                ranges.push(range);
                equations.push(eq);
            }
        }
        self.in_equation_body = false;
        if equations.is_empty() && self.at_block_end() {
            self.hetero_bison_refuse(self.i, None);
        }
        if self.at_block_end() {
            self.record_missing_final("model", body_i, self.i);
        }
        let end = self.finish_block_named("model", opener_span, body_i);
        self.model
            .heterogeneous_models
            .push(HeterogeneousModelBlock {
                dimension,
                dimension_span,
                span: Span { start, end },
                equations,
            });
        self.hetero_eq_token_ranges.push(ranges);
    }

    /// `heterogeneity_dimension name1, name2;` — one record per name, file order.
    fn parse_heterogeneity_dimension(&mut self) {
        let start = self.bump().span.start;
        let mut names: Vec<(Name, Span)> = Vec::new();
        while !self.at(TokenKind::Semi) && !self.at(TokenKind::Eof) {
            if self.at(TokenKind::Ident) {
                let tok = self.bump();
                let name = self.lexeme(&tok).to_string();
                names.push((self.intern.intern(&name), tok.span));
            } else if self.at(TokenKind::Number) {
                self.hetero_bison_refuse(self.i, None);
                self.bump();
            } else {
                self.bump();
            }
        }
        let end = if self.at(TokenKind::Semi) {
            self.bump().span.end
        } else {
            self.current_start()
        };
        self.eat(TokenKind::Semi);
        let span = Span { start, end };
        for (name, name_span) in names {
            self.model
                .heterogeneity_dimensions
                .push(HeterogeneityDimension {
                    name,
                    name_span,
                    span,
                });
        }
    }

    fn at_heterogeneity_command(&self) -> bool {
        self.at_ident_ci("heterogeneity_load_steady_state")
            || self.at_ident_ci("heterogeneity_compute_steady_state")
            || self.at_ident_ci("heterogeneity_solve")
            || self.at_ident_ci("heterogeneity_simulate")
    }

    /// One `heterogeneity_*` statement: written options with spans (the twice
    /// refusal keys on 7.2's internal option name), and `heterogeneity_simulate`'s
    /// trailing symbol list. A malformed list recovers to the next `;`.
    fn parse_heterogeneity_command(&mut self) {
        let opener = self.bump();
        let start = opener.span.start;
        let command = self.lexeme(&opener).to_string();
        let kind = if command.eq_ignore_ascii_case("heterogeneity_load_steady_state") {
            HeterogeneityCommandKind::LoadSteadyState
        } else if command.eq_ignore_ascii_case("heterogeneity_compute_steady_state") {
            HeterogeneityCommandKind::ComputeSteadyState
        } else if command.eq_ignore_ascii_case("heterogeneity_solve") {
            HeterogeneityCommandKind::Solve
        } else {
            HeterogeneityCommandKind::Simulate
        };
        let mut options = Vec::new();
        let mut seen: HashMap<String, ()> = HashMap::new();
        if self.at(TokenKind::LParen) {
            self.bump();
            if self.at(TokenKind::RParen)
                && matches!(
                    kind,
                    HeterogeneityCommandKind::Solve | HeterogeneityCommandKind::Simulate
                )
            {
                let expected = if kind == HeterogeneityCommandKind::Solve {
                    Some("TRUNCATION_HORIZON")
                } else {
                    None
                };
                self.hetero_bison_refuse(self.i, expected);
            }
            while !self.at(TokenKind::RParen)
                && !self.at(TokenKind::Semi)
                && !self.at(TokenKind::Eof)
            {
                if !self.at(TokenKind::Ident) {
                    self.bump();
                    continue;
                }
                let name_tok = self.bump();
                let name = self.lexeme(&name_tok).to_string();
                if kind != HeterogeneityCommandKind::Simulate && name.eq_ignore_ascii_case("print")
                {
                    self.push_bison(
                        name_tok.span,
                        "syntax error, unexpected PRINT, expecting FILENAME or TOLF or VARIABLE"
                            .to_string(),
                    );
                }
                let mut value = None;
                if self.at(TokenKind::Eq) {
                    self.bump();
                    if name.eq_ignore_ascii_case("tolf") && self.at(TokenKind::Minus) {
                        self.hetero_bison_refuse(self.i, Some("FLOAT_NUMBER or INT_NUMBER"));
                    }
                    value = self.read_hetero_option_value();
                }
                // `option_num` refuses on the internal name. `print` and
                // `noprint` are both `noprint`, and only `heterogeneity_simulate`
                // accepts that pair; the other commands reject `print` as a token.
                let internal = hetero_option_path(&name);
                let key = if kind == HeterogeneityCommandKind::Simulate {
                    internal.clone()
                } else {
                    name.to_ascii_lowercase()
                };
                if seen.insert(key, ()).is_some() {
                    self.model.option_twice.push((internal, name_tok.span));
                }
                options.push(HeterogeneityOption {
                    name,
                    name_span: name_tok.span,
                    value,
                });
            }
            if self.at(TokenKind::Semi) {
                self.hetero_bison_refuse(self.i, Some("COMMA or ')'"));
            }
            if self.at(TokenKind::RParen) {
                self.bump();
            }
        }
        let mut simulate_names = Vec::new();
        if kind == HeterogeneityCommandKind::Simulate {
            loop {
                match self.tokens.get(self.i).map(|t| t.kind) {
                    Some(TokenKind::Ident) => {
                        let tok = self.bump();
                        let name = self.lexeme(&tok).to_string();
                        simulate_names.push((self.intern.intern(&name), tok.span));
                    }
                    Some(TokenKind::Comma) => {
                        self.bump();
                    }
                    _ => break,
                }
            }
        }
        let end = if self.at(TokenKind::Semi) {
            self.bump().span.end
        } else {
            self.current_start()
        };
        self.eat(TokenKind::Semi);
        self.model
            .heterogeneity_commands
            .push(HeterogeneityCommand {
                kind,
                command,
                span: Span { start, end },
                options,
                simulate_names,
            });
    }

    /// The raw written value of a `heterogeneity_*` option, through the comma,
    /// closing paren, or semicolon at nesting depth zero.
    fn read_hetero_option_value(&mut self) -> Option<(String, Span)> {
        let start = self.current_start();
        let from = self.i;
        let mut depth = 0_i32;
        while !self.at(TokenKind::Eof) {
            match self.tokens[self.i].kind {
                TokenKind::LParen => depth += 1,
                TokenKind::RParen if depth == 0 => break,
                TokenKind::RParen => depth -= 1,
                TokenKind::Comma | TokenKind::Semi if depth == 0 => break,
                _ => {}
            }
            self.bump();
        }
        if self.i == from {
            return None;
        }
        let end = self.tokens[self.i - 1].span.end;
        let text = self.src[start as usize..end as usize].trim().to_string();
        Some((text, Span { start, end }))
    }

    /// The pinned Bison sentence on the token at `index`, as a parse-stage E001
    /// with the official text verbatim (P-pac's mechanism).
    fn hetero_bison_refuse(&mut self, index: usize, expected: Option<&str>) {
        let Some(tok) = self.tokens.get(index) else {
            return;
        };
        let unexpected = self.bison_token_name(index);
        let message = match expected {
            Some(expected) => {
                format!("syntax error, unexpected {unexpected}, expecting {expected}")
            }
            None => format!("syntax error, unexpected {unexpected}"),
        };
        self.push_bison(tok.span, message);
    }

    fn push_bison(&mut self, span: Span, message: String) {
        self.model.parse_issues.push(ParseIssue {
            kind: ParseIssueKind::BisonSyntax(message),
            span,
        });
    }

    /// `model_remove(TAGS);` and `model_replace(TAGS); BODY end;`. 7.1 removes the
    /// matching equations during parse, so the model object is post-removal everywhere.
    fn parse_equation_surgery(&mut self, replace: bool) {
        let keyword = if replace {
            "model_replace"
        } else {
            "model_remove"
        };
        let start = self.bump().span.start;
        let mut tag_sets: Vec<Vec<(String, String)>> = Vec::new();
        let mut tag_twice: Vec<(String, Span)> = Vec::new();
        let mut saw_tag = false;
        if self.at(TokenKind::LParen) {
            self.bump();
            self.parse_surgery_tag_selection(keyword, &mut tag_sets, &mut tag_twice, &mut saw_tag);
            self.eat(TokenKind::RParen);
        }
        let opener_end = if self.at(TokenKind::Semi) {
            self.bump().span.end
        } else {
            self.current_start()
        };
        let span = Span {
            start,
            end: opener_end,
        };
        if !saw_tag {
            self.record_issue(ParseIssue {
                kind: ParseIssueKind::MissingSurgeryTag {
                    keyword: keyword.to_string(),
                },
                span,
            });
        }
        let (removed, unmatched) = self.take_surgery_equations(&tag_sets);
        if !replace {
            self.apply_excluded_type_change(&removed, span);
        }
        self.model.equation_surgery.push(EquationSurgery {
            span,
            replace,
            tag_sets,
            unmatched,
            tag_twice,
            removed,
        });
        if !replace {
            return;
        }
        let body_i = self.i;
        let mut n_equations = 0usize;
        self.in_equation_body = true;
        while !self.at(TokenKind::Eof) && !self.at_block_stop() {
            if let Some((eq, range)) = self.parse_equation_statement() {
                self.eq_token_ranges.push(range);
                self.model.equations.push(eq);
                n_equations += 1;
            }
        }
        self.in_equation_body = false;
        if n_equations == 0 {
            let issue_span = self.tokens.get(self.i).map(|tok| tok.span).unwrap_or(span);
            self.record_issue(ParseIssue {
                kind: ParseIssueKind::EmptyReplaceBody,
                span: issue_span,
            });
        }
        if self.at_block_end() {
            self.record_missing_final("model_replace", body_i, self.i);
        }
        self.finish_block_named("model_replace", span, body_i);
    }

    fn parse_var_remove_statement(&mut self) {
        let start = self.bump().span.start;
        let mut names = Vec::new();
        while !self.at(TokenKind::Semi) && !self.at(TokenKind::Eof) {
            if self.at(TokenKind::Comma) {
                self.bump();
                continue;
            }
            if self.at(TokenKind::Ident) {
                let tok = self.bump();
                let spelling = self.lexeme(&tok).to_string();
                names.push((self.intern.intern(&spelling), tok.span));
                continue;
            }
            let tok = self.bump();
            self.model.shape_refuses.push(ShapeRefuse::new(
                tok.span,
                "var_remove",
                "a list of symbols",
            ));
        }
        let end = if self.at(TokenKind::Semi) {
            self.bump().span.end
        } else {
            self.current_start()
        };
        let statement = Span { start, end };
        self.model
            .var_removed
            .extend(names.into_iter().map(|(name, name_span)| VarRemovedName {
                name,
                name_span,
                statement,
            }));
    }

    /// A surgery tag list: `'value'`, `key='value'`, or a bracketed pair list. Each
    /// comma-separated element is one conjunctive set.
    fn parse_surgery_tag_selection(
        &mut self,
        keyword: &str,
        sets: &mut Vec<Vec<(String, String)>>,
        twice: &mut Vec<(String, Span)>,
        saw_tag: &mut bool,
    ) {
        loop {
            if self.at(TokenKind::String) {
                *saw_tag = true;
                sets.push(vec![("name".to_string(), self.surgery_tag_string())]);
            } else if self.at(TokenKind::LBrack) {
                *saw_tag = true;
                let open = self.bump().span;
                let mut pairs: Vec<(String, String)> = Vec::new();
                let mut refused = false;
                while !self.at(TokenKind::RBrack) && !self.at(TokenKind::Eof) {
                    if self.at(TokenKind::Ident) {
                        refused |= !self.parse_surgery_tag_pair(keyword, &mut pairs, twice);
                    } else {
                        self.bump();
                    }
                    if self.at(TokenKind::Comma) {
                        self.bump();
                    }
                }
                self.eat(TokenKind::RBrack);
                if refused {
                    // The set names a tag 7.1 refuses; it must not select equations.
                } else if pairs.is_empty() {
                    self.record_issue(ParseIssue {
                        kind: ParseIssueKind::MissingSurgeryTag {
                            keyword: keyword.to_string(),
                        },
                        span: Span {
                            start: open.start,
                            end: self.current_start(),
                        },
                    });
                } else {
                    sets.push(pairs);
                }
            } else if self.at(TokenKind::Ident) {
                *saw_tag = true;
                let mut pairs: Vec<(String, String)> = Vec::new();
                let mut no_twice = Vec::new();
                if self.parse_surgery_tag_pair(keyword, &mut pairs, &mut no_twice) {
                    sets.push(pairs);
                }
            } else {
                break;
            }
            if self.at(TokenKind::Comma) {
                self.bump();
            } else {
                break;
            }
        }
    }

    /// One `key` / `key='value'` pair of a surgery tag list. 7.1 lowercases the key and
    /// wants the value in single quotes; an unquoted value is refused, so `false` means
    /// the pair ? and the tag set it belongs to ? is dropped.
    fn parse_surgery_tag_pair(
        &mut self,
        keyword: &str,
        pairs: &mut Vec<(String, String)>,
        twice: &mut Vec<(String, Span)>,
    ) -> bool {
        let tok = self.bump();
        let key = self.lexeme(&tok).to_ascii_lowercase();
        let mut value = String::new();
        if self.at(TokenKind::Eq) {
            self.bump();
            if self.at(TokenKind::String) {
                value = self.surgery_tag_string();
            } else if self.at(TokenKind::Ident) || self.at(TokenKind::Number) {
                let v = self.bump();
                self.record_issue(ParseIssue {
                    kind: ParseIssueKind::SurgeryTagUnquoted {
                        keyword: keyword.to_string(),
                    },
                    span: v.span,
                });
                return false;
            }
        }
        if pairs.iter().any(|(seen, _)| seen == &key) {
            twice.push((key.clone(), tok.span));
        }
        pairs.push((key, value));
        true
    }

    /// A quoted tag value.
    fn surgery_tag_string(&mut self) -> String {
        let tok = self.bump();
        unquote_string(self.lexeme(&tok))
    }

    /// Remove every equation the tag sets match. Returns what went (with the position
    /// each held before this statement) and the sets that matched nothing.
    fn take_surgery_equations(
        &mut self,
        tag_sets: &[Vec<(String, String)>],
    ) -> (Vec<RemovedEquation>, Vec<Vec<(String, String)>>) {
        let mut drop = vec![false; self.model.equations.len()];
        let mut unmatched = Vec::new();
        for set in tag_sets {
            let mut hit = false;
            for (i, eq) in self.model.equations.iter().enumerate() {
                if set
                    .iter()
                    .all(|(key, value)| eq.tag_map.get(key).is_some_and(|got| got == value))
                {
                    drop[i] = true;
                    hit = true;
                }
            }
            if !hit {
                unmatched.push(set.clone());
            }
        }
        let mut removed = Vec::new();
        for (i, eq) in self.model.equations.iter().enumerate() {
            if drop[i] {
                removed.push(RemovedEquation {
                    number: i + 1,
                    endogenous: self.equation_named_endogenous(eq),
                    equation: eq.clone(),
                });
            }
        }
        if !removed.is_empty() {
            let before = self.model.equations.len();
            let mut i = 0usize;
            self.model.equations.retain(|_| {
                let keep = !drop[i];
                i += 1;
                keep
            });
            if self.eq_token_ranges.len() == before {
                let mut i = 0usize;
                self.eq_token_ranges.retain(|_| {
                    let keep = !drop[i];
                    i += 1;
                    keep
                });
            }
        }
        (removed, unmatched)
    }

    /// The endogenous a removed equation names: its `endogenous` tag value, or the one
    /// endogenous symbol on its left side. `None` when it names none (7.1 refuses that file).
    fn equation_named_endogenous(&self, eq: &Equation) -> Option<String> {
        if let Some(name) = eq.tag_map.get("endogenous") {
            if !name.is_empty() {
                return Some(name.clone());
            }
        }
        let id = eq.lhs_expr?;
        let mut names: Vec<String> = Vec::new();
        self.collect_names(id, true, &mut names);
        if names.len() == 1 {
            names.pop()
        } else {
            None
        }
    }

    /// Distinct names used by an expression tree (`endogenous_only` keeps just declared
    /// endogenous symbols ? 7.1's left-side variable count).
    fn collect_names(&self, id: ExprId, endogenous_only: bool, out: &mut Vec<String>) {
        for ident in self.model.exprs.walk_idents(id) {
            let keep = !endogenous_only
                || self
                    .model
                    .endogenous
                    .iter()
                    .any(|decl| decl.name == ident.name);
            let text = self.intern.get(ident.name).to_string();
            if keep && !out.iter().any(|seen| seen == &text) {
                out.push(text);
            }
        }
    }

    /// 7.1 changes the type of each removed equation's endogenous: exogenous while it is
    /// still used somewhere, gone otherwise. Each name leaves a record with the
    /// statement's span, so a later check can tell whether a statement read it while it
    /// was still endogenous.
    fn apply_excluded_type_change(&mut self, removed: &[RemovedEquation], span: Span) {
        let names: Vec<String> = removed
            .iter()
            .filter_map(|row| row.endogenous.clone())
            .collect();
        if names.is_empty() {
            return;
        }
        let mut used: Vec<String> = Vec::new();
        for eq in &self.model.equations {
            for id in [eq.lhs_expr, eq.rhs_expr].into_iter().flatten() {
                self.collect_names(id, false, &mut used);
            }
        }
        for name in names {
            let Some(pos) = self
                .model
                .endogenous
                .iter()
                .position(|decl| self.intern.get(decl.name) == name.as_str())
            else {
                continue;
            };
            let decl = self.model.endogenous.remove(pos);
            let name_id = decl.name;
            let exit = if used.iter().any(|seen| seen == &name) {
                self.model.exogenous.push(decl);
                SurgeryKind::Exogenous
            } else {
                self.prune_dropped_symbol(&decl);
                self.model.excluded_endogenous.push(decl);
                SurgeryKind::Dropped
            };
            self.model.surgery_exits.push(SurgeryExit {
                name: name_id,
                statement: span,
                kind: exit,
            });
        }
    }

    /// 7.1 accepts what a block recorded **before** the removal for a symbol the removal
    /// then drops, but still refuses `observation_trends` and `filter_initial_state`, so
    /// only the entries it accepts are dropped here. The declaration moves to
    /// `Model::excluded_endogenous` (7.1's `excludedVariable`).
    fn prune_dropped_symbol(&mut self, decl: &Decl) {
        self.model.initval.retain(|entry| entry.name != decl.name);
        self.model.endval.retain(|entry| entry.name != decl.name);
        self.model.histval.retain(|entry| entry.name != decl.name);
        self.model
            .varobs
            .retain(|observed| observed.name != decl.name);
    }

    fn parse_ss_block(&mut self) {
        let opener_span = self.bump_plain_opener();
        let start = opener_span.start;
        let body_i = self.i;
        // Same body as `model`: an opener spelling is an identifier here.
        self.in_equation_body = true;
        while !self.at(TokenKind::Eof) && !self.at_block_stop() {
            if let Some((eq, _)) = self.parse_equation_statement() {
                self.model.steady_state_equations.push(eq);
            }
        }
        self.in_equation_body = false;
        if self.at_block_end() {
            self.record_missing_final("steady_state_model", body_i, self.i);
        }
        let end = self.finish_block_named("steady_state_model", opener_span, body_i);
        self.model.ss_block = Some(Span { start, end });
    }

    fn parse_initval_block(&mut self) {
        let opener_span = self.bump_init_end_opener(true);
        let start = opener_span.start;
        let body_i = self.i;
        let body_end_i = self.consume_until_end();
        self.record_missing_end_if_unclosed("initval", opener_span, body_i, body_end_i);
        if self.i > body_end_i {
            self.record_missing_final("initval", body_i, body_end_i);
        }
        let end = self.block_end_after_consume();
        self.model.initval_block = Some(Span { start, end });
        let saved = self.i;
        self.i = body_i;
        while self.i < body_end_i && !self.at(TokenKind::Eof) {
            if self.at(TokenKind::Semi) {
                self.bump();
                continue;
            }
            if self.refuse_end_word(body_end_i) {
                continue;
            }
            let before = self.i;
            if let Some(a) = self.parse_named_assignment() {
                self.model.initval.push(a);
            }
            if self.i <= before {
                self.bump();
            }
            if self.i > body_end_i {
                self.i = body_end_i;
                break;
            }
        }
        self.i = saved;
    }

    fn parse_endval_block(&mut self) {
        let opener_i = self.i;
        let opener_span = self.bump_init_end_opener(false);
        let start = opener_span.start;
        let body_i = self.i;
        let body_end_i = self.consume_until_end();
        self.record_missing_end_if_unclosed("endval", opener_span, body_i, body_end_i);
        if self.i > body_end_i {
            self.record_missing_final("endval", body_i, body_end_i);
        }
        let end = self.block_end_after_consume();
        self.model.endval_block = Some(Span { start, end });
        for (raw, span) in self.statements_in(body_i, body_end_i) {
            if statement_is_end_word(&raw) {
                self.record_issue(ParseIssue {
                    kind: ParseIssueKind::UnexpectedEndAssign,
                    span,
                });
                continue;
            }
            if let Some(a) = self.assignment_from(&raw, span) {
                self.model.endval.push(a);
            }
        }
        self.collect_endval_instruction(opener_i, body_i, body_end_i, end);
    }

    fn parse_histval_block(&mut self) {
        let opener_span = self.bump_histval_opener();
        let start = opener_span.start;
        let body_i = self.i;
        let body_end_i = self.consume_until_end();
        self.record_missing_end_if_unclosed("histval", opener_span, body_i, body_end_i);
        let end = self.block_end_after_consume();
        if self.model.histval_block.is_none() {
            self.model.histval_block = Some(Span { start, end });
        }
        self.model
            .histval_block_starts
            .push(self.model.histval.len());
        let saved = self.i;
        self.i = body_i;
        while self.i < body_end_i && !self.at(TokenKind::Eof) {
            if self.at(TokenKind::Semi) {
                self.bump();
                continue;
            }
            if self.refuse_end_word(body_end_i) {
                continue;
            }
            let before = self.i;
            if let Some(entry) = self.parse_histval_entry(body_end_i) {
                self.model.histval.push(entry);
            }
            if self.i <= before {
                self.bump();
            }
            if self.i > body_end_i {
                self.i = body_end_i;
                break;
            }
        }
        self.i = saved;
    }

    fn bump_histval_opener(&mut self) -> Span {
        let start = self.bump().span.start;
        if self.at(TokenKind::LParen) {
            let from = self.i;
            self.skip_balanced(TokenKind::LParen, TokenKind::RParen);
            self.record_option_twice(from, self.i);
            if self.option_ident_in_range(from, self.i, "all_values_required") {
                self.model.histval_all_values_required = true;
            }
        }
        let end = if self.at(TokenKind::Semi) {
            self.bump().span.end
        } else {
            self.current_start()
        };
        Span { start, end }
    }

    fn parse_histval_entry(&mut self, end_i: usize) -> Option<HistvalEntry> {
        if self.i >= end_i || !self.at(TokenKind::Ident) {
            return None;
        }
        let name_tok = self.bump();
        let lexeme = self.lexeme(&name_tok).to_string();
        let name = self.intern.intern(&lexeme);
        if !self.at(TokenKind::LParen) {
            return None;
        }
        let (lag, _) = self.parse_signed_int_in_parens();
        self.eat(TokenKind::Eq);
        let expr = self.parse_expr();
        let end = self.finish_shock_stmt(end_i);
        Some(HistvalEntry {
            name,
            lag,
            span: Span {
                start: name_tok.span.start,
                end,
            },
            expr,
        })
    }

    fn parse_osr_params_bounds(&mut self) {
        let opener_span = self.bump_plain_opener();
        let start = opener_span.start;
        let body_i = self.i;
        let body_end_i = self.consume_until_end();
        self.record_missing_end_if_unclosed("osr_params_bounds", opener_span, body_i, body_end_i);
        let end = self.current_start();
        if self.model.osr_params_bounds_span.is_none() {
            self.model.osr_params_bounds_span = Some(Span { start, end });
        }
        let saved = self.i;
        self.i = body_i;
        while self.i < body_end_i && !self.at(TokenKind::Eof) {
            if self.at(TokenKind::Semi) {
                self.bump();
                continue;
            }
            let before = self.i;
            if let Some(bound) = self.parse_osr_bound(body_end_i) {
                self.model.osr_params_bounds.push(bound);
            }
            if self.i <= before {
                self.bump();
            }
        }
        self.i = saved;
    }

    fn parse_osr_bound(&mut self, end_i: usize) -> Option<OsrBound> {
        if self.i >= end_i || !self.at(TokenKind::Ident) {
            return None;
        }
        let name_tok = self.bump();
        let lexeme = self.lexeme(&name_tok).to_string();
        let name = self.intern.intern(&lexeme);
        self.eat(TokenKind::Comma);
        let lower = self.parse_expr();
        self.eat(TokenKind::Comma);
        let upper = self.parse_expr();
        let end = self.finish_shock_stmt(end_i);
        Some(OsrBound {
            name,
            span: Span {
                start: name_tok.span.start,
                end,
            },
            lower,
            upper,
        })
    }

    fn parse_generate_irfs_block(&mut self) {
        let opener_span = self.bump_plain_opener();
        let start = opener_span.start;
        let body_i = self.i;
        let body_end_i = self.consume_until_end();
        self.record_missing_end_if_unclosed("generate_irfs", opener_span, body_i, body_end_i);
        let end = self.current_start();
        if self.model.generate_irfs_span.is_none() {
            self.model.generate_irfs_span = Some(Span { start, end });
        }
        self.model
            .generate_irfs_block_starts
            .push(self.model.generate_irfs.len());
        let saved = self.i;
        self.i = body_i;
        while self.i < body_end_i && !self.at(TokenKind::Eof) {
            if self.at(TokenKind::Semi) {
                self.bump();
                continue;
            }
            let before = self.i;
            if let Some(el) = self.parse_generate_irfs_element(body_end_i) {
                self.model.generate_irfs.push(el);
            }
            if self.i <= before {
                self.bump();
            }
        }
        self.i = saved;
    }

    fn parse_generate_irfs_element(&mut self, end_i: usize) -> Option<GenerateIrfsElement> {
        if self.i >= end_i || !self.at(TokenKind::Ident) {
            return None;
        }
        let name_tok = self.bump();
        let lexeme = self.lexeme(&name_tok).to_string();
        let name = self.intern.intern(&lexeme);
        let mut exos = Vec::new();
        while self.i < end_i && !self.at(TokenKind::Semi) {
            if self.at(TokenKind::Comma) {
                self.bump();
                continue;
            }
            if self.at(TokenKind::Ident) {
                let exo_tok = self.bump();
                let exo_lex = self.lexeme(&exo_tok).to_string();
                let exo = self.intern.intern(&exo_lex);
                self.eat(TokenKind::Eq);
                if self.at(TokenKind::Plus) || self.at(TokenKind::Minus) {
                    self.bump();
                }
                if self.at(TokenKind::Number) {
                    self.bump();
                }
                exos.push((exo, exo_tok.span));
                continue;
            }
            self.bump();
        }
        let end = self.finish_shock_stmt(end_i);
        Some(GenerateIrfsElement {
            name,
            span: Span {
                start: name_tok.span.start,
                end,
            },
            exos,
        })
    }

    /// `epilogue; ident = expr; ? end;` Names are epilogue-typed, not endogenous.
    fn parse_epilogue_block(&mut self) {
        let opener_span = self.bump_plain_opener();
        let start = opener_span.start;
        let body_i = self.i;
        let body_end_i = self.consume_until_end();
        self.record_missing_end_if_unclosed("epilogue", opener_span, body_i, body_end_i);
        let end = self.block_end_after_consume();
        if self.model.epilogue_block.is_none() {
            self.model.epilogue_block = Some(Span { start, end });
        }
        let saved = self.i;
        self.i = body_i;
        while self.i < body_end_i && !self.at(TokenKind::Eof) {
            if self.at(TokenKind::Semi) {
                self.bump();
                continue;
            }
            if self.refuse_end_word(body_end_i) {
                continue;
            }
            let before = self.i;
            if let Some(assignment) = self.parse_named_assignment() {
                self.model.epilogue.push(assignment);
            }
            if self.i <= before {
                self.bump();
            }
            if self.i > body_end_i {
                self.i = body_end_i;
                break;
            }
        }
        self.i = saved;
    }

    /// `trend_var(options) A, B;` / `log_trend_var(options) A;`
    fn parse_trend_declaration(&mut self, log_trend: bool) {
        self.bump();
        let mut growth = None;
        if self.at(TokenKind::LParen) {
            self.bump();
            while !self.at(TokenKind::Eof) && !self.at(TokenKind::RParen) {
                if self.at_ident_ci("growth_factor") || self.at_ident_ci("log_growth_factor") {
                    self.bump();
                    if self.at(TokenKind::Eq) {
                        self.bump();
                        let expr = self.parse_expr();
                        if growth.is_none() {
                            growth = expr;
                        }
                    }
                    continue;
                }
                if self.at(TokenKind::LParen) {
                    self.skip_balanced(TokenKind::LParen, TokenKind::RParen);
                    continue;
                }
                self.bump();
            }
            self.eat(TokenKind::RParen);
        }
        while !self.at(TokenKind::Eof) && !self.at(TokenKind::Semi) {
            if self.at(TokenKind::LParen) {
                self.skip_balanced(TokenKind::LParen, TokenKind::RParen);
                continue;
            }
            if self.at(TokenKind::Ident) {
                let tok = self.bump();
                let lex = self.lexeme(&tok).to_string();
                let name = self.intern.intern(&lex);
                self.model.trend_vars.push(TrendVar {
                    name,
                    span: tok.span,
                    log_trend,
                    growth,
                });
                continue;
            }
            self.bump();
        }
        self.eat(TokenKind::Semi);
    }

    /// `load_params_and_steady_state('file');`
    fn parse_load_params(&mut self) {
        let start = self.current_start();
        self.bump();
        let mut filename = None;
        if self.at(TokenKind::LParen) {
            self.bump();
            if self.at(TokenKind::String) || self.at(TokenKind::Ident) {
                let tok = self.bump();
                filename = Some(self.lexeme(&tok).to_string());
            }
            while !self.at(TokenKind::Eof) && !self.at(TokenKind::RParen) {
                self.bump();
            }
            self.eat(TokenKind::RParen);
        }
        let end = if self.at(TokenKind::Semi) {
            self.bump().span.end
        } else {
            self.current_start()
        };
        if self.model.load_params_file.is_none() {
            if let Some(raw) = filename {
                let file = unquote_string(&raw).replace('\\', "/");
                self.model.load_params_file = Some((file, Span { start, end }));
            }
        }
    }

    /// `filter_initial_state; name(lag) = expr; ? end;`
    fn parse_filter_initial_state_block(&mut self) {
        let opener_span = self.bump_plain_opener();
        let start = opener_span.start;
        let body_i = self.i;
        let body_end_i = self.consume_until_end();
        self.record_missing_end_if_unclosed(
            "filter_initial_state",
            opener_span,
            body_i,
            body_end_i,
        );
        let end = self.block_end_after_consume();
        if self.model.filter_initial_state_block.is_none() {
            self.model.filter_initial_state_block = Some(Span { start, end });
        }
        self.model
            .filter_initial_state_block_starts
            .push(self.model.filter_initial_state.len());
        let saved = self.i;
        self.i = body_i;
        while self.i < body_end_i && !self.at(TokenKind::Eof) {
            if self.at(TokenKind::Semi) {
                self.bump();
                continue;
            }
            let before = self.i;
            if let Some(entry) = self.parse_histval_entry(body_end_i) {
                self.model.filter_initial_state.push(entry);
            }
            if self.i <= before {
                self.bump();
            }
            if self.i > body_end_i {
                self.i = body_end_i;
                break;
            }
        }
        self.i = saved;
    }

    /// `external_function(name=?, nargs=?, first_deriv_provided[, =?], ?);`
    fn parse_external_function(&mut self) {
        let start = self.current_start();
        self.bump();
        let mut stmt = ExternalFunctionStmt {
            name: None,
            nargs: None,
            first_deriv: None,
            second_deriv: None,
            span: Span { start, end: start },
        };
        if self.at(TokenKind::LParen) {
            let from = self.i;
            self.bump();
            while !self.at(TokenKind::Eof) && !self.at(TokenKind::RParen) {
                let Some(opt) = self.parse_external_function_option() else {
                    self.bump();
                    continue;
                };
                let value = if opt.value_lex.is_empty() {
                    None
                } else {
                    Some((opt.value_lex.clone(), opt.value_span))
                };
                match opt.ident.to_ascii_lowercase().as_str() {
                    "name" if stmt.name.is_none() => {
                        if let Some((raw, span)) = value {
                            let id = self.intern.intern(&unquote_string(&raw));
                            stmt.name = Some((id, span));
                            self.push_external_function_name(id);
                        }
                    }
                    "nargs" if stmt.nargs.is_none() => {
                        stmt.nargs = value.as_ref().and_then(|(v, _)| parse_int_lexeme(v));
                    }
                    "first_deriv_provided" if stmt.first_deriv.is_none() => {
                        stmt.first_deriv = Some(self.deriv_spec(opt.span, value));
                    }
                    "second_deriv_provided" if stmt.second_deriv.is_none() => {
                        stmt.second_deriv = Some(self.deriv_spec(opt.span, value));
                    }
                    _ => {}
                }
            }
            self.eat(TokenKind::RParen);
            self.record_option_twice(from, self.i);
        }
        let end = if self.at(TokenKind::Semi) {
            self.bump().span.end
        } else {
            self.current_start()
        };
        stmt.span = Span { start, end };
        self.model.external_functions.push(stmt);
    }

    fn parse_external_function_option(&mut self) -> Option<TopOption> {
        if !self.at(TokenKind::Ident) {
            return None;
        }
        let ident = self.lexeme(&self.tokens[self.i]).to_string();
        let span = self.tokens[self.i].span;
        let mut eq = false;
        if self.peek_kind(1) == Some(TokenKind::Eq) {
            eq = true;
        } else if self.peek_kind(1) != Some(TokenKind::Comma)
            && self.peek_kind(1) != Some(TokenKind::RParen)
        {
            return None;
        }
        self.bump();
        let mut value_lex = String::new();
        let mut value_span = span;
        if eq {
            self.bump();
            if self.at(TokenKind::String) || self.at(TokenKind::Ident) || self.at(TokenKind::Number)
            {
                let tok = self.bump();
                value_lex = self.lexeme(&tok).to_string();
                value_span = tok.span;
            }
        }
        Some(TopOption {
            ident,
            span,
            eq,
            value_lex,
            value_span,
        })
    }

    fn deriv_spec(&mut self, opt_span: Span, value: Option<(String, Span)>) -> DerivSpec {
        match value {
            // Their driver declares a named derivative as an externalFunction
            // symbol too (`ParsingDriver::external_function`), so the name lands
            // in the same unsupported-slot set as `name=`.
            Some((lex, span)) => {
                let id = self.intern.intern(&unquote_string(&lex));
                self.push_external_function_name(id);
                DerivSpec::Named(id, span)
            }
            None => DerivSpec::Bare(opt_span),
        }
    }

    fn push_external_function_name(&mut self, id: Name) {
        if !self.model.external_function_names.contains(&id) {
            self.model.external_function_names.push(id);
        }
    }

    /// `init2shocks(name=group); endo exo; ? end;`
    fn parse_init2shocks_block(&mut self) {
        let opener_span = self.bump_plain_opener();
        let body_i = self.i;
        let body_end_i = self.consume_until_end();
        self.record_missing_end_if_unclosed("init2shocks", opener_span, body_i, body_end_i);
        let (group, group_span) = self.group_name_from_opener(opener_span);
        let span = Span {
            start: opener_span.start,
            end: self.block_end_after_consume(),
        };
        let mut rows = Vec::new();
        let saved = self.i;
        self.i = body_i;
        while self.i < body_end_i && !self.at(TokenKind::Eof) {
            if self.at(TokenKind::Semi) || self.at(TokenKind::Comma) {
                self.bump();
                continue;
            }
            let before = self.i;
            if let Some(row) = self.parse_pair_symbols(body_end_i) {
                rows.push(row);
            }
            if self.i <= before {
                self.bump();
            }
            if self.i > body_end_i {
                self.i = body_end_i;
                break;
            }
        }
        self.i = saved;
        self.model.init2shocks_blocks.push(Init2ShocksBlock {
            group,
            group_span,
            rows,
            span,
        });
    }

    /// Two symbols through `;`: `a b;` or `a, b;`.
    fn parse_pair_symbols(&mut self, end_i: usize) -> Option<Init2ShocksRow> {
        if !self.at(TokenKind::Ident) {
            return None;
        }
        let first = self.bump();
        let first_lex = self.lexeme(&first).to_string();
        self.eat(TokenKind::Comma);
        if !self.at(TokenKind::Ident) {
            self.skip_to_stmt_end();
            self.eat(TokenKind::Semi);
            return None;
        }
        let second = self.bump();
        let second_lex = self.lexeme(&second).to_string();
        let end = self.finish_shock_stmt(end_i);
        Some(Init2ShocksRow {
            endo: self.intern.intern(&first_lex),
            endo_span: first.span,
            exo: self.intern.intern(&second_lex),
            exo_span: second.span,
            span: Span {
                start: first.span.start,
                end,
            },
        })
    }

    /// `homotopy_setup[(from_initval_to_endval)]; name, expr[, expr]; ? end;`
    fn parse_homotopy_setup_block(&mut self) {
        let opener_span = self.bump_plain_opener();
        let body_i = self.i;
        let body_end_i = self.consume_until_end();
        self.record_missing_end_if_unclosed("homotopy_setup", opener_span, body_i, body_end_i);
        let saved = self.i;
        self.i = body_i;
        while self.i < body_end_i && !self.at(TokenKind::Eof) {
            if self.at(TokenKind::Semi) || self.at(TokenKind::Comma) {
                self.bump();
                continue;
            }
            let before = self.i;
            if let Some(row) = self.parse_homotopy_row(body_end_i) {
                self.model.homotopy_rows.push(row);
            }
            if self.i <= before {
                self.bump();
            }
            if self.i > body_end_i {
                self.i = body_end_i;
                break;
            }
        }
        self.i = saved;
    }

    fn parse_homotopy_row(&mut self, end_i: usize) -> Option<HomotopyRow> {
        if !self.at(TokenKind::Ident) {
            return None;
        }
        let name_tok = self.bump();
        let lex = self.lexeme(&name_tok).to_string();
        if !self.at(TokenKind::Comma) {
            self.skip_to_stmt_end();
            self.eat(TokenKind::Semi);
            return None;
        }
        while self.i < end_i && !self.at(TokenKind::Semi) && !self.at(TokenKind::Eof) {
            if self.at(TokenKind::Comma) {
                self.bump();
                continue;
            }
            let before = self.i;
            self.parse_expr();
            if self.i <= before {
                self.bump();
            }
        }
        let end = self.finish_shock_stmt(end_i);
        Some(HomotopyRow {
            name: self.intern.intern(&lex),
            span: Span {
                start: name_tok.span.start,
                end,
            },
        })
    }

    /// `shock_groups[(name=group)]; symbol = name_list; ? end;`
    fn parse_shock_groups_block(&mut self) {
        let opener_span = self.bump_plain_opener();
        let body_i = self.i;
        let body_end_i = self.consume_until_end();
        self.record_missing_end_if_unclosed("shock_groups", opener_span, body_i, body_end_i);
        let (group, group_span) = self.group_name_from_opener(opener_span);
        let span = Span {
            start: opener_span.start,
            end: self.block_end_after_consume(),
        };
        let saved = self.i;
        self.i = body_i;
        let row_start = self.model.shock_groups.len();
        self.model.shock_group_block_starts.push(row_start);
        while self.i < body_end_i && !self.at(TokenKind::Eof) {
            if self.at(TokenKind::Semi) || self.at(TokenKind::Comma) {
                self.bump();
                continue;
            }
            let before = self.i;
            if let Some(group) = self.parse_shock_group(body_end_i) {
                self.model.shock_groups.push(group);
            }
            if self.i <= before {
                self.bump();
            }
            if self.i > body_end_i {
                self.i = body_end_i;
                break;
            }
        }
        self.i = saved;
        self.model.shock_group_blocks.push(ShockGroupBlock {
            group,
            group_span,
            row_start,
            row_end: self.model.shock_groups.len(),
            span,
        });
    }

    fn parse_shock_group(&mut self, end_i: usize) -> Option<ShockGroup> {
        if !self.at(TokenKind::Ident) && !self.at(TokenKind::String) {
            return None;
        }
        // The label token: `g1 = …` or `'g1' = …`. Their grammar takes both
        // (`symbol` / `QUOTED_STRING`), and the reuse warning compares the
        // unquoted text, so quotes are stripped here.
        let label_tok = self.bump();
        let label = unquote_string(self.lexeme(&label_tok));
        let label_span = label_tok.span;
        if !self.at(TokenKind::Eq) {
            self.skip_to_stmt_end();
            self.eat(TokenKind::Semi);
            return None;
        }
        self.bump();
        let mut members = Vec::new();
        while self.i < end_i && !self.at(TokenKind::Semi) && !self.at(TokenKind::Eof) {
            if self.at(TokenKind::Comma) {
                self.bump();
                continue;
            }
            if self.at(TokenKind::Ident) {
                let tok = self.bump();
                let lex = self.lexeme(&tok).to_string();
                members.push((self.intern.intern(&lex), tok.span));
                continue;
            }
            self.bump();
        }
        let end = if self.at(TokenKind::Semi) {
            self.bump().span.end
        } else {
            self.current_start()
        };
        Some(ShockGroup {
            label,
            label_span,
            members,
            span: Span {
                start: label_tok.span.start,
                end,
            },
        })
    }

    /// `method_of_moments[(?)];` — a `;` statement, not a block. Its `(…)` list is
    /// recorded as `FamilyOption` rows and keeps the option side-effects the skip
    /// path used to run, so **W160** / **E271** do not move.
    fn parse_mom_statement(&mut self) {
        let keyword = self.bump().span;
        let mut options = Vec::new();
        let mut has_option_list = false;
        if self.at(TokenKind::LParen) {
            has_option_list = true;
            let from = self.i;
            self.skip_balanced(TokenKind::LParen, TokenKind::RParen);
            let close_i = self.i;
            self.record_deprecated_options_in_range(from, close_i);
            self.record_skip_command_options("method_of_moments", from, close_i);
            self.record_option_twice(from, close_i);
            options = self.read_family_options(from, close_i);
        }
        while !self.at(TokenKind::Semi) && !self.at(TokenKind::Eof) {
            self.bump();
        }
        let end = if self.at(TokenKind::Semi) {
            self.bump().span.end
        } else {
            self.current_start()
        };
        // The clash check reads the keyword's own span, not the statement's.
        if self.model.method_of_moments_span.is_none() {
            self.model.method_of_moments_span = Some(keyword);
        }
        self.model.mom_statements.push(MomStatement {
            span: Span {
                start: keyword.start,
                end,
            },
            has_option_list,
            options,
        });
    }

    /// `matched_moments;` one model expression per `;`, `end;`.
    fn parse_matched_moments_block(&mut self) {
        // A row is one expression. A declared opener spelling (`shocks;`) is that
        // expression, which 7.1 accepts; it is not the next block.
        self.in_equation_body = true;
        let (opener_span, _, body_i, body_end_i) = self.bump_block_opener("matched_moments");
        let end = self.block_end_after_consume();
        self.model.matched_moments_blocks.push(Span {
            start: opener_span.start,
            end,
        });
        let saved = self.i;
        self.i = body_i;
        while self.i < body_end_i && !self.at(TokenKind::Eof) {
            let before = self.i;
            if let Some(row) = self.read_matched_moment(body_end_i) {
                self.model.matched_moments.push(row);
            }
            if self.i <= before {
                self.bump();
            }
            if self.i > body_end_i {
                self.i = body_end_i;
                break;
            }
        }
        self.in_equation_body = false;
        self.i = saved;
    }

    /// One `model_expression ';'` row. An empty row is skipped, not stored.
    /// A token the grammar cannot reduce (`y = 3`, `foo.bar`) records the
    /// sentence 7.1 prints and stores nothing.
    fn read_matched_moment(&mut self, end_i: usize) -> Option<MatchedMoment> {
        if self.at(TokenKind::Semi) {
            self.bump();
            return None;
        }
        if self.looks_like_dotted_name() {
            return self.read_dotted_moment(end_i);
        }
        let start = self.current_start();
        let expr_from = self.i;
        let expr = self.parse_expr();
        if !self.at(TokenKind::Semi) {
            // `y = 3` stops on EQUAL and names nothing it expected.
            let expecting = if self.at(TokenKind::Eq) {
                None
            } else {
                Some("';'")
            };
            self.record_syntax_at(self.i, expecting);
            self.skip_until_semi();
            self.eat(TokenKind::Semi);
            return None;
        }
        let text = join_lexemes(self.src, &self.tokens[expr_from..self.i]);
        let end = self.finish_row(end_i);
        Some(MatchedMoment {
            text,
            span: Span { start, end },
            expr,
        })
    }

    /// `matched_irfs[(overwrite)];` one `var`/`varexo` pair per `;`, `end;`.
    fn parse_matched_irfs_block(&mut self) {
        let (opener_span, word, body_i, body_end_i) = self.bump_block_opener("matched_irfs");
        let end = self.block_end_after_consume();
        let rows = self.read_matched_irfs_rows(body_i, body_end_i);
        self.model.matched_irfs.push(MatchedIrfsBlock {
            span: Span {
                start: opener_span.start,
                end,
            },
            overwrite: is_flag_word(&word, "overwrite"),
            rows,
        });
    }

    fn read_matched_irfs_rows(&mut self, body_i: usize, body_end_i: usize) -> Vec<MatchedIrfsRow> {
        let saved = self.i;
        self.i = body_i;
        let mut rows = Vec::new();
        while self.i < body_end_i && !self.at(TokenKind::Eof) {
            let before = self.i;
            if self.at(TokenKind::Semi) {
                self.bump();
                continue;
            }
            if let Some(row) = self.read_matched_irfs_row(body_end_i, rows.is_empty()) {
                rows.push(row);
            }
            if self.i <= before {
                self.skip_until_semi();
                self.eat(TokenKind::Semi);
            }
            if self.i > body_end_i {
                self.i = body_end_i;
                break;
            }
        }
        self.i = saved;
        rows
    }

    /// One `var ENDO; varexo EXO; periods …; values …; [weights …;]` row, in either
    /// `var`/`varexo` order and either `values`/`weights` order.
    ///
    /// A token the grammar cannot reduce records 7.1's sentence and abandons the
    /// rest of the block: their parser stops at the first of these.
    fn read_matched_irfs_row(&mut self, end_i: usize, block_empty: bool) -> Option<MatchedIrfsRow> {
        let start = self.current_start();
        if !self.at_ident_ci("var") && !self.at_ident_ci("varexo") {
            let expecting = if block_empty {
                "VAR or VAREXO"
            } else {
                "END or VAR or VAREXO"
            };
            self.record_syntax_at(self.i, Some(expecting));
            return None;
        }
        let mut endogenous = None;
        let mut exogenous = None;
        while self.at_ident_ci("var") || self.at_ident_ci("varexo") {
            let is_var = self.at_ident_ci("var");
            self.bump();
            let (name, span) = self.read_symbol()?;
            if !self.at(TokenKind::Semi) {
                // `var y(1)` — the name slot is a symbol, then `;`.
                self.record_syntax_at(self.i, Some("';'"));
                self.i = end_i;
                return None;
            }
            self.eat(TokenKind::Semi);
            if is_var {
                endogenous.get_or_insert((name, span));
            } else {
                exogenous.get_or_insert((name, span));
            }
        }
        let (endogenous, endogenous_span) = endogenous?;
        let (exogenous, exogenous_span) = exogenous?;
        if !self.at_ident_ci("periods") {
            self.record_syntax_at(self.i, Some("PERIODS"));
            self.i = end_i;
            return None;
        }
        let periods = match self.read_irf_period_list(end_i) {
            Some(periods) => periods,
            None => {
                self.i = end_i;
                return None;
            }
        };
        let value_weights = match self.read_irf_value_weights(end_i) {
            Some(lists) => lists,
            None => {
                self.i = end_i;
                return None;
            }
        };
        let end = self.finish_row(end_i);
        Some(MatchedIrfsRow {
            endogenous,
            endogenous_span,
            exogenous,
            exogenous_span,
            periods,
            values: value_weights.values,
            value_exprs: value_weights.value_exprs,
            weights: value_weights.weights,
            weight_exprs: value_weights.weight_exprs,
            span: Span { start, end },
        })
    }

    /// `matched_irfs_weights[(overwrite)];` one four-name tuple per `;`, `end;`.
    fn parse_matched_irfs_weights_block(&mut self) {
        let (opener_span, word, body_i, body_end_i) =
            self.bump_block_opener("matched_irfs_weights");
        let end = self.block_end_after_consume();
        let saved = self.i;
        self.i = body_i;
        let mut rows = Vec::new();
        while self.i < body_end_i && !self.at(TokenKind::Eof) {
            let before = self.i;
            if self.at(TokenKind::Semi) {
                self.bump();
                continue;
            }
            if let Some(row) = self.read_matched_irfs_weight_row(body_end_i) {
                rows.push(row);
            }
            if self.i <= before {
                self.skip_until_semi();
                self.eat(TokenKind::Semi);
            }
            if self.i > body_end_i {
                self.i = body_end_i;
                break;
            }
        }
        self.i = saved;
        self.model
            .matched_irfs_weight_rows
            .extend(rows.iter().cloned());
        self.model
            .matched_irfs_weights
            .push(MatchedIrfsWeightsBlock {
                span: Span {
                    start: opener_span.start,
                    end,
                },
                overwrite: is_flag_word(&word, "overwrite"),
                rows,
            });
    }

    /// One `name(periods), exo, name(periods), exo, expression;` row.
    fn read_matched_irfs_weight_row(&mut self, end_i: usize) -> Option<MatchedIrfsWeight> {
        let start = self.current_start();
        let (left_endo, left_endo_span, left_periods, left_periods_span) =
            self.read_weighted_symbol()?;
        self.eat(TokenKind::Comma);
        let (left_exo, left_exo_span) = self.read_symbol()?;
        self.eat(TokenKind::Comma);
        let (right_endo, right_endo_span, right_periods, right_periods_span) =
            self.read_weighted_symbol()?;
        self.eat(TokenKind::Comma);
        let (right_exo, right_exo_span) = self.read_symbol()?;
        self.eat(TokenKind::Comma);
        let (weight_text, weight_span, weight_expr) = self.read_expression_text();
        let end = self.finish_row(end_i);
        Some(MatchedIrfsWeight {
            left_endo,
            left_endo_span,
            left_periods,
            left_periods_span,
            left_exo,
            left_exo_span,
            right_endo,
            right_endo_span,
            right_periods,
            right_periods_span,
            right_exo,
            right_exo_span,
            weight_text,
            weight_span,
            weight_expr,
            span: Span { start, end },
        })
    }

    /// `name` then the required `(integer_or_range)`. `1` and `1:2` are each one
    /// period entry. A missing `(` is their syntax error on the next token.
    fn read_weighted_symbol(&mut self) -> Option<(Name, Span, String, Span)> {
        let (name, span) = self.read_symbol()?;
        if !self.at(TokenKind::LParen) {
            self.record_syntax_at(self.i, Some("'('"));
            return None;
        }
        self.read_checked_paren(false)
            .map(|(text, group_span)| (name, span, text, group_span))
    }

    /// `moment_calibration;` one `name, name[(lags)], range;` row, `end;`.
    fn parse_moment_calibration_block(&mut self) {
        let (opener_span, _, body_i, body_end_i) = self.bump_block_opener("moment_calibration");
        let end = self.block_end_after_consume();
        let saved = self.i;
        self.i = body_i;
        let mut rows = Vec::new();
        while self.i < body_end_i && !self.at(TokenKind::Eof) {
            let before = self.i;
            if self.at(TokenKind::Semi) {
                self.bump();
                continue;
            }
            if let Some(row) = self.read_moment_calibration_row(body_end_i) {
                rows.push(row);
            }
            if self.i <= before {
                self.skip_until_semi();
                self.eat(TokenKind::Semi);
            }
            if self.i > body_end_i {
                self.i = body_end_i;
                break;
            }
        }
        self.i = saved;
        self.model.moment_calibration.push(MomentCalibrationBlock {
            span: Span {
                start: opener_span.start,
                end,
            },
            rows,
        });
    }

    fn read_moment_calibration_row(&mut self, end_i: usize) -> Option<MomentCalibrationRow> {
        let start = self.current_start();
        let (first, first_span) = self.read_symbol()?;
        if !self.at(TokenKind::Comma) {
            self.record_syntax_at(self.i, Some("COMMA"));
            return None;
        }
        self.eat(TokenKind::Comma);
        let (second, second_span) = self.read_symbol()?;
        let (lags, lags_span) = if self.at(TokenKind::LParen) {
            let (text, span) = self.read_checked_paren(true)?;
            (Some(text), Some(span))
        } else {
            (None, None)
        };
        self.eat(TokenKind::Comma);
        let range = self.read_calibration_range()?;
        let end = self.finish_row(end_i);
        Some(MomentCalibrationRow {
            first,
            first_span,
            second,
            second_span,
            lags,
            lags_span,
            range,
            span: Span { start, end },
        })
    }

    /// `irf_calibration[(relative_irf)];` one `name[(periods)], exo, range;` row.
    fn parse_irf_calibration_block(&mut self) {
        let (opener_span, word, body_i, body_end_i) = self.bump_block_opener("irf_calibration");
        let end = self.block_end_after_consume();
        let saved = self.i;
        self.i = body_i;
        let mut rows = Vec::new();
        while self.i < body_end_i && !self.at(TokenKind::Eof) {
            let before = self.i;
            if self.at(TokenKind::Semi) {
                self.bump();
                continue;
            }
            if let Some(row) = self.read_irf_calibration_row(body_end_i) {
                rows.push(row);
            }
            if self.i <= before {
                self.skip_until_semi();
                self.eat(TokenKind::Semi);
            }
            if self.i > body_end_i {
                self.i = body_end_i;
                break;
            }
        }
        self.i = saved;
        self.model.irf_calibration.push(IrfCalibrationBlock {
            span: Span {
                start: opener_span.start,
                end,
            },
            relative_irf: is_flag_word(&word, "relative_irf"),
            rows,
        });
    }

    fn read_irf_calibration_row(&mut self, end_i: usize) -> Option<IrfCalibrationRow> {
        let start = self.current_start();
        let (endogenous, endogenous_span) = self.read_symbol()?;
        let (periods, periods_span) = if self.at(TokenKind::LParen) {
            let (text, span) = self.read_checked_paren(false)?;
            (Some(text), Some(span))
        } else {
            (None, None)
        };
        if !self.at(TokenKind::Comma) {
            self.record_syntax_at(self.i, Some("COMMA or '('"));
            return None;
        }
        self.eat(TokenKind::Comma);
        let (exogenous, exogenous_span) = self.read_symbol()?;
        self.eat(TokenKind::Comma);
        let range = self.read_calibration_range()?;
        let end = self.finish_row(end_i);
        Some(IrfCalibrationRow {
            endogenous,
            endogenous_span,
            periods,
            periods_span,
            exogenous,
            exogenous_span,
            range,
            span: Span { start, end },
        })
    }

    /// `[expr, expr]` / `+` / `-`: a calibration row's third column.
    fn read_calibration_range(&mut self) -> Option<CalibrationRange> {
        if self.at(TokenKind::LBrack) {
            let start = self.bump().span.start;
            let lower = self.read_range_side();
            self.eat(TokenKind::Comma);
            let upper = self.read_range_side();
            let end = if self.at(TokenKind::RBrack) {
                self.bump().span.end
            } else {
                self.current_start()
            };
            return Some(CalibrationRange::Bracket {
                lower,
                upper,
                span: Span { start, end },
            });
        }
        if self.at(TokenKind::Plus) {
            return Some(CalibrationRange::Plus {
                span: self.bump().span,
            });
        }
        if self.at(TokenKind::Minus) {
            return Some(CalibrationRange::Minus {
                span: self.bump().span,
            });
        }
        None
    }

    /// One side of a `[…]` range, as written.
    fn read_range_side(&mut self) -> String {
        let from = self.i;
        while !self.at(TokenKind::Eof) && !self.at(TokenKind::Comma) && !self.at(TokenKind::RBrack)
        {
            let before = self.i;
            self.parse_expr();
            if self.i <= before {
                self.bump();
            }
        }
        join_lexemes(self.src, &self.tokens[from..self.i.min(self.tokens.len())])
    }

    /// One expression's text, span, and tree, read at the cursor.
    fn read_expression_text(&mut self) -> (String, Span, Option<ExprId>) {
        let start = self.current_start();
        let from = self.i;
        let before = self.i;
        let expr = self.parse_expr();
        if self.i <= before {
            self.bump();
        }
        let end = (from..self.i.min(self.tokens.len()))
            .rev()
            .find(|&k| self.tokens[k].kind != TokenKind::Eof)
            .map(|k| self.tokens[k].span.end)
            .unwrap_or(start);
        let text = self
            .src
            .get(start as usize..end as usize)
            .unwrap_or("")
            .trim()
            .to_string();
        (text, Span { start, end }, expr)
    }

    /// A symbol token, or `None` when the cursor is not on one.
    fn read_symbol(&mut self) -> Option<(Name, Span)> {
        if !self.at(TokenKind::Ident) {
            return None;
        }
        let tok = self.bump();
        let lex = self.lexeme(&tok).to_string();
        Some((self.intern.intern(&lex), tok.span))
    }

    fn record_mom_syntax(&mut self, span: Span, message: impl Into<String>) {
        self.model.mom_syntax.push(MomSyntax {
            span,
            message: message.into(),
        });
    }

    /// 7.1's `syntax error, unexpected …` on the token at `index`.
    fn record_syntax_at(&mut self, index: usize, expecting: Option<&str>) {
        let span = self.tokens.get(index).map(|tok| tok.span).unwrap_or(Span {
            start: self.current_start(),
            end: self.current_start(),
        });
        let unexpected = self.bison_token_name(index);
        let message = match expecting {
            Some(expected) => {
                format!("syntax error, unexpected {unexpected}, expecting {expected}")
            }
            None => format!("syntax error, unexpected {unexpected}"),
        };
        self.record_mom_syntax(span, message);
    }

    fn bison_token_name(&self, index: usize) -> String {
        let Some(tok) = self.tokens.get(index) else {
            return "end of file".to_string();
        };
        match tok.kind {
            TokenKind::Semi => "';'".to_string(),
            TokenKind::LParen => "'('".to_string(),
            TokenKind::RParen => "')'".to_string(),
            TokenKind::Comma => "COMMA".to_string(),
            TokenKind::Plus => "PLUS".to_string(),
            TokenKind::Minus => "MINUS".to_string(),
            TokenKind::Eq => "EQUAL".to_string(),
            TokenKind::Number => {
                if is_integer_lexeme(self.lexeme(tok)) {
                    "INT_NUMBER".to_string()
                } else {
                    "FLOAT_NUMBER".to_string()
                }
            }
            TokenKind::Ident => {
                let lex = self.lexeme(tok);
                if crate::model::dynare_date(lex) {
                    "DATE".to_string()
                } else if let Some(keyword) = block_keyword_token(lex) {
                    keyword.to_string()
                } else {
                    "IDENTIFIER".to_string()
                }
            }
            _ => "IDENTIFIER".to_string(),
        }
    }

    /// The `(` on a moment-block opener. One flag is legal; anything else is the
    /// syntax error on the token 7.1 stops on.
    fn note_mom_opener(&mut self, keyword: &str, open_i: usize, close_i: usize) {
        let rparen_i = close_i.saturating_sub(1);
        let flag = match keyword {
            "matched_irfs" | "matched_irfs_weights" => "OVERWRITE",
            "irf_calibration" => "RELATIVE_IRF",
            "matched_moments" | "moment_calibration" => {
                self.record_syntax_at(open_i, Some("';'"));
                return;
            }
            _ => return,
        };
        if open_i + 1 >= rparen_i {
            self.record_syntax_at(rparen_i, Some(flag));
            return;
        }
        let word = match flag {
            "OVERWRITE" => "overwrite",
            _ => "relative_irf",
        };
        let first = &self.tokens[open_i + 1];
        let ok = first.kind == TokenKind::Ident && self.lexeme(first).eq_ignore_ascii_case(word);
        if !ok {
            self.record_syntax_at(open_i + 1, Some(flag));
            return;
        }
        if open_i + 2 < rparen_i {
            self.record_syntax_at(open_i + 2, Some("')'"));
        }
    }

    fn looks_like_dotted_name(&self) -> bool {
        self.at(TokenKind::Ident)
            && self.peek_kind(1) == Some(TokenKind::Dot)
            && self.peek_kind(2) == Some(TokenKind::Ident)
    }

    /// `foo.bar` and `foo.bar(y)` in a matched-moment row. A bare dotted name
    /// stops on `;`. A call is an external function: undeclared, their declare
    /// sentence; declared, the walk's unsupported expression.
    fn read_dotted_moment(&mut self, end_i: usize) -> Option<MatchedMoment> {
        let start = self.current_start();
        let mut parts = Vec::new();
        parts.push(self.lexeme(&self.tokens[self.i]).to_string());
        self.bump();
        while self.at(TokenKind::Dot) && self.peek_kind(1) == Some(TokenKind::Ident) {
            self.bump();
            parts.push(self.lexeme(&self.tokens[self.i]).to_string());
            self.bump();
        }
        let name = parts.join(".");
        if self.at(TokenKind::LParen) {
            let call = self.skip_balanced(TokenKind::LParen, TokenKind::RParen);
            let known = self
                .model
                .external_function_names
                .iter()
                .any(|id| self.model.name(*id) == name);
            if known {
                let span = Span {
                    start,
                    end: call.end,
                };
                let expr = self.alloc(ExprKind::Error, span);
                let end = self.finish_row(end_i);
                return Some(MatchedMoment {
                    text: self
                        .src
                        .get(start as usize..end as usize)
                        .unwrap_or("")
                        .trim()
                        .trim_end_matches(';')
                        .trim()
                        .to_string(),
                    span: Span { start, end },
                    expr: Some(expr),
                });
            }
            self.record_mom_syntax(
                Span {
                    start,
                    end: call.end,
                },
                format!(
                    "To use an external function ({name}) within the model block, you must first declare it via the external_function() statement."
                ),
            );
            self.skip_until_semi();
            self.eat(TokenKind::Semi);
            return None;
        }
        if self.at(TokenKind::Semi) {
            self.record_syntax_at(self.i, Some("'(' or '.'"));
            self.bump();
            return None;
        }
        self.record_syntax_at(self.i, Some("'(' or '.'"));
        self.skip_until_semi();
        self.eat(TokenKind::Semi);
        None
    }

    /// `periods` then a `period_list`. `None` after recording the syntax error.
    fn read_irf_period_list(&mut self, end_i: usize) -> Option<Vec<Span>> {
        self.bump();
        if self.i >= end_i || self.at(TokenKind::Semi) {
            self.record_syntax_at(self.i, Some("DATE or INT_NUMBER"));
            self.eat(TokenKind::Semi);
            return None;
        }
        let mut entries = Vec::new();
        while self.i < end_i && !self.at(TokenKind::Eof) && !self.at(TokenKind::Semi) {
            if self.at(TokenKind::Comma) {
                self.bump();
                continue;
            }
            entries.push(self.take_irf_period()?);
        }
        self.eat(TokenKind::Semi);
        Some(entries)
    }

    /// One `period_range`. A date is `2000Q1`: our lexer splits the suffix off the
    /// number, and 7.1 keeps them as one `DATE`. A second `:` in `1:2:3` is the
    /// syntax error.
    fn take_irf_period(&mut self) -> Option<Span> {
        let Some(first) = self.peek_period_atom() else {
            self.record_syntax_at(self.i, Some("DATE or INT_NUMBER"));
            return None;
        };
        self.i += first.tokens;
        let left_last = self.i - 1;
        if self.colon_between_tokens(left_last, self.i).is_none() {
            return Some(first.span);
        }
        let Some(second) = self.peek_period_atom() else {
            let expecting = if first.is_date { "DATE" } else { "INT_NUMBER" };
            self.record_syntax_at(self.i, Some(expecting));
            return None;
        };
        if first.is_date != second.is_date {
            let unexpected = if second.is_date { "DATE" } else { "INT_NUMBER" };
            let expecting = if first.is_date { "DATE" } else { "INT_NUMBER" };
            self.record_mom_syntax(
                second.span,
                format!("syntax error, unexpected {unexpected}, expecting {expecting}"),
            );
            return None;
        }
        self.i += second.tokens;
        if let Some(extra) = self.colon_between_tokens(self.i - 1, self.i) {
            self.record_mom_syntax(
                extra,
                "syntax error, unexpected ':', expecting COMMA or DATE or INT_NUMBER or ';'",
            );
            return None;
        }
        Some(Span {
            start: first.span.start,
            end: second.span.end,
        })
    }

    /// An integer, or a `DATE` written as a number plus its unit (`2000` `Q1`).
    fn peek_period_atom(&self) -> Option<PeriodAtom> {
        if let Some(span) = self.glued_date_span(self.i) {
            return Some(PeriodAtom {
                is_date: true,
                span,
                tokens: 2,
            });
        }
        match self.period_kind(self.i) {
            PeriodKind::Int => Some(PeriodAtom {
                is_date: false,
                span: self.tokens[self.i].span,
                tokens: 1,
            }),
            PeriodKind::Date => Some(PeriodAtom {
                is_date: true,
                span: self.tokens[self.i].span,
                tokens: 1,
            }),
            _ => None,
        }
    }

    /// `2000Q1` when the lexer stored `2000` and `Q1` as two adjacent tokens.
    fn glued_date_span(&self, index: usize) -> Option<Span> {
        let number = self.tokens.get(index)?;
        let suffix = self.tokens.get(index + 1)?;
        if number.kind != TokenKind::Number || suffix.kind != TokenKind::Ident {
            return None;
        }
        if !is_integer_lexeme(self.lexeme(number)) || number.span.end != suffix.span.start {
            return None;
        }
        let text = &self.src[number.span.start as usize..suffix.span.end as usize];
        if !crate::model::dynare_date(text) {
            return None;
        }
        Some(Span {
            start: number.span.start,
            end: suffix.span.end,
        })
    }

    fn period_kind(&self, index: usize) -> PeriodKind {
        let Some(tok) = self.tokens.get(index) else {
            return PeriodKind::Other;
        };
        match tok.kind {
            TokenKind::Minus => PeriodKind::Minus,
            TokenKind::Number => {
                if is_integer_lexeme(self.lexeme(tok)) {
                    PeriodKind::Int
                } else {
                    PeriodKind::Float
                }
            }
            TokenKind::Ident if crate::model::dynare_date(self.lexeme(tok)) => PeriodKind::Date,
            _ => PeriodKind::Other,
        }
    }

    fn colon_between_tokens(&self, left: usize, right: usize) -> Option<Span> {
        let left = self.tokens.get(left)?;
        let right = self.tokens.get(right)?;
        if left.span.end > right.span.start {
            return None;
        }
        let between = &self.src[left.span.end as usize..right.span.start as usize];
        if between.trim() != ":" {
            return None;
        }
        let rel = between.find(':')? as u32;
        let start = left.span.end + rel;
        Some(Span {
            start,
            end: start + 1,
        })
    }

    /// `values` / `weights` after `periods`. Missing both, or a repeated `values`,
    /// is the syntax error. `None` once that error is recorded.
    fn read_irf_value_weights(&mut self, end_i: usize) -> Option<IrfValueWeights> {
        let mut saw_values = false;
        let mut saw_weights = false;
        let mut values = Vec::new();
        let mut value_exprs = Vec::new();
        let mut weights = Vec::new();
        let mut weight_exprs = Vec::new();
        loop {
            if self.i >= end_i {
                break;
            }
            if self.at_ident_ci("values") {
                if saw_values {
                    self.record_syntax_at(self.i, Some("END or VAR or VAREXO"));
                    return None;
                }
                saw_values = true;
                let (spans, exprs, ok) = self.read_value_list(end_i);
                values = spans;
                value_exprs = exprs;
                if !ok {
                    return None;
                }
                continue;
            }
            if self.at_ident_ci("weights") {
                if saw_weights {
                    self.record_syntax_at(self.i, Some("END or VAR or VAREXO"));
                    return None;
                }
                saw_weights = true;
                let (spans, exprs, ok) = self.read_value_list(end_i);
                weights = spans;
                weight_exprs = exprs;
                if !ok {
                    return None;
                }
                continue;
            }
            break;
        }
        if !saw_values {
            let expecting = if saw_weights {
                "VALUES"
            } else {
                "VALUES or WEIGHTS"
            };
            self.record_syntax_at(self.i, Some(expecting));
            return None;
        }
        Some(IrfValueWeights {
            values,
            value_exprs,
            weights,
            weight_exprs,
        })
    }

    /// One `value_list`. A bare signed number is an entry. A `(expression)` is an
    /// entry and an expression. Anything else is `unexpected IDENTIFIER` with no
    /// expecting list — their set there is too large to print.
    fn read_value_list(&mut self, end_i: usize) -> (Vec<Span>, Vec<ExprId>, bool) {
        self.bump();
        let mut spans = Vec::new();
        let mut exprs = Vec::new();
        while self.i < end_i && !self.at(TokenKind::Eof) && !self.at(TokenKind::Semi) {
            if self.at(TokenKind::Comma) {
                self.bump();
                continue;
            }
            if self.at(TokenKind::LParen) {
                let start = self.current_start();
                self.bump();
                let before = self.i;
                if let Some(id) = self.parse_expr() {
                    exprs.push(id);
                }
                if self.i <= before {
                    self.bump();
                }
                let end = if self.at(TokenKind::RParen) {
                    self.bump().span.end
                } else {
                    self.current_start()
                };
                spans.push(Span { start, end });
                continue;
            }
            if self.at_signed_number() {
                let start = self.current_start();
                if self.at(TokenKind::Plus) || self.at(TokenKind::Minus) {
                    self.bump();
                }
                let end = self.bump().span.end;
                spans.push(Span { start, end });
                continue;
            }
            self.record_syntax_at(self.i, None);
            self.skip_until_semi();
            self.eat(TokenKind::Semi);
            return (spans, exprs, false);
        }
        self.eat(TokenKind::Semi);
        (spans, exprs, true)
    }

    fn at_signed_number(&self) -> bool {
        if self.at(TokenKind::Number) {
            return true;
        }
        (self.at(TokenKind::Plus) || self.at(TokenKind::Minus))
            && self.peek_kind(1) == Some(TokenKind::Number)
    }

    /// The `(…)` of a lag or an IRF period. `signed` allows `+`/`-` and `-(a:b)`.
    /// A bad token is recorded; the group is still consumed.
    fn read_checked_paren(&mut self, signed: bool) -> Option<(String, Span)> {
        if !self.at(TokenKind::LParen) {
            return None;
        }
        let open = self.bump().span;
        let inner_from = open.end;
        if self.at(TokenKind::RParen) {
            let expecting = if signed {
                "INT_NUMBER or PLUS or MINUS"
            } else {
                "INT_NUMBER"
            };
            self.record_syntax_at(self.i, Some(expecting));
            let end = self.bump().span.end;
            return Some((
                String::new(),
                Span {
                    start: open.start,
                    end,
                },
            ));
        }
        if signed && self.at(TokenKind::Minus) && self.peek_kind(1) == Some(TokenKind::LParen) {
            self.bump();
            self.bump();
            if !self.take_int_atom(true) {
                return Some(self.finish_open_paren(open.start, inner_from));
            }
            if self
                .colon_between_tokens(self.i.wrapping_sub(1), self.i)
                .is_none()
            {
                self.record_syntax_at(self.i, Some("':'"));
                return Some(self.finish_open_paren(open.start, inner_from));
            }
            if !self.take_int_atom(true) {
                return Some(self.finish_open_paren(open.start, inner_from));
            }
            if !self.at(TokenKind::RParen) {
                self.record_syntax_at(self.i, Some("')'"));
                return Some(self.finish_open_paren(open.start, inner_from));
            }
            self.bump();
        } else if !self.take_paren_range(signed) {
            return Some(self.finish_open_paren(open.start, inner_from));
        }
        if !self.at(TokenKind::RParen) {
            if let Some(extra) = self.colon_between_tokens(self.i.wrapping_sub(1), self.i) {
                self.record_mom_syntax(extra, "syntax error, unexpected ':', expecting ')'");
            } else {
                self.record_syntax_at(self.i, Some("')'"));
            }
            return Some(self.finish_open_paren(open.start, inner_from));
        }
        let close = self.bump().span;
        let text = self
            .src
            .get(inner_from as usize..close.start as usize)
            .unwrap_or("")
            .trim()
            .to_string();
        Some((
            text,
            Span {
                start: open.start,
                end: close.end,
            },
        ))
    }

    /// One integer, or `int:int`, inside a paren. `false` records the error.
    fn take_paren_range(&mut self, signed: bool) -> bool {
        if !self.take_int_atom(signed) {
            return false;
        }
        let first_end = self.i.wrapping_sub(1);
        if self.colon_between_tokens(first_end, self.i).is_none() {
            return true;
        }
        if !self.take_int_atom(signed) {
            return false;
        }
        let second_end = self.i.wrapping_sub(1);
        if let Some(extra) = self.colon_between_tokens(second_end, self.i) {
            self.record_mom_syntax(extra, "syntax error, unexpected ':', expecting ')'");
            return false;
        }
        true
    }

    /// One `signed_integer` or bare `INT_NUMBER`. The bad token is consumed only
    /// when it is not the `)` that closes the group.
    fn take_int_atom(&mut self, signed: bool) -> bool {
        let expecting = if signed {
            "INT_NUMBER or PLUS or MINUS"
        } else {
            "INT_NUMBER"
        };
        if signed && (self.at(TokenKind::Plus) || self.at(TokenKind::Minus)) {
            self.bump();
            if self.at(TokenKind::Number) && is_integer_lexeme(self.lexeme(&self.tokens[self.i])) {
                self.bump();
                return true;
            }
            self.record_syntax_at(self.i, Some(expecting));
            if !self.at(TokenKind::RParen) && !self.at(TokenKind::Eof) {
                self.bump();
            }
            return false;
        }
        if self.at(TokenKind::Number) && is_integer_lexeme(self.lexeme(&self.tokens[self.i])) {
            if let Some(date) = self.glued_date_span(self.i) {
                self.record_mom_syntax(
                    date,
                    format!("syntax error, unexpected DATE, expecting {expecting}"),
                );
                self.i += 2;
                return false;
            }
            self.bump();
            return true;
        }
        self.record_syntax_at(self.i, Some(expecting));
        if !self.at(TokenKind::RParen) && !self.at(TokenKind::Eof) {
            self.bump();
        }
        false
    }

    /// Consume the rest of a `(` group already opened, through its `)`.
    fn finish_open_paren(&mut self, open_start: u32, inner_from: u32) -> (String, Span) {
        let mut inner_end = self
            .tokens
            .get(self.i.wrapping_sub(1))
            .map(|tok| tok.span.end)
            .unwrap_or(inner_from);
        // Parens already consumed, including a nested `-(…)` , still have to close.
        let mut depth = 0i32;
        for tok in self.tokens.iter().take(self.i) {
            if tok.span.start < open_start {
                continue;
            }
            match tok.kind {
                TokenKind::LParen => depth += 1,
                TokenKind::RParen => depth -= 1,
                _ => {}
            }
        }
        if depth < 1 {
            depth = 1;
        }
        while !self.at(TokenKind::Eof) {
            if self.at(TokenKind::LParen) {
                depth += 1;
                inner_end = self.bump().span.end;
                continue;
            }
            if self.at(TokenKind::RParen) {
                depth -= 1;
                let end = self.bump().span.end;
                if depth == 0 {
                    let text = self
                        .src
                        .get(inner_from as usize..inner_end as usize)
                        .unwrap_or("")
                        .trim()
                        .to_string();
                    return (
                        text,
                        Span {
                            start: open_start,
                            end,
                        },
                    );
                }
                inner_end = end;
                continue;
            }
            inner_end = self.tokens[self.i].span.end;
            self.bump();
        }
        (
            String::new(),
            Span {
                start: open_start,
                end: inner_end,
            },
        )
    }

    /// The block opener's keyword through its `(…)` and `;`, the single bare word
    /// its list holds (`None` when the list is absent or holds something else), and
    /// the body's token bounds.
    ///
    /// The `(…)` is one fixed token in the grammar (`OVERWRITE` or `RELATIVE_IRF`),
    /// not an option list. Any other word, and any `(…)` on `matched_moments` or
    /// `moment_calibration`, is the syntax error 7.1 prints on that token.
    fn bump_block_opener(&mut self, keyword: &str) -> (Span, Option<String>, usize, usize) {
        let start = self.bump().span.start;
        let mut word = None;
        if self.at(TokenKind::LParen) {
            let from = self.i;
            self.skip_balanced(TokenKind::LParen, TokenKind::RParen);
            let close_i = self.i;
            self.note_mom_opener(keyword, from, close_i);
            let inner = (from + 1)..close_i.saturating_sub(1);
            if inner.len() == 1 && self.tokens[inner.start].kind == TokenKind::Ident {
                word = Some(self.lexeme(&self.tokens[inner.start]).to_string());
            }
        }
        let end = if self.at(TokenKind::Semi) {
            self.bump().span.end
        } else {
            self.current_start()
        };
        let opener_span = Span { start, end };
        let body_i = self.i;
        let body_end_i = self.consume_until_end();
        self.record_missing_end_if_unclosed(keyword, opener_span, body_i, body_end_i);
        (opener_span, word, body_i, body_end_i)
    }

    /// A row's own `;`. If that `;` was already eaten (a `matched_irfs` keyword
    /// list), stay put so the next pair is still a row. Leftover tokens on this
    /// row still skip to that `;`.
    fn finish_row(&mut self, end_i: usize) -> u32 {
        if self.at(TokenKind::Semi) {
            return self.bump().span.end;
        }
        if let Some(prev) = self.i.checked_sub(1) {
            if self
                .tokens
                .get(prev)
                .is_some_and(|t| t.kind == TokenKind::Semi)
            {
                return self.tokens[prev].span.end;
            }
        }
        self.finish_shock_stmt(end_i)
    }

    /// Sims `bvar_density N;` / `bvar_forecast N;` / `bvar_irf(N, 'name');`
    fn parse_bvar_statement(&mut self) {
        self.model.bvar_present = true;
        self.skip_until_semi();
    }

    /// Command name for the `;` statements of the MS-SBVAR family, or `None`.
    ///
    /// The dotted statements are not here: their head is a symbol, not a command.
    /// A name followed by `=`, a bare name, or a name before another word is not a
    /// statement here ? 7.1 refuses those shapes, and they keep their existing paths.
    fn at_ms_family_command(&self) -> Option<&'static str> {
        const COMMANDS: &[&str] = &[
            "ms_estimation",
            "ms_simulation",
            "ms_compute_mdd",
            "ms_compute_probabilities",
            "ms_irf",
            "ms_forecast",
            "ms_variance_decomposition",
            "markov_switching",
            "svar",
            "sbvar",
            "svar_global_identification_check",
            "conditional_forecast",
            "plot_conditional_forecast",
        ];
        let command = COMMANDS.iter().copied().find(|cmd| self.at_ident_ci(cmd))?;
        match self.peek_kind(1) {
            Some(TokenKind::LParen) | Some(TokenKind::Semi) => Some(command),
            // `ms_irf y, c;` and `plot_conditional_forecast y;` take a symbol list.
            Some(TokenKind::Ident) if is_trailing_symbol_command(command) => Some(command),
            _ => None,
        }
    }

    /// One family `;` statement: its span, its option rows, and ? for `ms_irf` and
    /// `plot_conditional_forecast` ? the trailing symbol list.
    fn parse_ms_statement(&mut self) {
        let command = self
            .at_ms_family_command()
            .expect("caller checked the command")
            .to_string();
        let start = self.current_start();
        self.bump();
        let mut options = Vec::new();
        let mut option_span = None;
        if self.at(TokenKind::LParen) {
            let from = self.i;
            self.skip_balanced(TokenKind::LParen, TokenKind::RParen);
            let close_i = self.i;
            self.record_deprecated_options_in_range(from, close_i);
            self.record_skip_command_options(&command, from, close_i);
            options = self.read_family_options(from, close_i);
            self.record_parsed_option_twice(&options);
            self.record_option_shape_refuse(&command, &options, from);
            option_span = Some(from);
        }
        // The grammar's own shape for this command: the list it requires, the one
        // it forbids, and the trailing symbol list `plot_conditional_forecast`
        // needs. Each refusal points at the token 7.1's parser stops on.
        if option_span.is_some() && command.eq_ignore_ascii_case("svar_global_identification_check")
        {
            let tok = self.tokens[self.i - 1].span;
            self.model
                .shape_refuses
                .push(ShapeRefuse::new(tok, &command, "no option list"));
        }
        if option_span.is_none() && requires_option_list(&command) {
            let tok = self.tokens[self.i].span;
            self.model
                .shape_refuses
                .push(ShapeRefuse::new(tok, &command, "an option list"));
        }
        self.model.ms_statements.push(MsStatement {
            command: command.clone(),
            span: Span {
                start,
                end: self.current_start(),
            },
            options,
        });
        if is_trailing_symbol_command(&command) {
            self.collect_trailing_symbols(&command);
        }
        // `plot_conditional_forecast` needs a trailing symbol list: `(periods=N)`
        // alone is a syntax error, and so is a bare `;`. `ms_irf` parses in all
        // four forms, so it has no such rule.
        if requires_symbol_list(&command)
            && !self
                .model
                .command_symbols
                .iter()
                .any(|sym| sym.list_id == self.symbol_list_id)
        {
            let tok = self.tokens[self.i].span;
            self.model.shape_refuses.push(ShapeRefuse::new(
                tok,
                &command,
                "a list of endogenous names",
            ));
        }
        while !self.at(TokenKind::Semi) && !self.at(TokenKind::Eof) {
            self.bump();
        }
        let end = if self.at(TokenKind::Semi) {
            self.bump().span.end
        } else {
            self.current_start()
        };
        if let Some(stmt) = self.model.ms_statements.last_mut() {
            stmt.span.end = end;
        }
    }

    /// One `data(file=?);` statement. The 0.5.2 presence-only record becomes this
    /// one: the estimation gate reads `has_file_or_series` off the parsed rows.
    fn parse_data_statement(&mut self) {
        let start = self.current_start();
        self.bump();
        let mut options = Vec::new();
        let mut has_list = false;
        if self.at(TokenKind::LParen) {
            has_list = true;
            let from = self.i;
            self.skip_balanced(TokenKind::LParen, TokenKind::RParen);
            let close_i = self.i;
            self.record_deprecated_options_in_range(from, close_i);
            self.record_skip_command_options("data", from, close_i);
            options = self.read_family_options(from, close_i);
            self.record_parsed_option_twice(&options);
            self.record_option_shape_refuse("data", &options, from);
        }
        // `data;` — the grammar's `DATA '(' data_options_list ')' ';'` requires the
        // list, and `data()` needs at least one option in it.
        if !has_list {
            let tok = self.tokens[self.i].span;
            self.model
                .shape_refuses
                .push(ShapeRefuse::new(tok, "data", "an option list"));
        }
        let end = if self.at(TokenKind::Semi) {
            self.bump().span.end
        } else {
            self.current_start()
        };
        self.model.data_statements.push(DataStatement {
            span: Span { start, end },
            options,
        });
    }

    /// The dotted statement at the cursor, when every name in its head is declared.
    /// `zzz.prior(?)` on an undeclared head is a native MATLAB line at the pin
    /// (7.1 accepts it), so its span must not be claimed here.
    fn at_dotted_statement(&mut self) -> Option<(DottedKind, usize)> {
        let (_, body_at) = self.dotted_head_at()?;
        let kind = if self.at_ident_ci_at(body_at, "prior") {
            DottedKind::Prior
        } else if self.at_ident_ci_at(body_at, "options") {
            DottedKind::Options
        } else if self.at_ident_ci_at(body_at, "subsamples") {
            DottedKind::Subsamples
        } else {
            return None;
        };
        Some((kind, body_at))
    }

    fn at_ident_ci_at(&self, ahead: usize, name: &str) -> bool {
        self.tokens.get(self.i + ahead).is_some_and(|t| {
            t.kind == TokenKind::Ident && t.text(self.src).eq_ignore_ascii_case(name)
        })
    }

    fn kind_at(&self, ahead: usize) -> Option<TokenKind> {
        self.tokens.get(self.i + ahead).map(|t| t.kind)
    }

    /// The token after a keyword is one a statement may begin with: `(` or `;`.
    /// `data = 0.5;` is not a `data` statement — 7.1 reads `data` as a name there.
    fn at_command_shape(&self, ahead: usize) -> bool {
        matches!(
            self.kind_at(ahead),
            Some(TokenKind::LParen) | Some(TokenKind::Semi)
        )
    }

    /// `model = 0.2;` / `steady = 0.9;` at statement head. The `<INITIAL>` rule
    /// returns the keyword, so `=` is a syntax error, not an assignment.
    fn at_keyword_followed_by_eq(&self) -> bool {
        if self.peek_kind(1) != Some(TokenKind::Eq) {
            return false;
        }
        BLOCK_OPENERS.iter().any(|kw| self.at_ident_ci(kw))
            || INITIAL_COMMANDS.iter().any(|kw| self.at_ident_ci(kw))
    }

    /// A dotted head at the cursor plus the offset of the identifier after it.
    /// The leading plain name must be declared to enter the statement grammar.
    /// A later name can be a subsample label rather than a declared symbol.
    fn dotted_head_at(&mut self) -> Option<(DottedHead, usize)> {
        self.dotted_head_at_with_gate(true)
    }

    /// A copy's right-hand head is already inside the grammar. Its plain first
    /// name may be unknown, in which case the driver's type check refuses it.
    fn dotted_copy_head_at(&mut self, index: usize) -> Option<DottedHead> {
        let saved = self.i;
        self.i = index;
        let head = self.dotted_head_at_with_gate(false).map(|(head, _)| head);
        self.i = saved;
        head
    }

    fn dotted_head_at_with_gate(
        &mut self,
        require_declared_plain: bool,
    ) -> Option<(DottedHead, usize)> {
        if self.kind_at(0) == Some(TokenKind::LBrack) {
            let (names, after) = self.vector_head_names()?;
            return Some((DottedHead::Vec { names }, after));
        }
        if self.at_ident_ci_at(0, "std") && self.kind_at(1) == Some(TokenKind::LParen) {
            // `std` is a lexer keyword, so the statement is entered whatever the
            // name inside the parentheses is: an undeclared one is checked by the
            // grammar's own `symbol` production, not by the head rule.
            let (first, first_span) = self.ident_at_name(2)?;
            if self.kind_at(3) != Some(TokenKind::RParen) || self.kind_at(4) != Some(TokenKind::Dot)
            {
                return None;
            }
            if self.kind_at(5) == Some(TokenKind::Ident) && self.kind_at(6) == Some(TokenKind::Dot)
            {
                let (second, _) = self.ident_at_name(5)?;
                return Some((
                    DottedHead::Std {
                        first,
                        first_span,
                        second: Some(second),
                    },
                    7,
                ));
            }
            return Some((
                DottedHead::Std {
                    first,
                    first_span,
                    second: None,
                },
                5,
            ));
        }
        if self.at_ident_ci_at(0, "corr") && self.kind_at(1) == Some(TokenKind::LParen) {
            let (first, first_span) = self.ident_at_name(2)?;
            if self.kind_at(3) != Some(TokenKind::Comma) {
                return None;
            }
            let (second, second_span) = self.ident_at_name(4)?;
            if self.kind_at(5) != Some(TokenKind::RParen) || self.kind_at(6) != Some(TokenKind::Dot)
            {
                return None;
            }
            if self.kind_at(7) == Some(TokenKind::Ident) && self.kind_at(8) == Some(TokenKind::Dot)
            {
                let (third, _) = self.ident_at_name(7)?;
                return Some((
                    DottedHead::Corr {
                        first,
                        first_span,
                        second,
                        second_span,
                        third: Some(third),
                    },
                    9,
                ));
            }
            return Some((
                DottedHead::Corr {
                    first,
                    first_span,
                    second,
                    second_span,
                    third: None,
                },
                7,
            ));
        }
        let (first, _) = if require_declared_plain {
            self.declared_ident_at(0)?
        } else {
            self.ident_at_name(0)?
        };
        if self.kind_at(1) != Some(TokenKind::Dot) {
            return None;
        }
        if let Some((second, _)) = self.ident_at_name(2) {
            if self.kind_at(3) == Some(TokenKind::Dot) {
                return Some((
                    DottedHead::Param {
                        first,
                        second: Some(second),
                    },
                    4,
                ));
            }
        }
        Some((
            DottedHead::Param {
                first,
                second: None,
            },
            2,
        ))
    }

    /// An `[a, b]` head whose names are all declared, plus the offset after it.
    fn vector_head_names(&self) -> Option<(Vec<(Name, Span)>, usize)> {
        let mut names = Vec::new();
        let mut k = 1;
        loop {
            let (name, span) = self.declared_ident_at(k)?;
            names.push((name, span));
            k += 1;
            match self.kind_at(k) {
                // The pin's lexer spells `[a]` and `[a, b]` with the same rule, so a
                // one-name bracket is a `SYMBOL_VEC` too; the joint prior's own check
                // pass then refuses the count (`you must pass at least two ?`).
                Some(TokenKind::RBrack) if !names.is_empty() => {
                    if self.kind_at(k + 1) == Some(TokenKind::Dot) {
                        return Some((names, k + 2));
                    }
                    return None;
                }
                Some(TokenKind::Comma) => k += 1,
                _ => return None,
            }
        }
    }

    /// The symbol at a token offset when the pin's lexer would read it as a
    /// statement head, or `None`.
    ///
    /// 7.1's rule is `symbol_exists_and_is_not_modfile_local_or_external_function`:
    /// a name declared as `var` / `varexo` / `parameters` / `predetermined_variables`
    /// enters a Dynare statement, while a mod-file local (`#x = 1;`) or an
    /// `external_function` name sends the whole line to native MATLAB, where no
    /// language claim is made. This mirrors that rule, so `#x = 1;` followed by
    /// `x.prior(?)` is native text to both sides.
    fn declared_ident_at(&self, offset: usize) -> Option<(Name, Span)> {
        let tok = self.tokens.get(self.i + offset)?;
        if tok.kind != TokenKind::Ident {
            return None;
        }
        // During parsing the interner lives on the parser, not on the model.
        let name = self.intern.lookup(tok.text(self.src))?;
        if !self.is_statement_head_symbol(name) {
            return None;
        }
        Some((name, tok.span))
    }

    /// An identifier at a token offset with its span, whether or not it is
    /// declared. The `std` / `corr` heads carry their names through the grammar's
    /// own `symbol` production, so those names are read here and checked later.
    fn ident_at_name(&mut self, offset: usize) -> Option<(Name, Span)> {
        let tok = self.tokens.get(self.i + offset)?;
        if tok.kind != TokenKind::Ident {
            return None;
        }
        let name = self.intern.intern(tok.text(self.src));
        Some((name, tok.span))
    }

    /// The text between two token offsets, when it is exactly `s`.
    fn gap_is(&self, left: usize, right: usize, s: &str) -> bool {
        let (Some(a), Some(b)) = (self.tokens.get(left), self.tokens.get(right)) else {
            return false;
        };
        self.src
            .get(a.span.end as usize..b.span.start as usize)
            .is_some_and(|gap| gap == s)
    }

    /// Whether a token offset is an identifier at all (declaration not checked).
    fn ident_at(&self, offset: usize) -> bool {
        self.kind_at(offset) == Some(TokenKind::Ident)
    }

    /// The head names of the dotted shape at the cursor plus the offset of the
    /// identifier after the head, or `None` when the tokens are not that shape.
    ///
    /// Shape only: 7.1's lexer decides statement versus native line by looking at
    /// the head, and the grammar then keys the body on `prior` / `options` /
    /// `subsamples`, so this must not consult the symbol table.
    fn syntactic_dotted_head(&self) -> Option<(Vec<usize>, usize)> {
        // `[a, b].prior(?)` and longer vectors.
        if self.kind_at(0) == Some(TokenKind::LBrack) {
            let mut names = Vec::new();
            let mut k = 1;
            loop {
                if !self.ident_at(k) {
                    return None;
                }
                names.push(k);
                k += 1;
                match self.kind_at(k) {
                    // One name is enough for the shape: `[a].prior(?)` is the same
                    // `SYMBOL_VEC` production, and the joint prior's check pass
                    // refuses it for its count.
                    Some(TokenKind::RBrack) => {
                        if !names.is_empty()
                            && self.kind_at(k + 1) == Some(TokenKind::Dot)
                            && self.ident_at(k + 2)
                        {
                            return Some((names, k + 2));
                        }
                        return None;
                    }
                    Some(TokenKind::Comma) => k += 1,
                    _ => return None,
                }
            }
        }
        // `std(x).prior(?)` and `corr(x, y).prior(?)`. Their names are checked by the
        // grammar's own `symbol` production, so the head is always a statement.
        let mut head_names = Vec::new();
        for (keyword, arity) in [("std", 1usize), ("corr", 2usize)] {
            if !self.at_ident_ci_at(0, keyword) || self.kind_at(1) != Some(TokenKind::LParen) {
                continue;
            }
            let mut k = 2;
            for arg in 0..arity {
                if arg > 0 && self.kind_at(k) == Some(TokenKind::Comma) {
                    k += 1;
                }
                if !self.ident_at(k) {
                    return None;
                }
                head_names.push(k);
                k += 1;
            }
            if self.kind_at(k) != Some(TokenKind::RParen)
                || self.kind_at(k + 1) != Some(TokenKind::Dot)
            {
                return None;
            }
            // These two are keywords, so they never go native.
            if self.ident_at(k + 2) {
                return Some((Vec::new(), k + 4));
            }
            return Some((Vec::new(), k + 2));
        }
        // `alpha.prior(?)` and `alpha.beta.prior(?)`.
        if self.ident_at(0) && self.kind_at(1) == Some(TokenKind::Dot) {
            if self.ident_at(2) && self.kind_at(3) == Some(TokenKind::Dot) {
                return Some((vec![0], 4));
            }
            return Some((vec![0], 2));
        }
        None
    }

    /// Whether every head name at these token offsets is declared as a statement
    /// head. An empty list means the head is a keyword (`std` / `corr`), which the
    /// grammar always reads as a statement.
    fn dotted_head_is_statement(&self, names: &[usize]) -> bool {
        names.iter().all(|&k| {
            self.ident_at(k)
                && self
                    .intern
                    .lookup(self.tokens[self.i + k].text(self.src))
                    .is_some_and(|name| self.is_statement_head_symbol(name))
        })
    }

    /// The end offset of a top-level statement 7.1 reads as native MATLAB text: it
    /// makes no language claim on the line, so neither may we beyond leaving it be.
    ///
    /// Two shapes reach here. A dotted head whose names fail the pin's declaration
    /// rule (`zzz.prior(?)`, `[aaa, bbb].prior(?)`, a mod-file local or an
    /// external-function name as the head), and an identifier that is not one of the
    /// pin's statement keywords followed by `(`. Both are 7.1-accepted however their
    /// contents read. A head that passes the rule stays a Dynare statement, so the
    /// grammar's own refusals on it keep their existing paths.
    fn native_statement_end(&self) -> Option<usize> {
        let native_dotted = self.syntactic_dotted_head().is_some_and(|(names, body)| {
            self.kind_at(body + 1) == Some(TokenKind::LParen)
                && !self.dotted_head_is_statement(&names)
        });
        let head = &self.tokens[self.i];
        let non_keyword_call = self.ident_at(0)
            && self.kind_at(1) == Some(TokenKind::LParen)
            && !crate::command_skip::is_pin_statement_keyword(head.text(self.src))
            && self
                .intern
                .lookup(head.text(self.src))
                .is_none_or(|name| !self.is_statement_head_symbol(name));
        if !native_dotted && !non_keyword_call {
            return None;
        }
        // The statement ends at its `;`, or at the end of the parenthesised group.
        let mut k = 0;
        while let Some(kind) = self.kind_at(k) {
            match kind {
                TokenKind::Semi => return Some(k),
                TokenKind::LParen | TokenKind::LBrack => {
                    let close = if kind == TokenKind::LParen {
                        TokenKind::RParen
                    } else {
                        TokenKind::RBrack
                    };
                    k = skip_balanced_tokens(&self.tokens, self.i + k, kind, close) - self.i;
                    continue;
                }
                TokenKind::Eof => return None,
                _ => k += 1,
            }
        }
        None
    }

    /// Claim a native statement's span and step over it, recording no row.
    fn skip_native_statement(&mut self, end: usize) {
        let start = self.tokens[self.i].span.start;
        let end_span = if self.kind_at(end) == Some(TokenKind::Semi) {
            self.tokens[self.i + end].span.end
        } else {
            self.tokens
                .get(self.i + end.saturating_sub(1))
                .map(|t| t.span.end)
                .unwrap_or(start)
        };
        self.model.ms_unparsed_spans.push(Span {
            start,
            end: end_span,
        });
        self.i = (self.i + end + 1).min(self.tokens.len().saturating_sub(1));
        if self.tokens[self.i].kind == TokenKind::Semi {
            self.bump();
        }
    }

    /// Whether the cursor is where 7.1's lexer would be in `INITIAL`, i.e. at the
    /// start of a statement rather than inside one.
    ///
    /// The pin's lexer enters a statement only from `INITIAL`, and it returns to
    /// `INITIAL` in exactly two ways: `<DYNARE_STATEMENT>;` after a statement's own
    /// `;`, and the `\n` that ends a `NATIVE` block. Everything else on a line that
    /// began as native MATLAB text is still native — so `zz = 1; data(nobs=1);` is
    /// one native line to 7.1 and never starts a `data` statement, while
    /// `data(file='x.csv'); data(nobs=1);` does.
    ///
    /// Reading a mid-line keyword as a statement is what put an Error on files 7.1
    /// accepts (`w = 1./xx.data(2,3);`, whose `data` fires only in `INITIAL`).
    fn at_statement_boundary(&self) -> bool {
        let line_start = self.line_first_token(self.i);
        if line_start == self.i {
            return true;
        }
        // Mid-line: only a `;` that closed a Dynare *statement* returns to `INITIAL`.
        self.tokens[self.i - 1].kind == TokenKind::Semi && self.line_began_a_statement(line_start)
    }

    /// The index of the first token on the line that contains token `at`.
    fn line_first_token(&self, at: usize) -> usize {
        let mut k = at;
        while k > 0 {
            let prev = &self.tokens[k - 1];
            let gap = self
                .src
                .get(prev.span.end as usize..self.tokens[k].span.start as usize)
                .unwrap_or("");
            if gap.contains('\n') {
                break;
            }
            k -= 1;
        }
        k
    }

    /// Whether the line that starts at token `head` began a Dynare statement or
    /// block, so every `;` after it returns the lexer to `INITIAL`. A line that
    /// began as native MATLAB text never does: the whole line stays native.
    fn line_began_a_statement(&self, head: usize) -> bool {
        let Some(tok) = self.tokens.get(head) else {
            return false;
        };
        if tok.kind != TokenKind::Ident {
            return false;
        }
        let lex = tok.text(self.src);
        if lex.eq_ignore_ascii_case("end")
            || crate::command_skip::is_pin_statement_keyword(lex)
            || self.at_policy_command_name(lex)
        {
            return true;
        }
        // A declared head: the dotted statements and a top-level assignment.
        self.intern
            .lookup(lex)
            .is_some_and(|name| self.is_statement_head_symbol(name))
    }

    fn at_policy_command_name(&self, lex: &str) -> bool {
        [
            "ramsey_model",
            "ramsey_policy",
            "discretionary_policy",
            "osr",
        ]
        .iter()
        .any(|c| lex.eq_ignore_ascii_case(c))
    }

    /// Claim the span of a handed-over shape and record its refuse. The statement
    /// is 7.1-refused, so nothing reads it further; the span is claimed all the
    /// same so the text-level passes leave its contents alone.
    fn parse_handed_over_statement(&mut self) {
        let start = self.tokens[self.i].span.start;
        if let Some(refuse) = self.handed_over_refusal() {
            self.model.shape_refuses.push(refuse);
        }
        while !self.at(TokenKind::Semi) && !self.at(TokenKind::Eof) {
            if self.at(TokenKind::LParen) {
                self.skip_balanced(TokenKind::LParen, TokenKind::RParen);
                continue;
            }
            self.bump();
        }
        let end = if self.at(TokenKind::Semi) {
            self.bump().span.end
        } else {
            self.current_start()
        };
        self.model.ms_unparsed_spans.push(Span { start, end });
    }

    /// A statement whose head is one of the pin's own keywords but whose shape the
    /// grammar has no production for, or a declared head with a body the grammar
    /// cannot put on it.
    ///
    /// These are parse syntax errors in 7.1, not native lines: the pin's lexer
    /// enters the statement on the keyword (`dsample`, `rplot`,
    /// `smoother2histval`, `var_remove`, `database`) or on the declared head (a
    /// name before `(` or `;`, a dotted head whose tail word is not `prior` /
    /// `options` / `subsamples`), and the grammar then refuses what it reads.
    ///
    /// A statement whose *legal* shape is untouched matches nothing here: `rplot y;`,
    /// `var_remove alpha;`, `dsample 10 20;`, `smoother2histval;` and
    /// `database myfile;` are all legal, and a command whose option list repeats a
    /// name keeps its own **E271** rather than gaining this refuse.
    fn at_handed_over_statement(&self) -> bool {
        self.handed_over_refusal().is_some()
    }

    /// The refuse one of those heads earns, or `None` when the shape is one the
    /// grammar does accept.
    fn handed_over_refusal(&self) -> Option<ShapeRefuse> {
        let head = &self.tokens[self.i];
        let lex = head.text(self.src);
        if lex.eq_ignore_ascii_case("dsample") {
            let legal = self.kind_at(1) == Some(TokenKind::Number)
                && (self.kind_at(2) == Some(TokenKind::Semi)
                    || (self.kind_at(2) == Some(TokenKind::Number)
                        && self.kind_at(3) == Some(TokenKind::Semi)));
            return (!legal).then(|| ShapeRefuse::new(head.span, lex, "one or two integers"));
        }
        if lex.eq_ignore_ascii_case("rplot") || lex.eq_ignore_ascii_case("var_remove") {
            if self.kind_at(1) == Some(TokenKind::Ident) && !self.at_repeated_option_list() {
                return None;
            }
            // `rplot(periods=10, periods=20);` keeps its **E271**; every other
            // non-list shape is the grammar refusing a missing symbol list.
            if self.at_repeated_option_list() {
                return None;
            }
            return Some(ShapeRefuse::new(head.span, lex, "a list of symbols"));
        }
        if lex.eq_ignore_ascii_case("squeeze_shock_decomposition") {
            return (self.kind_at(1) == Some(TokenKind::LParen)).then(|| {
                ShapeRefuse::official(
                    self.tokens[self.i + 1].span,
                    lex,
                    "syntax error, unexpected '('",
                )
            });
        }
        if lex.eq_ignore_ascii_case("dynasave") || lex.eq_ignore_ascii_case("dynatype") {
            if self.kind_at(1) != Some(TokenKind::LParen) {
                let next = self
                    .tokens
                    .get(self.i + 1)
                    .map(|tok| tok.span)
                    .unwrap_or(head.span);
                return Some(ShapeRefuse::official(
                    next,
                    lex,
                    "syntax error, unexpected IDENTIFIER, expecting '('",
                ));
            }
            let value = self.kind_at(2);
            if !matches!(value, Some(TokenKind::String | TokenKind::Ident)) {
                return Some(ShapeRefuse::new(
                    self.tokens[self.i + 2].span,
                    lex,
                    "a filename",
                ));
            }
            if self.kind_at(3) != Some(TokenKind::RParen) {
                let token = &self.tokens[self.i + 3];
                return Some(if token.kind == TokenKind::Comma {
                    ShapeRefuse::official(
                        token.span,
                        lex,
                        "syntax error, unexpected COMMA, expecting ')'",
                    )
                } else {
                    ShapeRefuse::new(token.span, lex, "one filename")
                });
            }
            return None;
        }
        if lex.eq_ignore_ascii_case("smoother2histval") {
            if self.kind_at(1) == Some(TokenKind::Semi) || self.at_repeated_option_list() {
                return None;
            }
            if self.kind_at(1) != Some(TokenKind::LParen) {
                return Some(ShapeRefuse::new(
                    head.span,
                    lex,
                    "invars, outvars, outfile or period options",
                ));
            }
            return self.option_list_refusal_at(1, lex);
        }
        if lex.eq_ignore_ascii_case("database") {
            if self.kind_at(1) == Some(TokenKind::Ident) {
                return None;
            }
            return Some(ShapeRefuse::new(head.span, lex, "a list of symbols"));
        }
        if !self.declared_spelling(lex) {
            return None;
        }
        // A block opener keeps the block reading at the head of a statement: the
        // pin's opener rules are `<INITIAL>`-scoped, so flex never considers them
        // inside a body and the symbol table never gets a say in `INITIAL`.
        // `priors;` is the `priors` block even when the file also declares a
        // variable `priors`.
        if BLOCK_OPENERS.iter().any(|kw| lex.eq_ignore_ascii_case(kw)) {
            return None;
        }
        self.declared_head_refusal(head.span, lex)
    }

    /// The option-list refuse for the list that opens `open` tokens ahead of the
    /// cursor, or `None` when every option is one the named command's production
    /// carries. An empty list has no production either.
    fn option_list_refusal_at(&self, open: usize, subject: &str) -> Option<ShapeRefuse> {
        let close = skip_balanced_tokens(
            &self.tokens,
            self.i + open,
            TokenKind::LParen,
            TokenKind::RParen,
        );
        let options = self.read_family_options(self.i + open, close);
        if options.is_empty() {
            return Some(ShapeRefuse::new(
                self.tokens[self.i + open].span,
                subject,
                "at least one option",
            ));
        }
        let table = crate::shape_gate::command_options(subject)?;
        crate::shape_gate::option_refusal(self.src, subject, &options, table)
    }

    /// Whether the `(…)` at the cursor holds an option name twice. Such a list is
    /// **E271**'s, not this sweep's, so it must be left alone here.
    fn at_repeated_option_list(&self) -> bool {
        if self.kind_at(1) != Some(TokenKind::LParen) {
            return false;
        }
        let close = skip_balanced_tokens(
            &self.tokens,
            self.i + 1,
            TokenKind::LParen,
            TokenKind::RParen,
        );
        let options = self.read_family_options(self.i + 1, close);
        let mut seen: HashMap<String, ()> = HashMap::new();
        options
            .iter()
            .any(|opt| seen.insert(opt.name.to_ascii_lowercase(), ()).is_some())
    }

    /// A declared name the pin's lexer sends to a Dynare statement. The lexer
    /// decides as it reads, so the declaration must start before this line.
    fn declared_spelling(&self, spelling: &str) -> bool {
        self.declared_before(self.tokens[self.i].span.start, spelling)
    }

    /// `alpha;`, `y(1) = 2;`, `alpha.foo(…)`, `alpha.foo.bar(…)`, `alpha.foo = 1;`:
    /// a declared head followed by a token whose production the grammar has no
    /// rule for.
    fn declared_head_refusal(&self, head_span: Span, first: &str) -> Option<ShapeRefuse> {
        match self.kind_at(1) {
            // `alpha;` — the grammar wants `EQUAL` or a `.`-tail.
            Some(TokenKind::Semi) => {
                Some(ShapeRefuse::new(head_span, first, "EQUAL or a dotted tail"))
            }
            // `y(1) = 2;` — the grammar's top-level `symbol` takes no arguments.
            Some(TokenKind::LParen) => Some(ShapeRefuse::new(head_span, first, "EQUAL or '.'")),
            Some(TokenKind::Dot) => self.dotted_tail_refusal(head_span, first),
            _ => None,
        }
    }

    /// `alpha.foo(…)`, `alpha.foo.bar(…)`, `alpha.foo = 1;`, `alpha.foo;`. The
    /// grammar's dotted productions are keyed on `prior` / `options` /
    /// `subsamples` only, and a two-level head takes `prior` / `options` after the
    /// middle word.
    fn dotted_tail_refusal(&self, head_span: Span, first: &str) -> Option<ShapeRefuse> {
        if self.kind_at(2) != Some(TokenKind::Ident) {
            return None;
        }
        let tail = self.tokens[self.i + 2].text(self.src);
        if !is_dotted_body_word(tail) {
            // `alpha.foo = 1;` names the second word; `alpha.foo(…)` and
            // `alpha.foo;` are the dot's own token.
            return Some(if self.kind_at(3) == Some(TokenKind::Eq) {
                ShapeRefuse::new(self.tokens[self.i + 2].span, first, "'.'")
            } else {
                ShapeRefuse::new(self.tokens[self.i + 1].span, first, "'.'")
            });
        }
        if self.kind_at(3) != Some(TokenKind::Dot) {
            // `alpha.prior;`, `alpha.prior = beta.prior;` — the copy form needs a
            // right-hand head, and a bare `;` has none. `at_dotted_statement` has
            // already claimed the legal body forms.
            return Some(ShapeRefuse::new(head_span, first, "EQUAL or '.'"));
        }
        if self.kind_at(4) != Some(TokenKind::Ident) {
            return None;
        }
        let inner = self.tokens[self.i + 4].text(self.src);
        if !is_dotted_body_word(inner) {
            return Some(ShapeRefuse::new(
                self.tokens[self.i + 4].span,
                first,
                "OPTIONS or PRIOR",
            ));
        }
        None
    }

    /// The declaration of `name`, when the parser has one: `var` / `varexo` /
    /// `varexo_det` / `parameters` / `predetermined_variables`.
    ///
    /// Shared by the two questions below, so a declaration class added here
    /// reaches both.
    fn declaration_of(&self, name: Name) -> Option<&Decl> {
        self.model
            .endogenous
            .iter()
            .chain(&self.model.exogenous)
            .chain(&self.model.deterministic_exogenous)
            .chain(&self.model.parameters)
            .chain(&self.model.predetermined)
            .find(|d| d.name == name)
    }

    /// A declared name that enters a Dynare statement: declared, and neither a
    /// mod-file local nor an external-function name. This reads the **finished**
    /// table, which is what every site that decides `local` vs `extern` needs.
    fn is_statement_head_symbol(&self, name: Name) -> bool {
        self.is_statement_head_at(name, None)
    }

    /// The same question read **as of** a byte position: the head is declared by a
    /// declaration that starts before `at`.
    ///
    /// The pin's lexer decides as it reads, not off the finished symbol table, so
    /// `gg = 1` followed by `parameters gg hh;` is native text to 7.1 even though
    /// `gg` ends up declared.
    ///
    /// **Only `record_missing_assign_semis` uses this form.** It is the one pass
    /// that asks per line after the file is parsed, so it has to reconstruct the
    /// position the lexer was at. Every other site — the dotted-statement head
    /// rule, `native_statement_end`, `at_statement_boundary`,
    /// `sweep_undeclared_dotted_heads` — reads the finished table through
    /// `is_statement_head_symbol`, because it runs while the cursor is already at
    /// the site and the lexer's own position is the cursor.
    fn declared_before(&self, at: u32, spelling: &str) -> bool {
        let Some(name) = self.intern.lookup(spelling) else {
            return false;
        };
        self.is_statement_head_at(name, Some(at))
    }

    /// `declaration_of` plus the local / external exclusions and the optional
    /// as-of position.
    fn is_statement_head_at(&self, name: Name, before: Option<u32>) -> bool {
        if self.model.mod_file_locals.contains(&name)
            || self.model.external_function_names.contains(&name)
        {
            return false;
        }
        self.declaration_of(name)
            .is_some_and(|d| before.is_none_or(|at| d.span.start < at))
    }

    /// One dotted statement. A `prior` body is read; an `options` / `subsamples`
    /// body is claimed but not read, so its contents never reach a check.
    fn parse_dotted_statement(&mut self) {
        let (kind, body_at) = self
            .at_dotted_statement()
            .expect("caller checked the dotted head");
        let (head, _) = self
            .dotted_head_at()
            .expect("caller checked the dotted head");
        let start = self.tokens[self.i].span.start;
        let open = body_at + 1;
        let has_body = self.kind_at(open) == Some(TokenKind::LParen);
        let copy_source = if self.kind_at(open) == Some(TokenKind::Eq) {
            self.dotted_copy_head_at(self.i + open + 1)
        } else {
            None
        };
        let mut k = body_at;
        let mut options = Vec::new();
        if self.kind_at(open) == Some(TokenKind::LParen) {
            let close = skip_balanced_tokens(
                &self.tokens,
                self.i + open,
                TokenKind::LParen,
                TokenKind::RParen,
            );
            self.record_deprecated_options_in_range(self.i + open, close);
            if kind == DottedKind::Prior {
                options = self.read_family_options(self.i + open, close);
                self.record_parsed_option_twice(&options);
                if options.is_empty() {
                    self.model.shape_refuses.push(ShapeRefuse::official(
                        self.tokens[close.saturating_sub(1)].span,
                        "prior",
                        "syntax error, unexpected ')'",
                    ));
                }
                let joint = matches!(head, DottedHead::Vec { .. });
                let table = crate::shape_gate::prior_options(joint);
                let subject = if joint { "[…].prior" } else { "prior" };
                if let Some(refuse) =
                    crate::shape_gate::option_refusal(self.src, subject, &options, table)
                {
                    self.model.shape_refuses.push(refuse);
                }
            } else if kind == DottedKind::Options {
                options = self.read_family_options(self.i + open, close);
                self.record_parsed_option_twice(&options);
                if options.is_empty() {
                    self.model.shape_refuses.push(ShapeRefuse::official(
                        self.tokens[close.saturating_sub(1)].span,
                        "options",
                        "syntax error, unexpected ')'",
                    ));
                }
                if let Some(refuse) = crate::shape_gate::option_refusal(
                    self.src,
                    "options",
                    &options,
                    crate::shape_gate::dotted_options(),
                ) {
                    if let Some(opt) = options
                        .iter()
                        .find(|opt| opt.name.eq_ignore_ascii_case("overwrite"))
                    {
                        self.model.shape_refuses.push(ShapeRefuse::official(
                            opt.span,
                            "options",
                            "syntax error, unexpected IDENTIFIER, expecting BOUNDS or JSCALE or INIT",
                        ));
                    } else {
                        self.model.shape_refuses.push(refuse);
                    }
                }
            }
            k = close - self.i;
        }
        while matches!(
            self.kind_at(k),
            Some(kind) if kind != TokenKind::Semi && kind != TokenKind::Eof
        ) {
            k += 1;
            if self.i + k >= self.tokens.len() {
                break;
            }
        }
        let end = if self.kind_at(k) == Some(TokenKind::Semi) {
            let end = self.tokens[self.i + k].span.end;
            k += 1;
            end
        } else {
            self.tokens
                .get(self.i + k.saturating_sub(1))
                .map(|t| t.span.end)
                .unwrap_or_else(|| self.src.len() as u32)
        };
        if kind == DottedKind::Subsamples {
            self.collect_subsample_statement(self.i, (self.i + k).min(self.tokens.len()));
        }
        self.i = (self.i + k).min(self.tokens.len().saturating_sub(1));
        self.model.dotted_statements.push(DottedStatement {
            kind,
            head,
            span: Span { start, end },
            has_body,
            copy_source,
            options,
        });
    }

    /// Every option row of one `(?)` list, with each value's shape.
    fn read_family_options(&self, open_i: usize, close_i: usize) -> Vec<FamilyOption> {
        self.read_options_in_range(open_i + 1, close_i.saturating_sub(1))
    }

    fn read_options_in_range(&self, from: usize, to: usize) -> Vec<FamilyOption> {
        let end = to.min(self.tokens.len());
        let mut out = Vec::new();
        let mut i = from;
        let mut depth: i32 = 0;
        while i < end {
            match self.tokens[i].kind {
                TokenKind::LParen | TokenKind::LBrack => {
                    depth += 1;
                    i += 1;
                }
                TokenKind::RParen | TokenKind::RBrack => {
                    depth = depth.saturating_sub(1);
                    i += 1;
                }
                TokenKind::Ident if depth == 0 => {
                    let name = self.tokens[i].text(self.src).to_string();
                    let span = self.tokens[i].span;
                    i += 1;
                    let mut opt = FamilyOption {
                        name,
                        span,
                        has_value: false,
                        value_kind: FamilyValueKind::Flag,
                        value_span: span,
                        value_text: String::new(),
                        names: Vec::new(),
                    };
                    if i < end && self.tokens[i].kind == TokenKind::Eq {
                        opt.has_value = true;
                        i += 1;
                        let value = self.read_family_value(i, end, span);
                        opt.value_kind = value.kind;
                        opt.value_span = value.span;
                        opt.value_text = value.text;
                        opt.names = value.names;
                        i = value.next;
                    }
                    out.push(opt);
                }
                _ => i += 1,
            }
        }
        out
    }

    /// One option value, from the token just after `=`. `name_span` is the option
    /// name's span, used when the `=` has no value at all.
    fn read_family_value(&self, from: usize, end: usize, name_span: Span) -> FamilyValue {
        if from >= end {
            return FamilyValue::empty(name_span, from);
        }
        let first = &self.tokens[from];
        if let Some((date, next)) = self.date_at(from) {
            return FamilyValue {
                kind: FamilyValueKind::Date,
                span: date.span,
                text: date.text,
                names: Vec::new(),
                next,
            };
        }
        // `A1:B10`: 7.1's `range` production reads the two halves as one value, so
        // the tail is part of this option, not a second option row. The lexer drops
        // the `:`, leaving two adjacent identifiers.
        if first.kind == TokenKind::Ident
            && self.tokens.get(from + 1).map(|t| t.kind) == Some(TokenKind::Ident)
            && self.gap_is(from, from + 1, ":")
        {
            let tail = &self.tokens[from + 1];
            let span = Span {
                start: first.span.start,
                end: tail.span.end,
            };
            return FamilyValue {
                kind: FamilyValueKind::Range,
                span,
                text: self.src[span.start as usize..span.end as usize].to_string(),
                names: Vec::new(),
                next: from + 2,
            };
        }
        if first.kind == TokenKind::LBrack || first.kind == TokenKind::LParen {
            let (open, close) = if first.kind == TokenKind::LBrack {
                (TokenKind::LBrack, TokenKind::RBrack)
            } else {
                (TokenKind::LParen, TokenKind::RParen)
            };
            let stop = skip_balanced_tokens(&self.tokens, from, open, close).min(end);
            let inner = (from + 1, stop.saturating_sub(1));
            let kind = self.bracket_value_kind(inner.0, inner.1);
            let (text, names) = if kind == FamilyValueKind::NameList {
                self.name_list_value(inner.0, inner.1)
            } else {
                (join_lexemes(self.src, &self.tokens[from..stop]), Vec::new())
            };
            let span = Span {
                start: first.span.start,
                end: self.tokens[stop.saturating_sub(1)].span.end,
            };
            return FamilyValue {
                kind,
                span,
                text,
                names,
                next: stop,
            };
        }
        if first.kind == TokenKind::Number {
            if let Some(suffix) = self
                .tokens
                .get(from + 1)
                .filter(|t| t.kind == TokenKind::Ident && t.span.start == first.span.end)
            {
                // `1959Q1` and friends: the lexer splits the number from its suffix.
                let span = Span {
                    start: first.span.start,
                    end: suffix.span.end,
                };
                let text = self.src[span.start as usize..span.end as usize].to_string();
                return FamilyValue {
                    kind: FamilyValueKind::Date,
                    span,
                    text,
                    names: Vec::new(),
                    next: from + 2,
                };
            }
            return FamilyValue {
                kind: FamilyValueKind::Scalar,
                span: first.span,
                text: first.text(self.src).to_string(),
                names: Vec::new(),
                next: from + 1,
            };
        }
        if first.kind == TokenKind::Minus || first.kind == TokenKind::Plus {
            let mut stop = from + 1;
            while stop < end
                && matches!(
                    self.tokens[stop].kind,
                    TokenKind::Number | TokenKind::Minus | TokenKind::Plus
                )
            {
                stop += 1;
            }
            let span = Span {
                start: first.span.start,
                end: self.tokens[stop - 1].span.end,
            };
            return FamilyValue {
                kind: FamilyValueKind::Scalar,
                span,
                text: join_lexemes(self.src, &self.tokens[from..stop]),
                names: Vec::new(),
                next: stop,
            };
        }
        FamilyValue {
            kind: FamilyValueKind::Scalar,
            span: first.span,
            text: first.text(self.src).to_string(),
            names: Vec::new(),
            next: from + 1,
        }
    }

    /// `NameList` when every top-level element of a bracketed value is an
    /// identifier, `Matrix` when a bracket sits inside it, `Vector` otherwise.
    fn bracket_value_kind(&self, from: usize, to: usize) -> FamilyValueKind {
        let mut depth: i32 = 0;
        let mut saw_inner_bracket = false;
        let mut saw_element = false;
        let mut all_idents = true;
        for k in from..to.min(self.tokens.len()) {
            match self.tokens[k].kind {
                TokenKind::LBrack => {
                    saw_inner_bracket = true;
                    depth += 1;
                }
                TokenKind::LParen => depth += 1,
                TokenKind::RParen | TokenKind::RBrack => depth -= 1,
                TokenKind::Comma => {}
                TokenKind::Ident if depth == 0 => saw_element = true,
                _ if depth == 0 => all_idents = false,
                _ => {}
            }
        }
        if saw_inner_bracket {
            FamilyValueKind::Matrix
        } else if saw_element && all_idents {
            FamilyValueKind::NameList
        } else {
            FamilyValueKind::Vector
        }
    }

    fn name_list_value(&self, from: usize, to: usize) -> (String, Vec<(Name, Span)>) {
        let mut names = Vec::new();
        let mut text = String::new();
        for k in from..to.min(self.tokens.len()) {
            let tok = &self.tokens[k];
            if tok.kind != TokenKind::Ident {
                continue;
            }
            let lex = tok.text(self.src);
            if !text.is_empty() {
                text.push(' ');
            }
            text.push_str(lex);
            if let Some(name) = self.intern.lookup(lex) {
                names.push((name, tok.span));
            }
        }
        (text, names)
    }

    /// `svar_identification;` ? `end;`. The body keeps structured rows, mirroring
    /// the pin's `SvarIdentificationStatement`.
    fn parse_svar_identification_block(&mut self) {
        let opener_span = self.bump_plain_opener();
        let body_i = self.i;
        let body_end_i = self.consume_until_end();
        self.record_missing_end_if_unclosed("svar_identification", opener_span, body_i, body_end_i);
        let end = self.block_end_after_consume();
        let saved = self.i;
        self.i = body_i;
        let mut elements = Vec::new();
        let mut refused_while_reading: Vec<ShapeRefuse> = Vec::new();
        while self.i < body_end_i && !self.at(TokenKind::Eof) {
            let before = self.i;
            self.read_svar_identification_element(&mut elements, &mut refused_while_reading);
            if self.i <= before {
                self.bump();
            }
        }
        self.i = saved;
        // The grammar's own shape: the body is a list of elements, each
        // `exclusion lag N;` must be followed by at least one `equation` row, and an
        // `equation` row belongs to the lag above it. A body with no elements at all
        // is refused at the `end;` that closes it. A lag this body closed without an
        // `equation` row is a syntax error too.
        let mut shape_refuses = refused_while_reading;
        if elements.is_empty() {
            let end_tok = self.tokens[body_end_i.min(self.tokens.len() - 1)].span;
            shape_refuses.push(ShapeRefuse::new(
                end_tok,
                "svar_identification",
                "an exclusion, cholesky or restriction row",
            ));
        }
        for element in &elements {
            if let SvarIdentificationElement::ExclusionLag {
                lag: Some(_),
                span,
                equations,
            } = element
            {
                if equations.is_empty() {
                    shape_refuses.push(ShapeRefuse::new(*span, "exclusion lag", "an equation row"));
                }
            }
        }
        self.model.svar_identifications.push(SvarIdentification {
            span: Span {
                start: opener_span.start,
                end,
            },
            elements,
            shape_refuses,
        });
    }

    /// One element of an `svar_identification` body, appended to `out`.
    fn read_svar_identification_element(
        &mut self,
        out: &mut Vec<SvarIdentificationElement>,
        refuses: &mut Vec<ShapeRefuse>,
    ) {
        let start = self.current_start();
        if self.at_ident_ci("exclusion") {
            self.bump();
            if self.at_ident_ci("lag") {
                self.bump();
                // `exclusion lag N;` — the lag is an unsigned integer, and the
                // grammar needs at least one `equation` row before the element is
                // combined.
                let lag_span = self.tokens[self.i].span;
                let lag = self.take_int();
                if lag.is_none() && !self.at_ident_ci("lag") {
                    refuses.push(ShapeRefuse::new(
                        lag_span,
                        "exclusion lag",
                        "a non-negative integer",
                    ));
                }
                let span = self.finish_family_element(start);
                out.push(SvarIdentificationElement::ExclusionLag {
                    lag,
                    span,
                    equations: Vec::new(),
                });
            } else {
                self.bump_until_semi();
                let span = self.finish_family_element(start);
                out.push(SvarIdentificationElement::ExclusionConstants { span });
            }
            return;
        }
        if self.at_ident_ci("upper_cholesky") || self.at_ident_ci("lower_cholesky") {
            let upper = self.at_ident_ci("upper_cholesky");
            self.bump();
            let span = self.finish_family_element(start);
            out.push(if upper {
                SvarIdentificationElement::UpperCholesky { span }
            } else {
                SvarIdentificationElement::LowerCholesky { span }
            });
            return;
        }
        if self.at_ident_ci("equation") {
            let row = self.read_svar_equation();
            match out.last_mut() {
                Some(SvarIdentificationElement::ExclusionLag { equations, .. }) => {
                    equations.push(row)
                }
                _ => {
                    // `equation N, …;` with no `exclusion lag` above it: the
                    // grammar's list has no production for a leading row.
                    refuses.push(ShapeRefuse::new(
                        row.span,
                        "svar_identification",
                        "a row of the block's own list",
                    ));
                    out.push(SvarIdentificationElement::ExclusionLag {
                        lag: None,
                        span: row.span,
                        equations: vec![row],
                    });
                }
            }
            return;
        }
        if self.at_ident_ci("restriction") {
            self.bump();
            if self.at_ident_ci("equation") {
                self.bump();
            }
            let number = self.take_int();
            if self.at(TokenKind::Comma) {
                self.bump();
            }
            let expr_start = self.current_start();
            let mut expr_end = expr_start;
            while !self.at(TokenKind::Semi) && !self.at(TokenKind::Eof) && !self.at(TokenKind::Eq) {
                if self.at(TokenKind::LParen) {
                    expr_end = self.skip_balanced(TokenKind::LParen, TokenKind::RParen).end;
                    continue;
                }
                expr_end = self.bump().span.end;
            }
            let expr_span = Span {
                start: expr_start,
                end: expr_end,
            };
            let span = self.finish_family_element(start);
            out.push(SvarIdentificationElement::Restriction {
                number,
                span,
                expr_span,
            });
            return;
        }
        self.bump();
    }

    /// One `equation N, name?;` row.
    fn read_svar_equation(&mut self) -> SvarEquation {
        let start = self.current_start();
        self.bump();
        let number = self.take_int();
        if self.at(TokenKind::Comma) {
            self.bump();
        }
        let mut names = Vec::new();
        while !self.at(TokenKind::Semi) && !self.at(TokenKind::Eof) {
            if self.at(TokenKind::Comma) {
                self.bump();
                continue;
            }
            if self.at(TokenKind::Ident) {
                let tok = self.bump();
                let lex = self.lexeme(&tok).to_string();
                names.push((self.intern.intern(&lex), tok.span));
                continue;
            }
            self.bump();
        }
        let end = self.block_end_after_consume();
        self.eat(TokenKind::Semi);
        SvarEquation {
            number,
            names,
            span: Span { start, end },
        }
    }

    /// An optional non-negative integer at the cursor.
    fn take_int(&mut self) -> Option<i32> {
        if !self.at(TokenKind::Number) {
            return None;
        }
        let tok = self.bump();
        parse_int_lexeme(self.lexeme(&tok))
    }

    fn bump_until_semi(&mut self) {
        while !self.at(TokenKind::Semi) && !self.at(TokenKind::Eof) {
            if self.at(TokenKind::LParen) {
                self.skip_balanced(TokenKind::LParen, TokenKind::RParen);
                continue;
            }
            self.bump();
        }
    }

    /// One element's span: its start through its `;`.
    fn finish_family_element(&mut self, start: u32) -> Span {
        let end = if self.at(TokenKind::Semi) {
            self.bump().span.end
        } else {
            self.current_start()
        };
        Span { start, end }
    }

    /// `conditional_forecast_paths;` ? `end;`: repeated `var name; periods ?; values ?;`.
    fn parse_conditional_forecast_paths_block(&mut self) {
        let opener_span = self.bump_plain_opener();
        let body_i = self.i;
        let body_end_i = self.consume_until_end();
        self.record_missing_end_if_unclosed(
            "conditional_forecast_paths",
            opener_span,
            body_i,
            body_end_i,
        );
        let end = self.block_end_after_consume();
        let saved = self.i;
        self.i = body_i;
        let mut rows = Vec::new();
        // A row that opens with a key the production does not take: the pin's
        // lexer knows `exogenize` / `endogenize`, and the grammar has no rule for
        // them inside this block.
        let mut stray_keywords: Vec<ShapeRefuse> = Vec::new();
        while self.i < body_end_i && !self.at(TokenKind::Eof) {
            let before = self.i;
            if let Some(row) = self.read_conditional_forecast_path() {
                rows.push(row);
            } else if self.at(TokenKind::Ident) {
                let tok = self.tokens[self.i].clone();
                let lex = self.lexeme(&tok).to_string();
                stray_keywords.push(ShapeRefuse::new(
                    tok.span,
                    "conditional_forecast_paths",
                    "a var row",
                ));
                let _ = lex;
                self.bump_until_semi();
                self.eat(TokenKind::Semi);
            }
            if self.i <= before {
                self.bump();
            }
            if self.i > body_end_i {
                self.i = body_end_i;
                break;
            }
        }
        self.i = saved;
        // The grammar's own shape: the body is a non-empty list of
        // `VAR symbol ';' PERIODS period_list ';' VALUES value_list ';'` rows. A row
        // that opens with a keyword the production does not take, one that stops
        // before its `values`, and an empty `periods` / `values` list are each a
        // syntax error; an empty body is refused at the `end;` that closes it.
        let mut shape_refuses = stray_keywords;
        if rows.is_empty() && shape_refuses.is_empty() {
            let end_tok = self.tokens[body_end_i.min(self.tokens.len() - 1)].span;
            shape_refuses.push(ShapeRefuse::new(
                end_tok,
                "conditional_forecast_paths",
                "a var row",
            ));
        }
        for row in &rows {
            if !row.has_periods {
                shape_refuses.push(ShapeRefuse::new(
                    row.span,
                    "conditional_forecast_paths",
                    "a periods row",
                ));
            } else if row.periods.is_empty() {
                shape_refuses.push(ShapeRefuse::new(
                    row.periods_span,
                    "periods",
                    "a date or an integer",
                ));
            } else if !row.has_values {
                shape_refuses.push(ShapeRefuse::new(
                    row.span,
                    "conditional_forecast_paths",
                    "a values row",
                ));
            } else if row.values.is_empty() {
                shape_refuses.push(ShapeRefuse::new(row.values_span, "values", "a value"));
            }
        }
        self.model
            .conditional_forecast_paths
            .push(ConditionalForecastPaths {
                span: Span {
                    start: opener_span.start,
                    end,
                },
                rows,
                shape_refuses,
            });
    }

    /// One `var name; periods ?; values ?;` row.
    fn read_conditional_forecast_path(&mut self) -> Option<ConditionalForecastPath> {
        if !self.at_ident_ci("var") {
            return None;
        }
        let start = self.current_start();
        self.bump();
        let Some(name_tok) = self
            .tokens
            .get(self.i)
            .filter(|t| t.kind == TokenKind::Ident)
            .cloned()
        else {
            self.bump_until_semi();
            self.eat(TokenKind::Semi);
            return None;
        };
        self.bump();
        let name = self.intern.intern(name_tok.text(self.src));
        self.eat(TokenKind::Semi);
        let mut periods = Vec::new();
        let mut periods_span = Span {
            start: name_tok.span.end,
            end: name_tok.span.end,
        };
        let mut has_periods = false;
        if self.at_ident_ci("periods") {
            has_periods = true;
            let from = self.i + 1;
            // `periods;` with nothing between the keyword and its `;`: the
            // grammar's `period_list` needs at least one `period_range`.
            if self
                .tokens
                .get(from)
                .is_some_and(|t| t.kind == TokenKind::Semi)
            {
                periods_span = Span {
                    start: self.tokens[self.i].span.start,
                    end: self.tokens[from].span.end,
                };
            }
            self.bump_until_semi();
            periods = self.list_entries(from, self.i);
            if !periods.is_empty() {
                periods_span = self.entry_list_span(from, self.i);
            }
            self.eat(TokenKind::Semi);
        }
        let mut values = Vec::new();
        let mut values_span = Span {
            start: periods_span.end,
            end: periods_span.end,
        };
        let mut has_values = false;
        if self.at_ident_ci("values") {
            has_values = true;
            let from = self.i + 1;
            if self
                .tokens
                .get(from)
                .is_some_and(|t| t.kind == TokenKind::Semi)
            {
                values_span = Span {
                    start: self.tokens[self.i].span.start,
                    end: self.tokens[from].span.end,
                };
            }
            self.bump_until_semi();
            values = self.list_entries(from, self.i);
            if !values.is_empty() {
                values_span = self.entry_list_span(from, self.i);
            }
            self.eat(TokenKind::Semi);
        }
        Some(ConditionalForecastPath {
            name,
            name_span: name_tok.span,
            periods,
            periods_span,
            values,
            values_span,
            span: Span {
                start,
                end: self.current_start(),
            },
            has_periods,
            has_values,
        })
    }

    /// List entries between two token offsets: a run of tokens with no whitespace
    /// or comma between them is one entry, so `1:4` counts as one and `1 2 3` as three.
    fn list_entries(&self, from: usize, to: usize) -> Vec<Span> {
        let mut out: Vec<Span> = Vec::new();
        let end = to.min(self.tokens.len());
        let mut k = from;
        while k < end {
            if self.tokens[k].kind == TokenKind::Comma {
                k += 1;
                continue;
            }
            let start = self.tokens[k].span.start;
            let mut stop = k;
            while stop + 1 < end {
                let prev = &self.tokens[stop];
                let next = &self.tokens[stop + 1];
                if next.kind == TokenKind::Comma {
                    break;
                }
                let between = &self.src[prev.span.end as usize..next.span.start as usize];
                if between.chars().any(char::is_whitespace) {
                    break;
                }
                stop += 1;
            }
            out.push(Span {
                start,
                end: self.tokens[stop].span.end,
            });
            k = stop + 1;
        }
        out
    }

    fn entry_list_span(&self, from: usize, to: usize) -> Span {
        let end = to.min(self.tokens.len());
        if from >= end {
            let at = self
                .tokens
                .get(from)
                .map(|t| t.span.start)
                .unwrap_or_else(|| self.current_start());
            return Span { start: at, end: at };
        }
        Span {
            start: self.tokens[from].span.start,
            end: self.tokens[end - 1].span.end,
        }
    }

    /// `change_type(type) name_list;`
    fn parse_change_type(&mut self) {
        let start = self.current_start();
        self.bump();
        let mut new_type = None;
        if self.at(TokenKind::LParen) {
            self.bump();
            if self.at(TokenKind::Ident) {
                let tok = self.bump();
                let lex = self.lexeme(&tok).to_string();
                new_type = change_type_kind(&lex);
            }
            while !self.at(TokenKind::Eof) && !self.at(TokenKind::RParen) {
                self.bump();
            }
            self.eat(TokenKind::RParen);
        }
        let mut names = Vec::new();
        while !self.at(TokenKind::Eof) && !self.at(TokenKind::Semi) {
            if self.at(TokenKind::Ident) {
                let tok = self.bump();
                let lex = self.lexeme(&tok).to_string();
                names.push((self.intern.intern(&lex), tok.span));
                continue;
            }
            self.bump();
        }
        let end = if self.at(TokenKind::Semi) {
            self.bump().span.end
        } else {
            self.current_start()
        };
        if let Some(new_type) = new_type {
            self.model.change_type_statements.push(ChangeTypeStmt {
                new_type,
                names,
                span: Span { start, end },
            });
        }
    }

    /// `optim_weights;` rows `symbol expr;` / `symbol, symbol expr;` `end;`
    fn parse_optim_weights_block(&mut self) {
        let opener_span = self.bump_plain_opener();
        if self.model.optim_weights_span.is_none() {
            self.model.optim_weights_span = Some(opener_span);
        }
        let body_i = self.i;
        let body_end_i = self.consume_until_end();
        self.record_missing_end_if_unclosed("optim_weights", opener_span, body_i, body_end_i);
        self.model
            .optim_weights_block_starts
            .push(self.model.optim_weights.len());
        let saved = self.i;
        self.i = body_i;
        while self.i < body_end_i && !self.at(TokenKind::Eof) {
            if self.at(TokenKind::Semi) || self.at(TokenKind::Comma) {
                self.bump();
                continue;
            }
            let before = self.i;
            if let Some(row) = self.parse_optim_weight_row(body_end_i) {
                self.model.optim_weights.push(row);
            }
            if self.i <= before {
                self.bump();
            }
            if self.i > body_end_i {
                self.i = body_end_i;
                break;
            }
        }
        self.i = saved;
    }

    fn parse_optim_weight_row(&mut self, end_i: usize) -> Option<OptimWeight> {
        if !self.at(TokenKind::Ident) {
            return None;
        }
        let first_tok = self.bump();
        let first_lex = self.lexeme(&first_tok).to_string();
        let mut second = None;
        if self.at(TokenKind::Comma) {
            self.bump();
            if self.at(TokenKind::Ident) {
                let second_tok = self.bump();
                let lex = self.lexeme(&second_tok).to_string();
                second = Some((self.intern.intern(&lex), second_tok.span));
            }
        }
        let expr = self.parse_expr();
        let end = self.finish_shock_stmt(end_i);
        Some(OptimWeight {
            first: self.intern.intern(&first_lex),
            first_span: first_tok.span,
            second: second.map(|(id, _)| id),
            second_span: second.map(|(_, span)| span),
            expr,
            span: Span {
                start: first_tok.span.start,
                end,
            },
        })
    }

    /// `ramsey_constraints;` one expression per `;`, `end;`
    fn parse_ramsey_constraints_block(&mut self) {
        let opener_span = self.bump_plain_opener();
        let start = opener_span.start;
        let body_i = self.i;
        let body_end_i = self.consume_until_end();
        self.record_missing_end_if_unclosed("ramsey_constraints", opener_span, body_i, body_end_i);
        let end = self.block_end_after_consume();
        if self.model.ramsey_constraints_span.is_none() {
            self.model.ramsey_constraints_span = Some(Span { start, end });
        }
        let saved = self.i;
        self.i = body_i;
        while self.i < body_end_i && !self.at(TokenKind::Eof) {
            if self.at(TokenKind::Semi) {
                self.bump();
                continue;
            }
            let before = self.i;
            let stmt_start = self.current_start();
            let expr = self.parse_expr();
            let stmt_end = self.finish_shock_stmt(body_end_i);
            if expr.is_none() && self.i <= before {
                self.bump();
                continue;
            }
            self.model.ramsey_constraints.push(RamseyConstraint {
                expr,
                span: Span {
                    start: stmt_start,
                    end: stmt_end,
                },
            });
            if self.i > body_end_i {
                self.i = body_end_i;
                break;
            }
        }
        self.i = saved;
    }

    /// `name=value` option of a block opener, as the group name.
    fn group_name_from_opener(&self, opener_span: Span) -> (String, Option<Span>) {
        let value = self
            .tokens
            .iter()
            .filter(|token| {
                token.span.start >= opener_span.start && token.span.end <= opener_span.end
            })
            .skip_while(|token| token.kind != TokenKind::Eq)
            .nth(1)
            .filter(|token| token.kind == TokenKind::Ident);
        match value {
            Some(token) => (token.text(self.src).to_string(), Some(token.span)),
            None => ("default".to_string(), None),
        }
    }

    fn parse_occbin_constraints_block(&mut self) {
        let start = self.bump().span.start;
        if self.at(TokenKind::LParen) {
            self.skip_balanced(TokenKind::LParen, TokenKind::RParen);
        }
        let opener_end = if self.at(TokenKind::Semi) {
            self.bump().span.end
        } else {
            self.current_start()
        };
        let opener_span = Span {
            start,
            end: opener_end,
        };
        let body_i = self.i;
        while !self.at(TokenKind::Eof) && !self.at_block_stop() {
            if self.at_ident_ci("name") {
                self.parse_occbin_regime();
            } else {
                self.skip_until_semi();
            }
        }
        if self.at_block_end() {
            self.record_missing_final("occbin_constraints", body_i, self.i);
        }
        let end = self.finish_block_named("occbin_constraints", opener_span, body_i);
        self.model
            .occbin_constraints_blocks
            .push(Span { start, end });
    }

    fn parse_occbin_regime(&mut self) {
        let name_tok = self.bump();
        if !self.at(TokenKind::String) {
            self.skip_until_semi();
            return;
        }
        let str_tok = self.bump();
        let name = unquote_string(self.lexeme(&str_tok));
        let name_span = str_tok.span;
        let mut end = if self.at(TokenKind::Semi) {
            self.tokens[self.i].span.end
        } else {
            str_tok.span.end
        };
        self.eat(TokenKind::Semi);
        let mut bind = None;
        let mut relax = None;
        let mut error_bind = None;
        let mut error_relax = None;
        while !self.at(TokenKind::Eof) && !self.at_block_stop() {
            let clause = if self.at_ident_ci("error_bind") {
                Some("error_bind")
            } else if self.at_ident_ci("error_relax") {
                Some("error_relax")
            } else if self.at_ident_ci("bind") {
                Some("bind")
            } else if self.at_ident_ci("relax") {
                Some("relax")
            } else {
                None
            };
            let Some(clause) = clause else {
                break;
            };
            self.bump();
            let expr_i = self.i;
            let expr_start = self.current_start();
            let expr = self.parse_expr();
            let expr_end_i = self.i;
            let expr_end = self.current_start();
            let text = join_lexemes(self.src, &self.tokens[expr_i..expr_end_i]);
            let occ = OccbinExpr {
                text,
                span: Span {
                    start: expr_start,
                    end: expr_end,
                },
                expr,
            };
            match clause {
                "bind" => bind = Some(occ),
                "relax" => relax = Some(occ),
                "error_bind" => error_bind = Some(occ),
                "error_relax" => error_relax = Some(occ),
                _ => {}
            }
            end = if self.at(TokenKind::Semi) {
                self.tokens[self.i].span.end
            } else {
                expr_end
            };
            self.eat(TokenKind::Semi);
        }
        self.model.occbin_constraints.push(OccbinConstraint {
            name,
            name_span,
            bind,
            relax,
            error_bind,
            error_relax,
            span: Span {
                start: name_tok.span.start,
                end,
            },
        });
    }

    fn parse_shocks_block(&mut self, record_stmts: bool) {
        let is_shocks = self.at_ident_ci("shocks");
        let opener_i = self.i;
        let keyword = self.tokens[opener_i].text(self.src).to_ascii_lowercase();
        let start = self.bump().span.start;
        if self.at(TokenKind::LParen) {
            let option_start = self.i;
            if is_shocks {
                let mut k = self.i + 1;
                let mut depth = 1;
                while k < self.tokens.len() && depth > 0 {
                    match self.tokens[k].kind {
                        TokenKind::LParen => depth += 1,
                        TokenKind::RParen => depth -= 1,
                        TokenKind::Ident
                            if self.tokens[k]
                                .text(self.src)
                                .eq_ignore_ascii_case("surprise") =>
                        {
                            self.model.shocks_surprise = true;
                            if self.model.shocks_surprise_span.is_none() {
                                self.model.shocks_surprise_span = Some(self.tokens[k].span);
                            }
                        }
                        _ => {}
                    }
                    k += 1;
                }
            }
            self.skip_balanced(TokenKind::LParen, TokenKind::RParen);
            if self.tokens[opener_i]
                .text(self.src)
                .eq_ignore_ascii_case("mshocks")
            {
                self.record_option_twice(option_start, self.i);
            }
        }
        let opener_end = if self.at(TokenKind::Semi) {
            self.bump().span.end
        } else {
            self.current_start()
        };
        let opener_span = Span {
            start,
            end: opener_end,
        };
        let body_i = self.i;
        let body_end_i = self.consume_until_end();
        self.record_missing_end_if_unclosed(&keyword, opener_span, body_i, body_end_i);
        let end = self.block_end_after_consume();
        if is_shocks || keyword == "mshocks" {
            self.model.shocks_block = Some(Span { start, end });
        }
        let block_kind = self.shock_block_kind(opener_i, body_i);
        if !matches!(block_kind, crate::model::ShockBlockKind::Heterogeneous) {
            self.collect_shock_vars(body_i, body_end_i);
        }
        if record_stmts && matches!(block_kind, crate::model::ShockBlockKind::Regular) {
            self.model
                .shock_stmt_block_starts
                .push(self.model.shock_stmts.len());
            self.collect_shock_stmts(body_i, body_end_i);
        }
        self.collect_shock_block(opener_i, body_i, body_end_i, end, block_kind);
        if self.i > body_end_i {
            self.record_missing_shocks_semis(body_i, body_end_i);
        }
        self.record_shock_shape_refuses(opener_i, body_i, body_end_i, block_kind);
    }

    fn parse_varobs(&mut self) {
        let start = self.current_start();
        self.bump();
        self.model.varobs_statement_count += 1;
        while !self.at(TokenKind::Eof) && !self.at(TokenKind::Semi) {
            if self.at(TokenKind::Comma) || self.at(TokenKind::Latex) {
                self.bump();
                continue;
            }
            if self.at(TokenKind::LParen) {
                self.skip_balanced(TokenKind::LParen, TokenKind::RParen);
                continue;
            }
            if self.at(TokenKind::Ident) {
                let tok = self.bump();
                let name = self.lexeme(&tok).to_string();
                let id = self.intern.intern(&name);
                self.model.varobs.push(ObservedVar {
                    name: id,
                    span: tok.span,
                });
                continue;
            }
            self.bump();
        }
        let end = if self.at(TokenKind::Semi) {
            self.tokens[self.i].span.end
        } else {
            self.current_start()
        };
        self.eat(TokenKind::Semi);
        let span = Span { start, end };
        if self.model.varobs_span.is_none() {
            self.model.varobs_span = Some(span);
        }
        if self.model.varobs_statement_count == 2 {
            self.model.varobs_second_span = Some(span);
        }
    }

    fn parse_varexobs(&mut self) {
        let start = self.current_start();
        self.bump();
        self.model.varexobs_statement_count += 1;
        while !self.at(TokenKind::Eof) && !self.at(TokenKind::Semi) {
            if self.at(TokenKind::Comma) || self.at(TokenKind::Latex) {
                self.bump();
                continue;
            }
            if self.at(TokenKind::LParen) {
                self.skip_balanced(TokenKind::LParen, TokenKind::RParen);
                continue;
            }
            if self.at(TokenKind::Ident) {
                let tok = self.bump();
                let name = self.lexeme(&tok).to_string();
                let id = self.intern.intern(&name);
                self.model.varexobs.push(ObservedVar {
                    name: id,
                    span: tok.span,
                });
                continue;
            }
            self.bump();
        }
        let end = if self.at(TokenKind::Semi) {
            self.tokens[self.i].span.end
        } else {
            self.current_start()
        };
        self.eat(TokenKind::Semi);
        let span = Span { start, end };
        if self.model.varexobs_span.is_none() {
            self.model.varexobs_span = Some(span);
        }
        if self.model.varexobs_statement_count == 2 {
            self.model.varexobs_second_span = Some(span);
        }
    }

    fn parse_estimated_params_block(&mut self) {
        let opener_span = self.bump_plain_opener();
        let start = opener_span.start;
        let body_i = self.i;
        let body_end_i = self.consume_until_end();
        self.record_missing_end_if_unclosed("estimated_params", opener_span, body_i, body_end_i);
        let end = self.current_start();
        self.model.estimated_params_span = Some(Span { start, end });
        self.model
            .estimated_params_block_starts
            .push(self.model.estimated_params.len());
        self.collect_estimated_params(
            body_i,
            body_end_i,
            opener_span.end,
            EstimatedParamsTarget::Params,
        );
    }

    fn parse_estimated_params_init_block(&mut self) {
        let opener_span = self.bump_estimated_params_init_opener();
        let start = opener_span.start;
        let body_i = self.i;
        let body_end_i = self.consume_until_end();
        self.record_missing_end_if_unclosed(
            "estimated_params_init",
            opener_span,
            body_i,
            body_end_i,
        );
        let end = self.current_start();
        self.model.estimated_params_init_span = Some(Span { start, end });
        self.model
            .estimated_params_init_block_starts
            .push(self.model.estimated_params_init.len());
        self.collect_estimated_params(
            body_i,
            body_end_i,
            opener_span.end,
            EstimatedParamsTarget::Init,
        );
    }

    fn parse_estimated_params_bounds_block(&mut self) {
        let opener_span = self.bump_plain_opener();
        let start = opener_span.start;
        let body_i = self.i;
        let body_end_i = self.consume_until_end();
        self.record_missing_end_if_unclosed(
            "estimated_params_bounds",
            opener_span,
            body_i,
            body_end_i,
        );
        let end = self.current_start();
        self.model.estimated_params_bounds_span = Some(Span { start, end });
        self.model
            .estimated_params_bounds_block_starts
            .push(self.model.estimated_params_bounds.len());
        self.collect_estimated_params(
            body_i,
            body_end_i,
            opener_span.end,
            EstimatedParamsTarget::Bounds,
        );
    }

    fn parse_observation_trends_block(&mut self) {
        let opener_span = self.bump_plain_opener();
        let start = opener_span.start;
        let body_i = self.i;
        let body_end_i = self.consume_until_end();
        self.record_missing_end_if_unclosed("observation_trends", opener_span, body_i, body_end_i);
        let end = self.current_start();
        self.model.observation_trends_span = Some(Span { start, end });
        self.collect_observation_trends(body_i, body_end_i);
    }

    fn at_policy_command(&self) -> Option<PolicyCommand> {
        if self.at_ident_ci("ramsey_model") {
            Some(PolicyCommand::RamseyModel)
        } else if self.at_ident_ci("ramsey_policy") {
            Some(PolicyCommand::RamseyPolicy)
        } else if self.at_ident_ci("discretionary_policy") {
            Some(PolicyCommand::DiscretionaryPolicy)
        } else if self.at_ident_ci("osr") {
            Some(PolicyCommand::Osr)
        } else {
            None
        }
    }

    fn parse_policy_command(&mut self, command: PolicyCommand) {
        let tok = self.bump();
        self.model.policy_commands.push(command);
        if self.model.policy_command_span.is_none() {
            self.model.policy_command_span = Some(tok.span);
        }
        if command == PolicyCommand::RamseyPolicy && self.model.ramsey_policy_span.is_none() {
            self.model.ramsey_policy_span = Some(tok.span);
        }
        if command == PolicyCommand::DiscretionaryPolicy
            && self.model.discretionary_policy_span.is_none()
        {
            self.model.discretionary_policy_span = Some(tok.span);
        }
        let (saw_instruments, planner_discount) = if self.at(TokenKind::LParen) {
            self.parse_policy_options(command)
        } else {
            (false, None)
        };
        if command == PolicyCommand::DiscretionaryPolicy {
            self.model.discretionary_has_instruments_option |= saw_instruments;
        }
        if matches!(
            command,
            PolicyCommand::RamseyPolicy | PolicyCommand::DiscretionaryPolicy | PolicyCommand::Osr
        ) {
            self.collect_trailing_symbols(command.as_str());
        }
        let end = if self.at(TokenKind::Semi) {
            self.bump().span.end
        } else {
            self.current_start()
        };
        self.model
            .policy_command_statements
            .push(PolicyCommandStatement {
                command,
                span: Span {
                    start: tok.span.start,
                    end,
                },
                planner_discount: if matches!(
                    command,
                    PolicyCommand::RamseyModel | PolicyCommand::RamseyPolicy
                ) {
                    planner_discount
                } else {
                    None
                },
            });
    }

    fn parse_policy_options(&mut self, command: PolicyCommand) -> (bool, Option<Span>) {
        let from = self.i;
        self.bump();
        let mut saw_instruments = false;
        let mut planner_discount = None;
        while !self.at(TokenKind::Eof) && !self.at(TokenKind::RParen) && !self.at(TokenKind::Semi) {
            if self.at_ident_ci("instruments") && self.peek_kind(1) == Some(TokenKind::Eq) {
                saw_instruments = true;
                self.bump();
                self.bump();
                self.collect_instruments();
            } else if self.at_ident_ci("planner_discount")
                && self.peek_kind(1) == Some(TokenKind::Eq)
            {
                let opt_span = self.tokens[self.i].span;
                self.bump();
                self.bump();
                if planner_discount.is_none() {
                    planner_discount = Some(opt_span);
                }
                let expr = self.parse_expr();
                if let Some(id) = expr {
                    if self.model.planner_discount_expr.is_none() {
                        self.model.planner_discount_expr = Some(id);
                    }
                    if self.model.planner_discount.is_none() {
                        let known = self.fold_known_params();
                        if let Some(v) = self.fold_expr(id, &known).filter(|v| v.is_finite()) {
                            self.model.planner_discount = Some(v);
                        }
                    }
                }
            } else if self.at(TokenKind::LParen) {
                let nested = self.i;
                self.skip_balanced(TokenKind::LParen, TokenKind::RParen);
                self.record_deprecated_options_in_range(nested, self.i);
            } else {
                if self.at(TokenKind::Ident) {
                    let tok = self.tokens[self.i].clone();
                    let lex = self.lexeme(&tok).to_string();
                    self.record_deprecated_option_ident(&lex, tok.span);
                }
                self.bump();
            }
        }
        self.eat(TokenKind::RParen);
        self.record_policy_option_flags(command, from, self.i);
        self.record_option_twice(from, self.i);
        (saw_instruments, planner_discount)
    }

    fn collect_instruments(&mut self) {
        if self.at(TokenKind::LParen) {
            self.bump();
            while !self.at(TokenKind::Eof) && !self.at(TokenKind::RParen) {
                if self.at(TokenKind::Ident) {
                    self.push_instrument();
                } else {
                    self.bump();
                }
            }
            self.eat(TokenKind::RParen);
            return;
        }
        while !self.at(TokenKind::Eof)
            && !self.at(TokenKind::Comma)
            && !self.at(TokenKind::RParen)
            && !self.at(TokenKind::Semi)
        {
            if self.at(TokenKind::Ident) {
                self.push_instrument();
            } else {
                self.bump();
            }
        }
    }

    fn push_instrument(&mut self) {
        let tok = self.bump();
        let name = self.lexeme(&tok).to_string();
        let id = self.intern.intern(&name);
        if !self.model.instruments.contains(&id) {
            self.model.instruments.push(id);
        }
    }

    fn parse_planner_objective(&mut self) {
        let start = self.current_start();
        self.bump();
        let expr = self.parse_expr();
        while !self.at(TokenKind::Semi) && !self.at(TokenKind::Eof) {
            if self.at(TokenKind::LParen) {
                self.skip_balanced(TokenKind::LParen, TokenKind::RParen);
                continue;
            }
            self.bump();
        }
        let end = if self.at(TokenKind::Semi) {
            self.tokens[self.i].span.end
        } else {
            self.current_start()
        };
        self.eat(TokenKind::Semi);
        let span = Span { start, end };
        self.model.planner_objective_spans.push(span);
        if self.model.planner_objective_span.is_none() {
            self.model.planner_objective_span = Some(span);
        }
        if self.model.planner_objective_expr.is_none() {
            self.model.planner_objective_expr = expr;
        }
    }

    fn parse_osr_params(&mut self) {
        self.symbol_list_id += 1;
        let list_id = self.symbol_list_id;
        let start = self.current_start();
        self.bump();
        self.model.osr_params_statement_count += 1;
        while !self.at(TokenKind::Semi) && !self.at(TokenKind::Eof) {
            if self.at(TokenKind::Ident) {
                let tok = self.tokens[self.i].clone();
                let lex = self.lexeme(&tok).to_string();
                let span = tok.span;
                self.bump();
                let id = self.intern.intern(&lex);
                self.model.osr_params.push(id);
                self.model.command_symbols.push(CommandSymbol {
                    command: "osr_params".to_string(),
                    name: id,
                    span,
                    list_id,
                });
            } else {
                self.bump();
            }
        }
        let end = if self.at(TokenKind::Semi) {
            self.tokens[self.i].span.end
        } else {
            self.current_start()
        };
        self.eat(TokenKind::Semi);
        if self.model.osr_params_span.is_none() {
            self.model.osr_params_span = Some(Span { start, end });
        } else if self.model.osr_params_second_span.is_none() {
            self.model.osr_params_second_span = Some(Span { start, end });
        }
    }

    fn fold_known_params(&self) -> HashMap<Name, f64> {
        let mut known = HashMap::new();
        for a in &self.model.param_assignments {
            let value = a
                .expr
                .and_then(|id| self.fold_expr(id, &known))
                .filter(|v| v.is_finite());
            match value {
                Some(v) => {
                    known.insert(a.name, v);
                }
                None => {
                    known.remove(&a.name);
                }
            }
        }
        known
    }

    fn fold_expr(&self, id: ExprId, known: &HashMap<Name, f64>) -> Option<f64> {
        if let Some(v) = self.model.exprs.get(id).interned {
            return Some(v);
        }
        match &self.model.exprs.get(id).kind {
            ExprKind::Number => {
                let span = self.model.exprs.get(id).span;
                let raw = self.src.get(span.start as usize..span.end as usize)?;
                raw.parse().ok()
            }
            ExprKind::Ident { name, timing, .. } => {
                if *timing != 0 {
                    return None;
                }
                known.get(name).copied()
            }
            ExprKind::Unary { op, arg } => {
                let v = self.fold_expr(*arg, known)?;
                Some(match op {
                    UnOp::Pos => v,
                    UnOp::Neg => -v,
                })
            }
            ExprKind::Binary { op, lhs, rhs } => {
                let l = self.fold_expr(*lhs, known)?;
                let r = self.fold_expr(*rhs, known)?;
                match op {
                    BinOp::Add => Some(l + r),
                    BinOp::Sub => Some(l - r),
                    BinOp::Mul => Some(l * r),
                    BinOp::Div => Some(l / r),
                    BinOp::Pow => Some(l.powf(r)),
                    BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge | BinOp::EqEq | BinOp::Ne => None,
                }
            }
            ExprKind::Call { .. }
            | ExprKind::String
            | ExprKind::Error
            | ExprKind::SteadyState { .. }
            | ExprKind::Expectation { .. } => None,
        }
    }

    fn collect_estimated_params(
        &mut self,
        start_i: usize,
        end_i: usize,
        body_start: u32,
        target: EstimatedParamsTarget,
    ) {
        let mut i = start_i;
        let mut entry_start = body_start;
        while i < end_i {
            if self.tokens[i].kind == TokenKind::Semi {
                entry_start = self.tokens[i].span.end;
                i += 1;
                continue;
            }
            let stmt_start = i;
            while i < end_i && self.tokens[i].kind != TokenKind::Semi {
                i += 1;
            }
            let has_semi = i < end_i && self.tokens[i].kind == TokenKind::Semi;
            if !has_semi {
                break;
            }
            let entry_end = self.tokens[i].span.end;
            if let Some(mut entry) = self.parse_estimated_param_entry(stmt_start, i) {
                entry.span = Span {
                    start: entry_start,
                    end: entry_end,
                };
                match target {
                    EstimatedParamsTarget::Params => self.model.estimated_params.push(entry),
                    EstimatedParamsTarget::Init => self.model.estimated_params_init.push(entry),
                    EstimatedParamsTarget::Bounds => self.model.estimated_params_bounds.push(entry),
                }
            }
            entry_start = entry_end;
            i += 1;
        }
    }

    fn parse_estimated_param_entry(
        &mut self,
        start_i: usize,
        end_i: usize,
    ) -> Option<EstimatedParam> {
        let mut i = start_i;
        while i < end_i && self.tokens[i].kind == TokenKind::Comma {
            i += 1;
        }
        if i >= end_i || self.tokens[i].kind != TokenKind::Ident {
            return None;
        }
        let first = self.tokens[i].text(self.src).to_string();
        i += 1;
        let (kind, name, corr_with) = if first.eq_ignore_ascii_case("stderr") {
            let (name, next) = next_ident(&self.tokens, self.src, i, end_i)?;
            i = next;
            (EstimatedParamKind::Stderr, name, None)
        } else if first.eq_ignore_ascii_case("skew") {
            let (name, next) = next_ident(&self.tokens, self.src, i, end_i)?;
            i = next;
            (EstimatedParamKind::Skew, name, None)
        } else if first.eq_ignore_ascii_case("corr") {
            let (name, next) = next_ident(&self.tokens, self.src, i, end_i)?;
            i = next;
            while i < end_i && self.tokens[i].kind == TokenKind::Comma {
                i += 1;
            }
            let (other, next) = next_ident(&self.tokens, self.src, i, end_i)?;
            i = next;
            (EstimatedParamKind::Corr, name, Some(other))
        } else {
            (EstimatedParamKind::Param, first, None)
        };
        let name = self.intern.intern(&name);
        let corr_with = corr_with.map(|n| self.intern.intern(&n));

        let saved = self.i;
        self.i = i;
        let mut value_exprs = Vec::new();
        loop {
            while self.i < end_i && self.at(TokenKind::Comma) {
                self.bump();
            }
            if self.i >= end_i || self.at(TokenKind::Semi) {
                break;
            }
            if self.at(TokenKind::Ident) {
                let ident = self.lexeme(&self.tokens[self.i]).to_string();
                if PRIOR_SHAPES.iter().any(|s| ident.eq_ignore_ascii_case(s)) {
                    break;
                }
            }
            match self.parse_expr() {
                Some(id) => value_exprs.push(id),
                None => break,
            }
        }
        let mut prior_beta = false;
        let mut mean_expr = None;
        let mut std_expr = None;
        if self.i < end_i && self.at(TokenKind::Ident) {
            let ident = self.lexeme(&self.tokens[self.i]).to_string();
            if PRIOR_SHAPES.iter().any(|s| ident.eq_ignore_ascii_case(s)) {
                prior_beta = ident.eq_ignore_ascii_case("beta_pdf");
                self.bump();
                while self.i < end_i && self.at(TokenKind::Comma) {
                    self.bump();
                }
                mean_expr = self.parse_expr();
                while self.i < end_i && self.at(TokenKind::Comma) {
                    self.bump();
                }
                std_expr = self.parse_expr();
            }
        }
        self.i = saved;

        let known = self.fold_known_params();
        let fold = |id: Option<ExprId>| id.and_then(|e| self.fold_expr(e, &known));
        let init_expr = value_exprs.first().copied();
        let lower_expr = value_exprs.get(1).copied();
        let upper_expr = value_exprs.get(2).copied();
        Some(EstimatedParam {
            name,
            kind,
            corr_with,
            init: fold(init_expr),
            lower: fold(lower_expr),
            upper: fold(upper_expr),
            init_expr,
            lower_expr,
            upper_expr,
            mean_expr,
            std_expr,
            prior_beta,
            span: Span { start: 0, end: 0 },
        })
    }

    fn collect_observation_trends(&mut self, start_i: usize, end_i: usize) {
        // Dynare clears trend_elements at the end of each block.
        let mut seen = std::collections::HashSet::new();
        let mut i = start_i;
        while i < end_i {
            let stmt_start = i;
            while i < end_i && self.tokens[i].kind != TokenKind::Semi {
                i += 1;
            }
            let mut j = stmt_start;
            while j < i {
                if self.tokens[j].kind == TokenKind::Ident {
                    let next = self.tokens.get(j + 1).filter(|_| j + 1 < i);
                    if next.is_some_and(|t| t.kind == TokenKind::LParen || t.kind == TokenKind::Eq)
                    {
                        let name = self.tokens[j].text(self.src).to_string();
                        let span = self.tokens[j].span;
                        let id = self.intern.intern(&name);
                        if seen.insert(id) {
                            self.model.observation_trends.push((id, span));
                        } else {
                            self.model.observation_trends_dups.push((id, span));
                        }
                        break;
                    }
                }
                j += 1;
            }
            if i < end_i && self.tokens[i].kind == TokenKind::Semi {
                i += 1;
            }
        }
    }

    fn bump_plain_opener(&mut self) -> Span {
        let start = self.bump().span.start;
        if self.at(TokenKind::LParen) {
            let from = self.i;
            self.skip_balanced(TokenKind::LParen, TokenKind::RParen);
            self.record_option_twice(from, self.i);
        }
        let end = if self.at(TokenKind::Semi) {
            self.bump().span.end
        } else {
            self.current_start()
        };
        Span { start, end }
    }

    fn bump_estimated_params_init_opener(&mut self) -> Span {
        let start = self.bump().span.start;
        if self.at(TokenKind::LParen) {
            let from = self.i;
            self.skip_balanced(TokenKind::LParen, TokenKind::RParen);
            self.record_option_twice(from, self.i);
            let opts = top_options(&self.tokens, self.src, from, self.i);
            for opt in opts {
                if opt.ident.eq_ignore_ascii_case("use_calibration")
                    && self.model.estimated_params_init_use_calibration.is_none()
                {
                    self.model.estimated_params_init_use_calibration = Some(opt.span);
                }
            }
        }
        let end = if self.at(TokenKind::Semi) {
            self.bump().span.end
        } else {
            self.current_start()
        };
        Span { start, end }
    }

    fn bump_init_end_opener(&mut self, is_initval: bool) -> Span {
        let start = self.bump().span.start;
        if self.at(TokenKind::LParen) {
            let from = self.i;
            self.skip_balanced(TokenKind::LParen, TokenKind::RParen);
            if self.option_ident_in_range(from, self.i, "all_values_required") {
                if is_initval {
                    self.model.initval_all_values_required = true;
                } else {
                    self.model.endval_all_values_required = true;
                }
            }
            self.record_option_twice(from, self.i);
        }
        let end = if self.at(TokenKind::Semi) {
            self.bump().span.end
        } else {
            self.current_start()
        };
        let span = Span { start, end };
        if is_initval && self.model.endval_block.is_some() {
            self.model.initval_after_endval_span.get_or_insert(span);
        }
        span
    }

    fn collect_shock_vars(&mut self, start_i: usize, end_i: usize) {
        let mut i = start_i;
        while i < end_i {
            let is_var_or_corr = self.tokens[i].kind == TokenKind::Ident && {
                let kw = self.tokens[i].text(self.src);
                kw.eq_ignore_ascii_case("var") || kw.eq_ignore_ascii_case("corr")
            };
            if is_var_or_corr {
                i += 1;
                while i < end_i {
                    let kind = self.tokens[i].kind;
                    if kind == TokenKind::Eq || kind == TokenKind::Semi {
                        break;
                    }
                    if kind == TokenKind::Ident {
                        let name = self.tokens[i].text(self.src).to_string();
                        if !name.eq_ignore_ascii_case("stderr") {
                            let id = self.intern.intern(&name);
                            if !self.model.shocks_vars.contains(&id) {
                                self.model.shocks_vars.push(id);
                            }
                        }
                    }
                    i += 1;
                }
                continue;
            }
            i += 1;
        }
    }

    fn collect_shock_stmts(&mut self, start_i: usize, end_i: usize) {
        let saved = self.i;
        self.i = start_i;
        while self.i < end_i {
            if self.at(TokenKind::Semi) {
                self.bump();
                continue;
            }
            if self.at_ident_ci("var") {
                // `var x; periods ...; values ...;` is deterministic and must
                // never enter the stochastic variance checks.
                if self.shock_var_is_scheduled(self.i, end_i) {
                    self.skip_scheduled_shock(end_i);
                } else if let Some(stmt) = self.parse_shock_var_stmt(end_i) {
                    self.model.shock_stmts.push(stmt);
                }
            } else if self.at_ident_ci("corr") {
                if let Some(stmt) = self.parse_shock_corr_stmt(end_i) {
                    self.model.shock_stmts.push(stmt);
                }
            } else if self.at_ident_ci("skew") {
                if let Some(stmt) = self.parse_shock_skew_stmt(end_i) {
                    self.model.shock_stmts.push(stmt);
                }
            } else {
                while self.i < end_i && !self.at(TokenKind::Semi) {
                    self.bump();
                }
                if self.at(TokenKind::Semi) {
                    self.bump();
                }
            }
        }
        self.i = saved;
    }

    pub(super) fn parse_shock_var_stmt(&mut self, end_i: usize) -> Option<ShockStmt> {
        let start = self.current_start();
        self.bump();
        let mut names = Vec::new();
        while self.i < end_i && !self.at(TokenKind::Eq) && !self.at(TokenKind::Semi) {
            if self.at(TokenKind::Ident) {
                let tok = self.bump();
                let lex = self.lexeme(&tok).to_string();
                if !lex.eq_ignore_ascii_case("stderr") {
                    names.push(self.intern.intern(&lex));
                }
            } else {
                self.bump();
            }
        }
        let (mut rhs_expr, rhs) = if self.at(TokenKind::Eq) {
            self.bump();
            self.parse_folded_rhs()
        } else {
            (None, None)
        };
        let mut end = self.finish_shock_stmt(end_i);
        let mut is_stderr = false;
        if rhs_expr.is_none() && names.len() == 1 && self.i < end_i && self.at_ident_ci("stderr") {
            self.bump();
            let (expr, _) = self.parse_folded_rhs();
            rhs_expr = expr;
            end = self.finish_shock_stmt(end_i);
            is_stderr = true;
        }
        if names.is_empty() {
            return None;
        }
        let kind = if is_stderr {
            ShockKind::Stderr(names[0])
        } else if names.len() == 1 {
            ShockKind::Var(names[0])
        } else {
            ShockKind::Cov(names)
        };
        Some(ShockStmt {
            kind,
            rhs,
            rhs_expr,
            span: Span { start, end },
        })
    }

    pub(super) fn parse_shock_corr_stmt(&mut self, end_i: usize) -> Option<ShockStmt> {
        let start = self.current_start();
        self.bump();
        let mut names = Vec::new();
        while self.i < end_i && !self.at(TokenKind::Eq) && !self.at(TokenKind::Semi) {
            if self.at(TokenKind::Ident) {
                let tok = self.bump();
                let lex = self.lexeme(&tok).to_string();
                names.push(self.intern.intern(&lex));
            } else {
                self.bump();
            }
        }
        let has_eq = self.at(TokenKind::Eq);
        let (rhs_expr, rhs) = if has_eq {
            self.bump();
            self.parse_folded_rhs()
        } else {
            (None, None)
        };
        let end = self.finish_shock_stmt(end_i);
        if names.len() < 2 || !has_eq {
            return None;
        }
        Some(ShockStmt {
            kind: ShockKind::Corr {
                a: names[0],
                b: names[1],
            },
            rhs,
            rhs_expr,
            span: Span { start, end },
        })
    }

    pub(super) fn parse_shock_skew_stmt(&mut self, end_i: usize) -> Option<ShockStmt> {
        let start = self.current_start();
        self.bump();
        let mut names = Vec::new();
        while self.i < end_i && !self.at(TokenKind::Eq) && !self.at(TokenKind::Semi) {
            if self.at(TokenKind::Ident) {
                let tok = self.bump();
                let lex = self.lexeme(&tok).to_string();
                names.push(self.intern.intern(&lex));
            } else {
                self.bump();
            }
        }
        let has_eq = self.at(TokenKind::Eq);
        let (rhs_expr, rhs) = if has_eq {
            self.bump();
            self.parse_folded_rhs()
        } else {
            (None, None)
        };
        let end = self.finish_shock_stmt(end_i);
        if names.is_empty() || !has_eq {
            return None;
        }
        Some(ShockStmt {
            kind: ShockKind::Skew(names),
            rhs,
            rhs_expr,
            span: Span { start, end },
        })
    }

    fn parse_folded_rhs(&mut self) -> (Option<ExprId>, Option<f64>) {
        let Some(id) = self.parse_expr() else {
            return (None, None);
        };
        let known = self.fold_known_params();
        let folded = self.fold_expr(id, &known).filter(|v| v.is_finite());
        (Some(id), folded)
    }

    fn finish_shock_stmt(&mut self, end_i: usize) -> u32 {
        while self.i < end_i && !self.at(TokenKind::Semi) {
            if self.at(TokenKind::LParen) {
                self.skip_balanced(TokenKind::LParen, TokenKind::RParen);
                continue;
            }
            self.bump();
        }
        let end = if self.at(TokenKind::Semi) {
            self.tokens[self.i].span.end
        } else {
            self.current_start()
        };
        if self.at(TokenKind::Semi) {
            self.bump();
        }
        end
    }

    fn skip_block(&mut self) {
        let opener = self.lexeme(&self.tokens[self.i]).to_ascii_lowercase();
        let start = self.i;
        self.bump();
        if self.at(TokenKind::LParen) {
            self.skip_balanced(TokenKind::LParen, TokenKind::RParen);
        }
        self.eat(TokenKind::Semi);
        self.consume_until_end();
        if opener == "verbatim" {
            // Raw text: 7.1 passes it through, so its quotes are none of the grammar's business.
            self.verbatim_ranges.push(start..self.i);
        }
    }

    fn parse_top_assignment(&mut self) {
        let Some(assignment) = self.parse_named_assignment() else {
            return;
        };
        if self
            .model
            .parameters
            .iter()
            .any(|d| d.name == assignment.name)
        {
            self.model.param_assignments.push(assignment);
        } else {
            self.model.helper_assignments.push(assignment);
        }
    }

    /// `end` inside a block is the closer. `end = 0;` is not a row. 7.1's
    /// sentence on the initval shape is `syntax error, unexpected IDENTIFIER,
    /// expecting ';'`. The span is the token after `end` (the `=`).
    fn refuse_end_word(&mut self, body_end_i: usize) -> bool {
        if !self.at_ident("end") {
            return false;
        }
        let span = self
            .tokens
            .get(self.i + 1)
            .map(|t| t.span)
            .unwrap_or(self.tokens[self.i].span);
        self.record_issue(ParseIssue {
            kind: ParseIssueKind::UnexpectedEndAssign,
            span,
        });
        while self.i < body_end_i && !self.at(TokenKind::Semi) && !self.at(TokenKind::Eof) {
            self.bump();
        }
        self.eat(TokenKind::Semi);
        true
    }

    fn parse_named_assignment(&mut self) -> Option<Assignment> {
        if !self.looks_like_assignment_start() {
            self.skip_to_stmt_end();
            self.eat(TokenKind::Semi);
            return None;
        }
        let tok = self.bump();
        let name = self.lexeme(&tok).to_string();
        self.eat(TokenKind::Eq);
        let expr_i = self.i;
        let expr = self.parse_expr();
        while !self.at(TokenKind::Semi) && !self.at(TokenKind::Eof) {
            if self.looks_like_assignment_start()
                || self.at_follower_keyword()
                || self.at_block_stop()
            {
                break;
            }
            if self.at(TokenKind::LParen) {
                self.skip_balanced(TokenKind::LParen, TokenKind::RParen);
                continue;
            }
            self.bump();
        }
        let expr_end_i = self.i;
        let stmt_end = if self.at(TokenKind::Semi) {
            self.tokens[self.i].span.end
        } else {
            self.current_start()
        };
        self.eat(TokenKind::Semi);
        let expression = join_lexemes(self.src, &self.tokens[expr_i..expr_end_i]);
        let id = self.intern.intern(&name);
        Some(Assignment {
            name: id,
            expression,
            span: Span {
                start: tok.span.start,
                end: stmt_end,
            },
            expr,
        })
    }

    fn consume_until_end(&mut self) -> usize {
        loop {
            if self.at(TokenKind::Eof) || self.at_block_opener() {
                return self.i;
            }
            if self.at_ident("end") && self.peek_kind(1) == Some(TokenKind::Semi) {
                let idx = self.i;
                self.bump();
                self.bump();
                return idx;
            }
            self.bump();
        }
    }

    fn statements_in(&self, start_i: usize, end_i: usize) -> Vec<(String, Span)> {
        let mut out = Vec::new();
        let mut stmt_start = start_i;
        for i in start_i..end_i {
            let tok = &self.tokens[i];
            if tok.kind == TokenKind::Eof {
                continue;
            }
            if tok.kind == TokenKind::Semi {
                if stmt_start < i {
                    let raw = join_lexemes(self.src, &self.tokens[stmt_start..i]);
                    out.push((
                        raw,
                        Span {
                            start: self.tokens[stmt_start].span.start,
                            end: tok.span.end,
                        },
                    ));
                }
                stmt_start = i + 1;
            }
        }
        if stmt_start < end_i {
            let last = &self.tokens[end_i - 1];
            out.push((
                join_lexemes(self.src, &self.tokens[stmt_start..end_i]),
                Span {
                    start: self.tokens[stmt_start].span.start,
                    end: last.span.end,
                },
            ));
        }
        out
    }

    fn block_end_after_consume(&self) -> u32 {
        if self.i > 0 {
            self.tokens[self.i - 1].span.end
        } else {
            self.current_start()
        }
    }

    fn assignment_from(&mut self, raw: &str, span: Span) -> Option<Assignment> {
        let text = collapse_ws(raw);
        let (lhs, rhs) = split_eq(&text)?;
        if !is_ident(lhs) {
            return None;
        }
        let name = self.intern.intern(lhs);
        Some(Assignment {
            name,
            expression: rhs.to_string(),
            span,
            expr: None,
        })
    }

    fn parse_equation_statement(&mut self) -> Option<(Equation, Range<usize>)> {
        if self.at(TokenKind::Semi) {
            self.bump();
            return None;
        }
        if self.at(TokenKind::Eof) || self.at_block_stop() {
            return None;
        }

        let stmt_i = self.i;
        let stmt_start = self.current_start();
        let mut static_tag = false;
        let mut dynamic_tag = false;
        let mut tags = Vec::new();
        let mut tag_map = BTreeMap::new();
        let mut tag_twice = Vec::new();
        while self.at(TokenKind::LBrack) {
            let tag = self.parse_tag();
            static_tag |= tag.static_tag;
            dynamic_tag |= tag.dynamic_tag;
            tags.extend(tag.flags);
            tag_twice.extend(tag.twice);
            tag_map.extend(tag.map);
        }
        let is_local = self.at(TokenKind::Hash);
        if is_local {
            self.bump();
        }

        let (lhs_expr, lhs_ok) = self.parse_expr_side(ExprStop::EqOrSemi);
        let saw_eq = self.at(TokenKind::Eq);
        let (rhs_expr, rhs_ok) = if saw_eq {
            self.bump();
            self.parse_expr_side(ExprStop::Semi)
        } else {
            (None, true)
        };

        let eq_end_i = self.i;
        let mut complementarity = None;
        if self.at(TokenKind::Perpendicular) {
            self.bump();
            let comp_i = self.i;
            let comp_start = self.current_start();
            let comp_expr = self.parse_expr();
            let comp_end_i = self.i;
            let comp_end = self.current_start();
            let text = join_lexemes(self.src, &self.tokens[comp_i..comp_end_i]);
            let matched = comp_expr.and_then(|id| self.match_complementarity(id));
            complementarity = Some(Complementarity {
                text,
                span: Span {
                    start: comp_start,
                    end: comp_end,
                },
                matched,
            });
        }

        if !self.at(TokenKind::Semi) && !self.at(TokenKind::Eof) && !self.at_block_stop() {
            self.skip_to_stmt_end();
        }
        let stmt_end_i = self.i;
        let stmt_end = self.current_start();
        self.eat(TokenKind::Semi);

        let raw = join_lexemes(self.src, &self.tokens[stmt_i..eq_end_i]);
        let mut eq = equation_from_statement(
            &raw,
            Span {
                start: stmt_start,
                end: stmt_end,
            },
        )?;
        eq.is_local = is_local;
        eq.model_local = is_local;
        eq.static_tag = static_tag;
        eq.dynamic_tag = dynamic_tag;
        eq.tags = tags;
        eq.tag_map = tag_map;
        eq.tag_twice = tag_twice;
        eq.complementarity = complementarity;
        eq.lhs_expr = Some(if lhs_ok {
            lhs_expr.unwrap_or_else(|| self.alloc_error(eq.span))
        } else {
            self.alloc_error(eq.span)
        });
        if saw_eq {
            eq.rhs_expr = Some(if rhs_ok {
                rhs_expr.unwrap_or_else(|| self.alloc_error(eq.span))
            } else {
                self.alloc_error(eq.span)
            });
        }
        Some((eq, stmt_i..stmt_end_i))
    }

    fn parse_expr_side(&mut self, stop: ExprStop) -> (Option<ExprId>, bool) {
        let id = self.parse_expr();
        let clean = match stop {
            ExprStop::EqOrSemi => {
                self.at(TokenKind::Eq)
                    || self.at(TokenKind::Semi)
                    || self.at(TokenKind::Perpendicular)
                    || self.at(TokenKind::Eof)
                    || self.at_block_stop()
            }
            ExprStop::Semi => {
                self.at(TokenKind::Semi)
                    || self.at(TokenKind::Perpendicular)
                    || self.at(TokenKind::Eof)
                    || self.at_block_stop()
            }
        };
        if !clean {
            self.skip_to_stmt_end();
            return (id, false);
        }
        (id, id.is_some() || self.at_side_empty(stop))
    }

    fn at_side_empty(&self, stop: ExprStop) -> bool {
        match stop {
            ExprStop::EqOrSemi => {
                self.at(TokenKind::Eq)
                    || self.at(TokenKind::Semi)
                    || self.at(TokenKind::Perpendicular)
                    || self.at_block_stop()
            }
            ExprStop::Semi => {
                self.at(TokenKind::Semi)
                    || self.at(TokenKind::Perpendicular)
                    || self.at_block_stop()
            }
        }
    }

    fn parse_tag(&mut self) -> ParsedTag {
        self.bump();
        let mut static_tag = false;
        let mut dynamic_tag = false;
        let mut flags = Vec::new();
        let mut map = BTreeMap::new();
        let mut twice = Vec::new();
        while !self.at(TokenKind::Eof)
            && !self.at(TokenKind::RBrack)
            && !self.at(TokenKind::Semi)
            && !self.at_block_stop()
        {
            if self.at(TokenKind::Ident) {
                let tok = self.bump();
                let key = self.lexeme(&tok).to_ascii_lowercase();
                let mut value = String::new();
                if self.at(TokenKind::Eq) {
                    self.bump();
                    if self.at(TokenKind::String) {
                        let v = self.bump();
                        value = unquote_string(self.lexeme(&v));
                    } else if self.at(TokenKind::Ident) || self.at(TokenKind::Number) {
                        let v = self.bump();
                        value = self.lexeme(&v).to_string();
                    }
                }
                if key == "static" {
                    static_tag = true;
                    flags.push("static".to_string());
                }
                if key == "dynamic" {
                    dynamic_tag = true;
                    flags.push("dynamic".to_string());
                }
                if map.contains_key(&key) {
                    twice.push((key.clone(), tok.span));
                }
                map.insert(key, value);
            } else {
                self.bump();
            }
        }
        self.eat(TokenKind::RBrack);
        ParsedTag {
            static_tag,
            dynamic_tag,
            flags,
            map,
            twice,
        }
    }

    fn skip_to_stmt_end(&mut self) {
        while !self.at(TokenKind::Semi) && !self.at(TokenKind::Eof) && !self.at_block_stop() {
            if self.at(TokenKind::LParen) {
                self.skip_balanced(TokenKind::LParen, TokenKind::RParen);
            } else if self.at(TokenKind::LBrack) {
                self.skip_balanced(TokenKind::LBrack, TokenKind::RBrack);
            } else {
                self.bump();
            }
        }
    }

    fn finish_block_named(&mut self, keyword: &str, opener_span: Span, body_i: usize) -> u32 {
        if self.at_ident("end") {
            self.bump();
            if self.at(TokenKind::Semi) {
                return self.bump().span.end;
            }
        }
        self.record_missing_end(keyword, opener_span, body_i);
        self.current_start()
    }

    fn at_block_end(&self) -> bool {
        self.at_ident("end") && self.peek_kind(1) == Some(TokenKind::Semi)
    }

    fn at_block_stop(&self) -> bool {
        self.at_block_end() || self.at_block_opener()
    }

    /// A `BLOCK_OPENERS` word the enclosing body never terminated. Their lexer gives
    /// the word the opener reading in exactly one state, `INITIAL`, and it reaches
    /// that state three ways: the file's first token, the `;` that closes a statement
    /// (`DynareFlex.ll:209`), and the word `end` — whose rule returns to `INITIAL`
    /// **before** it returns `token::END`, so no `;` is needed (`:255-260`).
    /// Everywhere else the word falls through to the identifier rule (`:1138`) and
    /// the symbol table decides: `var y shocks;` declares two endogenous and
    /// `+ shocks` is that variable.
    ///
    /// Inside a `model`, `model_replace`, `steady_state_model`, or
    /// `matched_moments` body two readings are rows of that body
    /// rather than a new block, and 7.1 takes both: a declared spelling is the
    /// variable (`model; shocks; end;` is accepted), and a word whose `;` is directly
    /// followed by `end;` closes the body, because their `model_equation` is an
    /// expression and 7.1 refuses an empty block (`unexpected END, expecting …`).
    ///
    /// Neither applies after a bare `end`: the lexer is back in `INITIAL`, where the
    /// opener rules outrank the symbol table, which is why 7.1 refuses `end` (no `;`)
    /// followed by `shocks;` with `unexpected SHOCKS, expecting ';'`.
    fn at_block_opener(&self) -> bool {
        let Some(keyword) = BLOCK_OPENERS.iter().find(|kw| self.at_ident_ci(kw)) else {
            return false;
        };
        let after_bare_end = self.previous_ident_is("end");
        if !after_bare_end && !self.starts_statement() {
            return false;
        }
        if !after_bare_end
            && self.in_equation_body
            && self.declared_before(self.current_start(), keyword)
        {
            return false;
        }
        let mut k = 1;
        if self.peek_kind(k) == Some(TokenKind::LParen) {
            let mut depth = 1;
            k += 1;
            while let Some(kind) = self.peek_kind(k) {
                match kind {
                    TokenKind::LParen => depth += 1,
                    TokenKind::RParen => {
                        depth -= 1;
                        if depth == 0 {
                            k += 1;
                            break;
                        }
                    }
                    TokenKind::Eof => return false,
                    _ => {}
                }
                k += 1;
            }
        }
        if self.peek_kind(k) != Some(TokenKind::Semi) {
            return false;
        }
        after_bare_end || !(self.in_equation_body && self.peek_ident_is(k + 1, "end"))
    }

    /// The cursor sits at the head of a statement, where the pin's lexer is in
    /// `INITIAL` after a `;`: the file's first token, or the token after that `;`.
    /// Macro directives carry no token of their own once the file is expanded, so
    /// they are stepped over. The other way into `INITIAL` is the word `end`, which
    /// `previous_ident_is` reads.
    fn starts_statement(&self) -> bool {
        let mut k = self.i;
        while k > 0 && self.tokens[k - 1].kind == TokenKind::MacroDir {
            k -= 1;
        }
        k == 0 || self.tokens[k - 1].kind == TokenKind::Semi
    }

    /// The word just before the cursor is the spelling given, case-insensitively,
    /// with macro directives stepped over.
    fn previous_ident_is(&self, spelling: &str) -> bool {
        let mut k = self.i;
        while k > 0 && self.tokens[k - 1].kind == TokenKind::MacroDir {
            k -= 1;
        }
        k > 0
            && self.tokens[k - 1].kind == TokenKind::Ident
            && self.tokens[k - 1]
                .text(self.src)
                .eq_ignore_ascii_case(spelling)
    }

    /// The word `ahead` tokens on is the spelling given, case-insensitively.
    fn peek_ident_is(&self, ahead: usize, spelling: &str) -> bool {
        self.peek_tok(ahead).is_some_and(|t| {
            t.kind == TokenKind::Ident && t.text(self.src).eq_ignore_ascii_case(spelling)
        })
    }

    /// The declaration scan's break list: a `var` list is a `DYNARE_STATEMENT`, and
    /// each word here carries a rule scoped to that state too, so each lexes to its
    /// own token and ends the list. The five declarations are `DynareFlex.ll:111-117`
    /// with `:667-671`, `varobs` `:124` with `:1084`, `epilogue` `:226` with `:738`,
    /// `init2shocks` `:222` with `:458`. Every other block opener carries an
    /// `<INITIAL>` rule only, so it falls through to the statement identifier rule and
    /// is an ordinary name there: `var y shocks;` declares two endogenous.
    fn at_decl_or_block_keyword(&self) -> bool {
        const KS: &[&str] = &[
            "varexo_det",
            "var",
            "varexo",
            "parameters",
            "predetermined_variables",
            "varobs",
            "epilogue",
            "init2shocks",
        ];
        KS.iter().any(|kw| self.at_ident_ci(kw))
    }

    fn looks_like_assignment_start(&self) -> bool {
        self.at(TokenKind::Ident) && self.peek_kind(1) == Some(TokenKind::Eq)
    }

    fn at_follower_keyword(&self) -> bool {
        const KS: &[&str] = &[
            "model",
            "model_remove",
            "model_replace",
            "var",
            "var_remove",
            "varexo",
            "varexo_det",
            "parameters",
            "predetermined_variables",
            "initval",
            "endval",
            "shocks",
            "occbin_constraints",
            "steady_state_model",
            "steady",
            "check",
            "resid",
            "stoch_simul",
            "simul",
            "estimation",
            "osr",
            "calib_smoother",
            "forecast",
            "identification",
            "dynasave",
            "dynatype",
            "model_diagnostics",
            "model_info",
            "perfect_foresight_setup",
            "perfect_foresight_solver",
        ];
        if KS.iter().any(|kw| self.at_ident_ci(kw)) {
            return true;
        }
        let Some(tok) = self.tokens.get(self.i) else {
            return false;
        };
        if tok.kind != TokenKind::Ident
            || !crate::command_skip::is_parse_skip_command(tok.text(self.src))
        {
            return false;
        }
        matches!(
            self.peek_kind(1),
            Some(TokenKind::LParen) | Some(TokenKind::Semi)
        )
    }

    fn record_issue(&mut self, issue: ParseIssue) {
        self.model.parse_issues.push(issue);
    }

    /// 7.1's lexer accepts only single-quoted strings, so a double-quoted one is lexer
    /// junk wherever the grammar reads a string. `verbatim` bodies pass raw text through.
    fn record_double_quoted_strings(&mut self) {
        let mut issues = Vec::new();
        for (i, tok) in self.tokens.iter().enumerate() {
            if tok.kind != TokenKind::String || !self.lexeme(tok).starts_with('"') {
                continue;
            }
            if self.verbatim_ranges.iter().any(|range| range.contains(&i)) {
                continue;
            }
            issues.push(ParseIssue {
                kind: ParseIssueKind::DoubleQuotedString,
                span: tok.span,
            });
        }
        if !issues.is_empty() {
            self.model.parse_issues.extend(issues);
            self.model
                .parse_issues
                .sort_by_key(|issue| issue.span.start);
        }
    }

    fn record_missing_end(&mut self, keyword: &str, opener_span: Span, body_i: usize) {
        let insert_offset = if self.at_block_opener() {
            self.current_start()
        } else {
            self.src.len() as u32
        };
        let next_block_label = if self.at_block_opener() {
            let raw = self.lexeme(&self.tokens[self.i]);
            Some(
                BLOCK_OPENERS
                    .iter()
                    .copied()
                    .find(|k| raw.eq_ignore_ascii_case(k))
                    .unwrap_or(raw)
                    .to_string(),
            )
        } else {
            None
        };
        let last_stmt_semi = self.last_semi_in_body(body_i);
        self.record_issue(ParseIssue {
            kind: ParseIssueKind::MissingEnd {
                keyword: keyword.to_string(),
                last_stmt_semi,
                next_block_label,
                insert_offset,
            },
            span: opener_span,
        });
    }

    fn record_missing_end_if_unclosed(
        &mut self,
        keyword: &str,
        opener_span: Span,
        body_i: usize,
        body_end_i: usize,
    ) {
        let closed = self.i > body_end_i;
        if !closed {
            self.record_missing_end(keyword, opener_span, body_i);
        }
    }

    fn record_missing_final(&mut self, keyword: &str, body_i: usize, body_end_i: usize) {
        if body_i >= body_end_i {
            return;
        }
        let last = (body_i..body_end_i)
            .rev()
            .find(|&i| self.tokens[i].kind != TokenKind::Eof);
        let Some(last) = last else {
            return;
        };
        if self.tokens[last].kind == TokenKind::Semi {
            return;
        }
        let body_start = self.tokens[body_i].span.start;
        let body_end = self.tokens[body_end_i].span.start;
        if body_start >= body_end {
            return;
        }
        let body = &self.src[body_start as usize..body_end as usize];
        let body_code = body.trim_end();
        if body_code.is_empty() || body_code.ends_with(';') {
            return;
        }
        let body_code_end = body_start + body_code.len() as u32;
        let mut stmt_rel = body_code.rfind(';').map(|p| p + 1).unwrap_or(0);
        while stmt_rel < body_code.len() && body_code.as_bytes()[stmt_rel].is_ascii_whitespace() {
            stmt_rel += 1;
        }
        let stmt_start = body_start + stmt_rel as u32;
        self.record_issue(ParseIssue {
            kind: ParseIssueKind::MissingFinalSemi {
                keyword: keyword.to_string(),
                body_code_end,
            },
            span: Span {
                start: stmt_start,
                end: body_code_end,
            },
        });
    }

    fn last_semi_in_body(&self, body_i: usize) -> Option<u32> {
        (body_i..self.i)
            .rev()
            .find(|&i| self.tokens[i].kind == TokenKind::Semi)
            .map(|i| self.tokens[i].span.start)
    }

    fn record_keyword_typos(&mut self) {
        let mut check = Vec::new();
        if self.model.equations.is_empty() {
            check.push("model");
        }
        if self.model.endogenous.is_empty() {
            check.push("var");
        }
        if self.model.parameters.is_empty() {
            check.push("parameters");
        }
        if self.model.exogenous.is_empty() {
            check.push("varexo");
        }
        if self.model.shocks_block.is_none() && !self.model.exogenous.is_empty() {
            check.push("shocks");
        }
        if check.is_empty() {
            return;
        }
        let blocks = complete_block_ranges(&self.tokens, self.src);
        for tok in &self.tokens {
            if tok.kind != TokenKind::Ident {
                continue;
            }
            if inside_span(tok.span.start, &blocks) {
                continue;
            }
            let word = tok.text(self.src);
            if word.len() < 3 || word.len() > 12 {
                continue;
            }
            if !at_line_start_ident(self.src, tok.span) {
                continue;
            }
            let lower = word.to_ascii_lowercase();
            let Some((_, correct)) = KEYWORD_TYPO_MAP.iter().find(|(t, _)| *t == lower) else {
                continue;
            };
            if !check.iter().any(|c| c == correct) {
                continue;
            }
            self.model.parse_issues.push(ParseIssue {
                kind: ParseIssueKind::KeywordTypo {
                    found: word.to_string(),
                    correct: (*correct).to_string(),
                },
                span: tok.span,
            });
        }
    }

    fn record_missing_assign_semis(&mut self) {
        let src = self.src;
        let blocks = complete_block_ranges(&self.tokens, src);
        let cmd_spans = crate::command_skip::command_stmt_spans(&self.tokens, src);
        // Parsed family statements claim their own lines: a multi-line `sbvar` or
        // `.prior(?)` option list is not a run of parameter assignments.
        let mut family_spans: Vec<Span> = self.model.statement_spans();
        family_spans.extend(cmd_spans);
        family_spans.sort_by_key(|span| (span.start, span.end));
        let trailing = trailing_code_line(&self.tokens, src);
        let lines: Vec<&str> = src.split('\n').collect();
        for (i, line) in lines.iter().enumerate() {
            let i = i as u32;
            if trailing.is_some_and(|t| i > t) {
                break;
            }
            let line_start = if i == 0 {
                0
            } else {
                lines[..i as usize]
                    .iter()
                    .map(|l| l.len() + 1)
                    .sum::<usize>() as u32
            };
            if inside_span(line_start, &blocks) || inside_span(line_start, &family_spans) {
                continue;
            }
            let trimmed_start = line.len() - line.trim_start().len();
            let rest = &line[trimmed_start..];
            let Some(name) = leading_ident(rest) else {
                continue;
            };
            // The pin's lexer decides, as it reads, whether this line enters a
            // Dynare statement: only a head declared **before** the line — and
            // neither a mod-file local nor an external-function name — reaches the
            // grammar's `symbol EQUAL expression`. Everything else is native MATLAB
            // text, which 7.1 accepts however it reads, so the pass must not claim it.
            if !self.declared_before(line_start, name) {
                continue;
            }
            let after_name = &rest[name.len()..];
            let after_ws = after_name.trim_start();
            if !after_ws.starts_with('=') || after_ws.starts_with("==") {
                continue;
            }
            if ASSIGN_BLOCK_LIKE
                .iter()
                .any(|k| name.eq_ignore_ascii_case(k))
            {
                continue;
            }
            let rhs_raw = after_ws[1..].trim_start();
            let rhs_code = strip_line_comment(rhs_raw).trim_end();
            if rhs_code.ends_with(';') {
                continue;
            }
            if looks_like_matlab(rhs_raw) {
                continue;
            }
            let mut j = i + 1;
            while (j as usize) < lines.len() {
                let next = lines[j as usize].trim();
                if next.is_empty() {
                    j += 1;
                    continue;
                }
                let next_code = strip_line_comment(next).trim();
                let follower = leading_ident(next_code).is_some_and(|id| {
                    let after = next_code[id.len()..].trim_start();
                    (after.starts_with('=') && !after.starts_with("=="))
                        || ASSIGN_FOLLOWERS.iter().any(|f| id.eq_ignore_ascii_case(f))
                        || (crate::command_skip::is_parse_skip_command(id)
                            && (after.starts_with('(') || after.starts_with(';')))
                });
                if follower {
                    self.model.parse_issues.push(ParseIssue {
                        kind: ParseIssueKind::MissingAssignSemi {
                            name: name.to_string(),
                        },
                        span: Span {
                            start: line_start,
                            end: line_start + line.len() as u32,
                        },
                    });
                }
                break;
            }
        }
    }

    fn record_missing_shocks_semis(&mut self, from: usize, to: usize) {
        let src = self.src;
        let mut i = from;
        while i < to {
            let tok = &self.tokens[i];
            if tok.kind == TokenKind::Ident && tok.text(src).eq_ignore_ascii_case("var") {
                let var_tok = tok.clone();
                let Some(name_tok) = self
                    .tokens
                    .get(i + 1)
                    .filter(|t| t.kind == TokenKind::Ident)
                    .cloned()
                else {
                    i += 1;
                    continue;
                };
                let after_i = i + 2;
                if after_i >= to {
                    i += 1;
                    continue;
                }
                let next = &self.tokens[after_i];
                if next.kind == TokenKind::Eq || next.kind == TokenKind::Comma {
                    i += 1;
                    continue;
                }
                if next.kind == TokenKind::Semi {
                    i += 1;
                    continue;
                }
                if next.kind == TokenKind::Ident
                    && ["var", "stderr", "corr"]
                        .iter()
                        .any(|k| next.text(src).eq_ignore_ascii_case(k))
                {
                    let keyword = next.text(src).to_string();
                    let next_start = next.span.start;
                    let mut fix_start = next_start;
                    while fix_start > var_tok.span.start {
                        let b = src.as_bytes()[(fix_start - 1) as usize];
                        if b == b' ' || b == b'\t' {
                            fix_start -= 1;
                        } else {
                            break;
                        }
                    }
                    self.record_issue(ParseIssue {
                        kind: ParseIssueKind::MissingShocksSemi {
                            family: ShocksSemiFamily::BeforeKeyword,
                            label: keyword,
                            fix_start,
                            fix_end: next_start,
                        },
                        span: Span {
                            start: var_tok.span.start,
                            end: next_start,
                        },
                    });
                } else {
                    let name = name_tok.text(src).to_string();
                    self.record_issue(ParseIssue {
                        kind: ParseIssueKind::MissingShocksSemi {
                            family: ShocksSemiFamily::AfterVar,
                            label: name,
                            fix_start: name_tok.span.end,
                            fix_end: name_tok.span.end,
                        },
                        span: Span {
                            start: var_tok.span.start,
                            end: name_tok.span.end,
                        },
                    });
                }
                i += 1;
                continue;
            }
            if tok.kind == TokenKind::Ident
                && ["stderr", "corr"]
                    .iter()
                    .any(|k| tok.text(src).eq_ignore_ascii_case(k))
            {
                let stmt = tok.text(src).to_string();
                let stmt_start = tok.span.start;
                let mut k = i + 1;
                let mut semi_same_line = false;
                while k < to {
                    if self.tokens[k].kind == TokenKind::Semi {
                        let nl = src[tok.span.end as usize..self.tokens[k].span.start as usize]
                            .contains('\n');
                        if !nl {
                            semi_same_line = true;
                        }
                        break;
                    }
                    if self.tokens[k].kind == TokenKind::Ident
                        && ["var", "stderr", "corr"]
                            .iter()
                            .any(|n| self.tokens[k].text(src).eq_ignore_ascii_case(n))
                    {
                        break;
                    }
                    k += 1;
                }
                if !semi_same_line {
                    let line = line_of(src, stmt_start).1;
                    let code = strip_line_comment(line);
                    let line_start = src[..stmt_start as usize]
                        .rfind('\n')
                        .map(|p| p + 1)
                        .unwrap_or(0);
                    let abs_end = line_start as u32 + code.len() as u32;
                    self.record_issue(ParseIssue {
                        kind: ParseIssueKind::MissingShocksSemi {
                            family: ShocksSemiFamily::EndOfStmt,
                            label: stmt,
                            fix_start: abs_end,
                            fix_end: abs_end,
                        },
                        span: Span {
                            start: stmt_start,
                            end: abs_end,
                        },
                    });
                }
            }
            i += 1;
        }
    }

    fn parse_expr(&mut self) -> Option<ExprId> {
        self.parse_bp(0)
    }

    fn parse_bp(&mut self, min_bp: u8) -> Option<ExprId> {
        let mut lhs = self.parse_prefix()?;
        while let Some((l_bp, r_bp, op)) = self.infix_op() {
            if l_bp < min_bp {
                break;
            }
            self.bump();
            let rhs = match self.parse_bp(r_bp) {
                Some(id) => id,
                None => self.alloc_error(self.expr_span(lhs)),
            };
            let span = Span {
                start: self.expr_span(lhs).start,
                end: self.expr_span(rhs).end,
            };
            lhs = self.alloc(ExprKind::Binary { op, lhs, rhs }, span);
        }
        Some(lhs)
    }

    fn parse_prefix(&mut self) -> Option<ExprId> {
        if self.at_expr_stop() {
            return None;
        }
        if self.at(TokenKind::Plus) || self.at(TokenKind::Minus) {
            let op = if self.at(TokenKind::Plus) {
                UnOp::Pos
            } else {
                UnOp::Neg
            };
            let tok = self.bump();
            let arg = match self.parse_bp(UNARY_BP) {
                Some(id) => id,
                None => self.alloc_error(tok.span),
            };
            let span = Span {
                start: tok.span.start,
                end: self.expr_span(arg).end,
            };
            return Some(self.alloc(ExprKind::Unary { op, arg }, span));
        }
        if self.at(TokenKind::LParen) {
            self.bump();
            let inner = self.parse_bp(0);
            self.eat(TokenKind::RParen);
            return inner;
        }
        if self.at(TokenKind::Number) {
            let tok = self.bump();
            return Some(self.alloc(ExprKind::Number, tok.span));
        }
        if self.at(TokenKind::String) {
            let tok = self.bump();
            return Some(self.alloc(ExprKind::String, tok.span));
        }
        if self.at(TokenKind::Ident) {
            return Some(self.parse_ident_expr());
        }
        None
    }

    /// The cursor is just past an `end` word. It is the bare closer when it
    /// heads the statement and the next token is not `=` or an operator.
    fn end_is_bare_closer(&self) -> bool {
        let end_i = self.i.saturating_sub(1);
        let boundary = end_i == 0
            || matches!(
                self.tokens[end_i - 1].kind,
                TokenKind::Semi | TokenKind::MacroDir
            );
        if !boundary {
            return false;
        }
        matches!(
            self.tokens.get(self.i).map(|t| t.kind),
            None | Some(TokenKind::Eof | TokenKind::Ident | TokenKind::Semi)
        )
    }

    fn parse_ident_expr(&mut self) -> ExprId {
        let tok = self.bump();
        let lexeme = self.lexeme(&tok).to_string();
        // Inside a block the word `end` is the closer, never a name. `+ end + 0`
        // and `end = 0.1*y` are `syntax error, unexpected END`. `end;` is the
        // closer and never reaches here.
        // A bare `end` with no `;` is the closer that returns the lexer to
        // `INITIAL`. That shape keeps the missing-`end` diagnostic. `end` after
        // an operator, or `end = …`, is the END token inside the equation.
        if self.in_equation_body && lexeme.eq_ignore_ascii_case("end") && !self.end_is_bare_closer()
        {
            self.record_issue(ParseIssue {
                kind: ParseIssueKind::UnexpectedEnd,
                span: tok.span,
            });
        }
        let name = self.intern.intern(&lexeme);
        if self.at(TokenKind::Dot) && self.peek_kind(1) == Some(TokenKind::Ident) {
            self.bump();
            let rhs = self.bump();
            let rhs_lex = self.lexeme(&rhs).to_string();
            let span = Span {
                start: tok.span.start,
                end: rhs.span.end,
            };
            self.model
                .namespace_qualified
                .push((format!("{lexeme}.{rhs_lex}"), span));
            return self.alloc(ExprKind::Error, span);
        }
        let becoming_call = self.at(TokenKind::LParen) && !self.looks_like_timing();
        if !self.in_model
            && !becoming_call
            && !is_builtin_function(&lexeme)
            && !self.is_known_symbol(name)
            && !self.model.mod_file_locals.contains(&name)
        {
            self.model.mod_file_locals.push(name);
        }
        if !self.at(TokenKind::LParen) {
            return self.alloc(
                ExprKind::Ident {
                    name,
                    timing: 0,
                    ident_span: tok.span,
                    timing_span: None,
                },
                tok.span,
            );
        }
        if lexeme.eq_ignore_ascii_case("expectation") && self.looks_like_expectation() {
            return self.parse_expectation(tok);
        }
        if lexeme.eq_ignore_ascii_case("steady_state") {
            return self.parse_steady_state(tok);
        }
        if let Some(kind) = pac_parser::named_operator_kind(&lexeme) {
            return self.parse_named_model_operator(tok, name, kind);
        }
        if self.looks_like_timing() && !is_builtin_function(&lexeme) {
            return self.parse_timing(name, tok);
        }
        self.parse_call(name, tok)
    }

    fn parse_expectation(&mut self, kw: Token) -> ExprId {
        let (shift, _) = self.parse_signed_int_in_parens();
        self.eat(TokenKind::LParen);
        let arg = match self.parse_expr() {
            Some(id) => id,
            None => self.alloc_error(kw.span),
        };
        let end = if self.at(TokenKind::RParen) {
            self.bump().span.end
        } else {
            self.expr_span(arg).end
        };
        self.alloc(
            ExprKind::Expectation { shift, arg },
            Span {
                start: kw.span.start,
                end,
            },
        )
    }

    fn parse_steady_state(&mut self, kw: Token) -> ExprId {
        self.eat(TokenKind::LParen);
        let arg = match self.parse_expr() {
            Some(id) => id,
            None => self.alloc_error(kw.span),
        };
        let end = if self.at(TokenKind::RParen) {
            self.bump().span.end
        } else {
            self.expr_span(arg).end
        };
        self.alloc(
            ExprKind::SteadyState { arg },
            Span {
                start: kw.span.start,
                end,
            },
        )
    }

    fn parse_timing(&mut self, name: Name, ident: Token) -> ExprId {
        let (timing, timing_span) = self.parse_signed_int_in_parens();
        self.alloc(
            ExprKind::Ident {
                name,
                timing,
                ident_span: ident.span,
                timing_span: Some(timing_span),
            },
            Span {
                start: ident.span.start,
                end: timing_span.end,
            },
        )
    }

    fn parse_call(&mut self, callee: Name, kw: Token) -> ExprId {
        self.eat(TokenKind::LParen);
        let mut args = Vec::new();
        if !self.at(TokenKind::RParen) && !self.at_expr_stop() {
            loop {
                if let Some(id) = self.parse_expr() {
                    args.push(id);
                }
                if self.at(TokenKind::Comma) {
                    self.bump();
                    continue;
                }
                break;
            }
        }
        let end = if self.at(TokenKind::RParen) {
            self.bump().span.end
        } else {
            args.last()
                .map(|id| self.expr_span(*id).end)
                .unwrap_or(kw.span.end)
        };
        self.alloc(
            ExprKind::Call { callee, args },
            Span {
                start: kw.span.start,
                end,
            },
        )
    }

    fn parse_signed_int_in_parens(&mut self) -> (i32, Span) {
        let start = self.bump().span.start;
        let mut sign = 1i32;
        if self.at(TokenKind::Plus) {
            self.bump();
        } else if self.at(TokenKind::Minus) {
            self.bump();
            sign = -1;
        }
        let num = self.bump();
        let mag: i32 = self.lexeme(&num).parse().unwrap_or(0);
        let end = if self.at(TokenKind::RParen) {
            self.bump().span.end
        } else {
            num.span.end
        };
        (sign * mag, Span { start, end })
    }

    fn is_known_symbol(&self, name: Name) -> bool {
        self.model
            .endogenous
            .iter()
            .chain(&self.model.exogenous)
            .chain(&self.model.deterministic_exogenous)
            .chain(&self.model.parameters)
            .chain(&self.model.predetermined)
            .any(|d| d.name == name)
            || self.model.mod_file_locals.contains(&name)
            || self.model.trend_vars.iter().any(|trend| trend.name == name)
            || self.model.external_function_names.contains(&name)
            || self.model.equations.iter().any(|eq| {
                eq.is_local
                    && matches!(
                        eq.lhs_expr.map(|id| &self.model.exprs.get(id).kind),
                        Some(ExprKind::Ident { name: local, .. }) if *local == name
                    )
            })
    }

    fn looks_like_timing(&self) -> bool {
        if !self.at(TokenKind::LParen) {
            return false;
        }
        let mut k = 1;
        if matches!(self.peek_kind(k), Some(TokenKind::Plus | TokenKind::Minus)) {
            k += 1;
        }
        match (self.peek_tok(k), self.peek_kind(k + 1)) {
            (Some(num), Some(TokenKind::RParen)) if num.kind == TokenKind::Number => {
                is_integer_lexeme(self.lexeme(num))
            }
            _ => false,
        }
    }

    fn looks_like_expectation(&self) -> bool {
        if !self.at(TokenKind::LParen) {
            return false;
        }
        let mut k = 1;
        if matches!(self.peek_kind(k), Some(TokenKind::Plus | TokenKind::Minus)) {
            k += 1;
        }
        match (
            self.peek_tok(k),
            self.peek_kind(k + 1),
            self.peek_kind(k + 2),
        ) {
            (Some(num), Some(TokenKind::RParen), Some(TokenKind::LParen))
                if num.kind == TokenKind::Number =>
            {
                is_integer_lexeme(self.lexeme(num))
            }
            _ => false,
        }
    }

    fn infix_op(&self) -> Option<(u8, u8, BinOp)> {
        match self.tokens.get(self.i)?.kind {
            TokenKind::Plus => Some((3, 4, BinOp::Add)),
            TokenKind::Minus => Some((3, 4, BinOp::Sub)),
            TokenKind::Star => Some((5, 6, BinOp::Mul)),
            TokenKind::Slash => Some((5, 6, BinOp::Div)),
            TokenKind::Caret => Some((9, 8, BinOp::Pow)),
            TokenKind::Lt => Some((1, 2, BinOp::Lt)),
            TokenKind::Gt => Some((1, 2, BinOp::Gt)),
            TokenKind::Le => Some((1, 2, BinOp::Le)),
            TokenKind::Ge => Some((1, 2, BinOp::Ge)),
            TokenKind::EqEq => Some((1, 2, BinOp::EqEq)),
            TokenKind::Ne => Some((1, 2, BinOp::Ne)),
            _ => None,
        }
    }

    fn at_expr_stop(&self) -> bool {
        matches!(
            self.tokens.get(self.i).map(|t| t.kind),
            Some(
                TokenKind::Eof
                    | TokenKind::Semi
                    | TokenKind::Eq
                    | TokenKind::Comma
                    | TokenKind::RParen
                    | TokenKind::RBrack
                    | TokenKind::Perpendicular
            )
        ) || self.at_block_stop()
    }

    fn alloc(&mut self, kind: ExprKind, span: Span) -> ExprId {
        if self.in_equation_body {
            if let ExprKind::Ident {
                name, ident_span, ..
            } = &kind
            {
                if self.model.var_removed.iter().any(|removed| {
                    removed.name == *name && removed.statement.end <= ident_span.start
                }) {
                    self.model.var_removed_model_uses.push((*name, *ident_span));
                }
            }
        }
        let interned = match &kind {
            ExprKind::Number => self
                .src
                .get(span.start as usize..span.end as usize)
                .and_then(|raw| raw.parse::<f64>().ok())
                .filter(|v| v.is_finite()),
            ExprKind::Unary { op, arg } => self.interned_value(*arg).map(|v| match op {
                UnOp::Pos => v,
                UnOp::Neg => -v,
            }),
            ExprKind::Binary { op, lhs, rhs } => {
                let l = self.interned_value(*lhs);
                let r = self.interned_value(*rhs);
                match (*op, l, r) {
                    (BinOp::Add, Some(a), Some(b)) => Some(a + b),
                    (BinOp::Sub, Some(a), Some(b)) => Some(a - b),
                    (BinOp::Mul, Some(a), Some(b)) => Some(a * b),
                    (BinOp::Div, Some(a), Some(b)) if b != 0.0 => Some(a / b),
                    (BinOp::Pow, Some(a), Some(b)) => Some(a.powf(b)),
                    _ => None,
                }
            }
            _ => None,
        };
        let interned = interned.filter(|v| v.is_finite());
        let id = self.model.exprs.alloc_interned(kind, span, interned);
        self.note_const_fold_errors(id);
        id
    }

    fn interned_value(&self, id: ExprId) -> Option<f64> {
        self.model.exprs.get(id).interned
    }

    fn interned_display(&self, id: ExprId) -> String {
        match self.interned_value(id) {
            Some(0.0) => "0".to_string(),
            Some(1.0) => "1".to_string(),
            Some(v) if v.fract() == 0.0 && v.abs() < 1e15 => format!("{}", v as i64),
            Some(v) => format!("{v}"),
            None => {
                let span = self.expr_span(id);
                self.src
                    .get(span.start as usize..span.end as usize)
                    .unwrap_or("?")
                    .to_string()
            }
        }
    }

    /// The small simplifications `DataTree::AddPlus/AddMinus/AddTimes` make while
    /// building an expression. This is deliberately local: assigned parameter
    /// values and broader algebraic identities are not available at parse.
    fn folded_key(&self, id: ExprId) -> FoldKey {
        if let Some(value) = self.interned_value(id) {
            return FoldKey::number(value);
        }
        match &self.model.exprs.get(id).kind {
            ExprKind::Ident { name, timing, .. } => FoldKey::Ident(*name, *timing),
            ExprKind::Unary { op, arg } => {
                let inner = self.folded_key(*arg);
                match op {
                    UnOp::Pos => inner,
                    UnOp::Neg => FoldKey::neg(inner),
                }
            }
            ExprKind::Binary { op, lhs, rhs } => {
                let left = self.folded_key(*lhs);
                let right = self.folded_key(*rhs);
                match op {
                    BinOp::Add => FoldKey::add(left, right),
                    BinOp::Sub => FoldKey::sub(left, right),
                    BinOp::Mul => FoldKey::mul(left, right),
                    _ => FoldKey::Binary(*op, Box::new(left), Box::new(right)),
                }
            }
            ExprKind::Call { callee, args } => FoldKey::Call(
                *callee,
                args.iter().map(|arg| self.folded_key(*arg)).collect(),
            ),
            ExprKind::SteadyState { arg } => FoldKey::SteadyState(Box::new(self.folded_key(*arg))),
            ExprKind::Expectation { shift, arg } => {
                FoldKey::Expectation(*shift, Box::new(self.folded_key(*arg)))
            }
            _ => FoldKey::Other(id),
        }
    }

    fn note_const_fold_errors(&mut self, id: ExprId) {
        let span = self.expr_span(id);
        let log_zero = match &self.model.exprs.get(id).kind {
            ExprKind::Call { callee, args } if args.len() == 1 => {
                let arg = args[0];
                let name = self.intern.get(*callee).to_string();
                Some((name, arg))
            }
            _ => None,
        };
        if let Some((name, arg)) = log_zero {
            if self.interned_value(arg) == Some(0.0) {
                if name.eq_ignore_ascii_case("log") || name.eq_ignore_ascii_case("ln") {
                    self.model.const_fold_errors.push((
                        span,
                        "E276",
                        "log(0) not defined!".to_string(),
                    ));
                } else if name.eq_ignore_ascii_case("log10") {
                    self.model.const_fold_errors.push((
                        span,
                        "E277",
                        "log10(0) not defined!".to_string(),
                    ));
                }
            }
            return;
        }
        let div_zero = match &self.model.exprs.get(id).kind {
            ExprKind::Binary {
                op: BinOp::Div,
                lhs,
                rhs,
            } => Some((*lhs, *rhs)),
            _ => None,
        };
        if let Some((lhs, rhs)) = div_zero {
            if self.folded_key(rhs).is_zero() {
                let num = if self.folded_key(lhs).is_zero() {
                    "0".to_string()
                } else {
                    self.interned_display(lhs)
                };
                self.model.const_fold_errors.push((
                    span,
                    "E278",
                    format!(
                        "Division by zero when forming ({num})/(0); denominator simplified to 0 (possibly after substituting a variable set to 0)."
                    ),
                ));
            }
        }
    }

    fn alloc_error(&mut self, span: Span) -> ExprId {
        self.alloc(ExprKind::Error, span)
    }

    fn expr_span(&self, id: ExprId) -> Span {
        self.model.exprs.get(id).span
    }

    fn peek_tok(&self, ahead: usize) -> Option<&Token> {
        self.tokens.get(self.i + ahead)
    }

    fn skip_until_semi(&mut self) {
        self.symbol_list_id += 1;
        let mut saw_ident = false;
        let mut opener: Option<String> = None;
        let mut opener_span = Span::default();
        let mut saw_datafile = false;
        let mut estimation_data_options = Vec::new();
        let mut stoch_options = None;
        let mut saw_trailing_symbol = false;
        while !self.at(TokenKind::Semi) && !self.at(TokenKind::Eof) {
            if !saw_ident && self.at(TokenKind::Ident) {
                saw_ident = true;
                let tok = self.tokens[self.i].clone();
                let lex = self.lexeme(&tok).to_string();
                self.record_top_command(&lex, tok.span);
                opener_span = tok.span;
                opener = Some(lex);
                self.bump();
                continue;
            }
            if self.at(TokenKind::LParen) {
                let from = self.i;
                self.skip_balanced(TokenKind::LParen, TokenKind::RParen);
                if saw_trailing_symbol {
                    self.model.shape_refuses.push(ShapeRefuse::official(
                        self.tokens[from].span,
                        opener.as_deref().unwrap_or("symbol list"),
                        "syntax error, unexpected '('",
                    ));
                    continue;
                }
                self.record_deprecated_options_in_range(from, self.i);
                if let Some(cmd) = opener.as_deref() {
                    if handed_option_command(cmd) {
                        let options = self.read_family_options(from, self.i);
                        if options.is_empty() {
                            self.model.shape_refuses.push(ShapeRefuse::official(
                                self.tokens[self.i.saturating_sub(1)].span,
                                cmd,
                                "syntax error, unexpected ')'",
                            ));
                        }
                        if let Some(refuse) =
                            crate::shape_gate::handed_option_refusal(self.src, cmd, &options)
                        {
                            self.model.shape_refuses.push(refuse);
                        }
                    }
                    if cmd.eq_ignore_ascii_case("stoch_simul") {
                        stoch_options = Some((from, self.i));
                    }
                    saw_datafile =
                        self.record_skip_command_options(cmd, from, self.i) || saw_datafile;
                    if cmd.eq_ignore_ascii_case("estimation") {
                        estimation_data_options.extend(
                            self.read_family_options(from, self.i)
                                .into_iter()
                                .filter(|option| {
                                    option.name.eq_ignore_ascii_case("datafile")
                                        || option.name.eq_ignore_ascii_case("first_obs")
                                }),
                        );
                    }
                    self.record_option_twice(from, self.i);
                    if cmd.eq_ignore_ascii_case("stoch_simul")
                        || cmd.eq_ignore_ascii_case("estimation")
                    {
                        self.collect_irf_shocks_option(cmd, from, self.i);
                    }
                    if cmd.eq_ignore_ascii_case("extended_path")
                        && self.option_ident_in_range(from, self.i, "periods")
                    {
                        self.model.extended_path_has_periods = true;
                    }
                }
                continue;
            }
            if self.at(TokenKind::Ident)
                && opener.as_deref().is_some_and(is_trailing_symbol_command)
            {
                let cmd = opener.as_deref().unwrap().to_string();
                let word = self.lexeme(&self.tokens[self.i]).to_string();
                if let Some(message) = reserved_trailing_option_error(&word) {
                    let span = self.tokens[self.i].span;
                    self.model
                        .shape_refuses
                        .push(ShapeRefuse::official(span, &cmd, message));
                    self.bump();
                    continue;
                }
                self.push_command_symbol(&cmd);
                saw_trailing_symbol = true;
                continue;
            }
            if self.at(TokenKind::Eq) && saw_trailing_symbol {
                self.model.shape_refuses.push(ShapeRefuse::official(
                    self.tokens[self.i].span,
                    opener.as_deref().unwrap_or("symbol list"),
                    "syntax error, unexpected EQUAL",
                ));
            }
            self.bump();
        }
        // One row per `estimation` statement: 7.1's data gate is per statement and
        // reads the `data` flag in file order, so the gate needs both.
        if opener.as_deref() == Some("estimation") {
            self.model.estimation_statements.push(EstimationStatement {
                span: opener_span,
                has_datafile: saw_datafile,
                data_options: estimation_data_options,
            });
        }
        if opener.as_deref() == Some("stoch_simul") {
            let end = self
                .tokens
                .get(self.i)
                .map(|token| token.span.end)
                .unwrap_or(opener_span.end);
            self.collect_stoch_simul_request(
                Span {
                    start: opener_span.start,
                    end,
                },
                stoch_options,
            );
        }
        self.eat(TokenKind::Semi);
    }

    /// Record the extras one catalogued command's `(?)` list carries. Returns
    /// whether this statement listed `datafile=` (only `estimation` asks).
    fn record_skip_command_options(&mut self, opener: &str, from: usize, to: usize) -> bool {
        let mut saw_datafile = false;
        self.collect_date_options(opener, from, to);
        if opener.eq_ignore_ascii_case("prior_function")
            || opener.eq_ignore_ascii_case("posterior_function")
        {
            self.model.prior_function_has_parens = true;
        }
        let opts = top_options(&self.tokens, self.src, from, to);
        for opt in &opts {
            if !opt.eq || !date_option_consumer(opener, &opt.ident) {
                continue;
            }
            let Some(value_at) = (from..to.min(self.tokens.len()))
                .find(|&i| self.tokens[i].span.start == opt.value_span.start)
            else {
                continue;
            };
            let Some((_, next)) = self.date_at(value_at) else {
                continue;
            };
            let minus_message = if opener.eq_ignore_ascii_case("plot_shock_decomposition")
                || opener.eq_ignore_ascii_case("initial_condition_decomposition")
            {
                "syntax error, unexpected MINUS, expecting ')'"
            } else {
                "syntax error, unexpected MINUS, expecting COMMA or ')'"
            };
            if let Some(refuse) = self.date_suffix_refusal_at(next, opener, minus_message) {
                self.model.shape_refuses.push(refuse);
            }
        }
        if opener.eq_ignore_ascii_case("perfect_foresight_setup")
            || opener.eq_ignore_ascii_case("perfect_foresight_with_expectation_errors_setup")
        {
            for opt in &opts {
                if !matches!(
                    opt.ident.to_ascii_lowercase().as_str(),
                    "first_simulation_period" | "last_simulation_period"
                ) {
                    continue;
                }
                let parsed_date = self.model.date_options.iter().any(|row| {
                    row.command.eq_ignore_ascii_case(opener)
                        && row.name.eq_ignore_ascii_case(&opt.ident)
                        && row.span.start == opt.span.start
                });
                if !parsed_date && opt.eq && opt.value_lex.parse::<i64>().is_ok() {
                    self.model.shape_refuses.push(ShapeRefuse::official(
                        opt.value_span,
                        opener,
                        "syntax error, unexpected INT_NUMBER, expecting DATE",
                    ));
                }
            }
        }
        if opener.eq_ignore_ascii_case("histval_file") {
            for opt in &opts {
                let message = if opt.ident.eq_ignore_ascii_case("nobs") {
                    Some("syntax error, unexpected NOBS")
                } else if opt.ident.eq_ignore_ascii_case("last_simulation_period") {
                    Some("syntax error, unexpected LAST_SIMULATION_PERIOD")
                } else {
                    None
                };
                if let Some(message) = message {
                    self.model
                        .shape_refuses
                        .push(ShapeRefuse::official(opt.span, opener, message));
                }
            }
        }
        if is_decomposition_command(opener) && self.model.with_epilogue_span.is_none() {
            if let Some(opt) = opts
                .iter()
                .find(|o| o.ident.eq_ignore_ascii_case("with_epilogue"))
            {
                self.model.with_epilogue_span = Some(opt.span);
            }
        }
        let mut stmt_estimated = None;
        let mut stmt_calibrated = None;
        for opt in &opts {
            if opener.eq_ignore_ascii_case("external_function")
                && opt.ident.eq_ignore_ascii_case("name")
                && opt.eq
                && !opt.value_lex.is_empty()
            {
                let id = self.intern.intern(&opt.value_lex);
                if !self.model.external_function_names.contains(&id) {
                    self.model.external_function_names.push(id);
                }
            }
            if opt.ident.eq_ignore_ascii_case("restriction_fname")
                && self.model.restriction_fname_span.is_none()
            {
                self.model.restriction_fname_span = Some(opt.span);
            }
            if opener.eq_ignore_ascii_case("estimation") {
                if opt.ident.eq_ignore_ascii_case("dsge_var") {
                    if opt.eq {
                        if stmt_calibrated.is_none() {
                            stmt_calibrated = Some(opt.span);
                        }
                    } else if stmt_estimated.is_none() {
                        stmt_estimated = Some(opt.span);
                    }
                } else if opt.ident.eq_ignore_ascii_case("dsge_varlag")
                    && self.model.dsge_varlag_span.is_none()
                {
                    self.model.dsge_varlag_span = Some(opt.span);
                } else if opt.ident.eq_ignore_ascii_case("bayesian_irf")
                    && self.model.bayesian_irf_span.is_none()
                {
                    self.model.bayesian_irf_span = Some(opt.span);
                } else if opt.ident.eq_ignore_ascii_case("datafile") && opt.eq {
                    saw_datafile = true;
                    if self.model.estimation_datafile_span.is_none() {
                        self.model.estimation_datafile_span = Some(opt.span);
                    }
                } else if opt.ident.eq_ignore_ascii_case("dataseries")
                    && opt.eq
                    && self.model.estimation_dataseries_span.is_none()
                {
                    self.model.estimation_dataseries_span = Some(opt.span);
                } else if opt.ident.eq_ignore_ascii_case("mode_file")
                    && opt.eq
                    && self.model.estimation_mode_file_span.is_none()
                {
                    self.model.estimation_mode_file_span = Some(opt.span);
                } else if opt.ident.eq_ignore_ascii_case("mh_tune_jscale")
                    && self.model.mh_tune_jscale_span.is_none()
                {
                    self.model.mh_tune_jscale_span = Some(opt.span);
                } else if opt.ident.eq_ignore_ascii_case("mh_jscale")
                    && opt.eq
                    && self.model.mh_jscale_span.is_none()
                {
                    self.model.mh_jscale_span = Some(opt.span);
                } else if opt.ident.eq_ignore_ascii_case("mh_tune_guess")
                    && opt.eq
                    && self.model.mh_tune_guess_span.is_none()
                {
                    self.model.mh_tune_guess_span = Some(opt.span);
                } else if opt.ident.eq_ignore_ascii_case("filter_algorithm")
                    && opt.value_lex.eq_ignore_ascii_case("gmf")
                    && self.model.filter_algorithm_gmf_span.is_none()
                {
                    self.model.filter_algorithm_gmf_span = Some(opt.span);
                } else if opt.ident.eq_ignore_ascii_case("proposal_approximation")
                    && opt.value_lex.eq_ignore_ascii_case("montecarlo")
                    && self.model.proposal_approximation_montecarlo_span.is_none()
                {
                    self.model.proposal_approximation_montecarlo_span = Some(opt.span);
                } else if opt.ident.eq_ignore_ascii_case("distribution_approximation")
                    && opt.value_lex.eq_ignore_ascii_case("montecarlo")
                    && self
                        .model
                        .distribution_approximation_montecarlo_span
                        .is_none()
                {
                    self.model.distribution_approximation_montecarlo_span = Some(opt.span);
                }
            } else if opener.eq_ignore_ascii_case("sensitivity") {
                if opt.ident.eq_ignore_ascii_case("identification")
                    && opt.eq
                    && opt.value_lex == "1"
                    && self.model.sensitivity_identification_eq_1.is_none()
                {
                    self.model.sensitivity_identification_eq_1 = Some(opt.span);
                }
            } else if opener.eq_ignore_ascii_case("identification") {
                if opt.ident.eq_ignore_ascii_case("order") {
                    if let Some(n) = parse_int_lexeme(&opt.value_lex) {
                        if self.model.identification_order.is_none() {
                            self.model.identification_order = Some((n, opt.value_span));
                        }
                    }
                } else if opt.ident.eq_ignore_ascii_case("max_dim_cova_group") {
                    if let Some(n) = parse_int_lexeme(&opt.value_lex) {
                        if self.model.max_dim_cova_group.is_none() {
                            self.model.max_dim_cova_group = Some((n, opt.value_span));
                        }
                    }
                }
            } else if opener.eq_ignore_ascii_case("stoch_simul") {
                if opt.ident.eq_ignore_ascii_case("hp_filter")
                    && self.model.stoch_simul_hp_filter.is_none()
                {
                    self.model.stoch_simul_hp_filter = Some(opt.span);
                } else if opt.ident.eq_ignore_ascii_case("one_sided_hp_filter")
                    && self.model.stoch_simul_one_sided_hp_filter.is_none()
                {
                    self.model.stoch_simul_one_sided_hp_filter = Some(opt.span);
                } else if opt.ident.eq_ignore_ascii_case("bandpass_filter")
                    && self.model.stoch_simul_bandpass_filter.is_none()
                {
                    self.model.stoch_simul_bandpass_filter = Some(opt.span);
                }
            } else if (opener.eq_ignore_ascii_case("prior_function")
                || opener.eq_ignore_ascii_case("posterior_function"))
                && opt.ident.eq_ignore_ascii_case("function")
                && opt.eq
            {
                self.model.prior_function_has_function = true;
            } else if opener.eq_ignore_ascii_case("estimated_params_init")
                && opt.ident.eq_ignore_ascii_case("use_calibration")
                && self.model.estimated_params_init_use_calibration.is_none()
            {
                self.model.estimated_params_init_use_calibration = Some(opt.span);
            }
        }
        if opener.eq_ignore_ascii_case("estimation")
            && (stmt_estimated.is_some() || stmt_calibrated.is_some())
        {
            if self.model.dsge_var_estimated.is_none() {
                self.model.dsge_var_estimated = stmt_estimated;
            }
            if self.model.dsge_var_calibrated.is_none() {
                self.model.dsge_var_calibrated = stmt_calibrated;
            }
            self.model
                .estimation_dsge_var_stmts
                .push(EstimationDsgeVarStmt {
                    estimated: stmt_estimated,
                    calibrated: stmt_calibrated,
                });
        }
        saw_datafile
    }

    fn record_policy_option_flags(&mut self, command: PolicyCommand, from: usize, to: usize) {
        if command != PolicyCommand::DiscretionaryPolicy || self.model.discretionary_order.is_some()
        {
            return;
        }
        for opt in top_options(&self.tokens, self.src, from, to) {
            if opt.ident.eq_ignore_ascii_case("order") {
                if let Some(n) = parse_int_lexeme(&opt.value_lex) {
                    self.model.discretionary_order = Some((n, opt.value_span));
                    break;
                }
            }
        }
    }

    fn collect_trailing_symbols(&mut self, command: &str) {
        self.symbol_list_id += 1;
        while !self.at(TokenKind::Semi) && !self.at(TokenKind::Eof) {
            if self.at(TokenKind::Ident) {
                self.push_command_symbol(command);
            } else {
                self.bump();
            }
        }
    }

    fn push_command_symbol(&mut self, command: &str) {
        let tok = self.tokens[self.i].clone();
        let lex = self.lexeme(&tok).to_string();
        let span = tok.span;
        self.bump();
        let name = self.intern.intern(&lex);
        self.model.command_symbols.push(CommandSymbol {
            // 7.1 names the command in its sentence with its own lowercase word:
            // `STOCH_SIMUL z;` prints `stoch_simul: Variable z was not declared.`
            command: command.to_ascii_lowercase(),
            name,
            span,
            list_id: self.symbol_list_id,
        });
    }

    fn record_deprecated_option_ident(&mut self, lex: &str, span: Span) {
        let option = if lex.eq_ignore_ascii_case("aim_solver") {
            DeprecatedOption::AimSolver
        } else if lex.eq_ignore_ascii_case("bytecode") {
            DeprecatedOption::Bytecode
        } else {
            return;
        };
        self.model.deprecated_option_spans.push((option, span));
    }

    fn record_deprecated_options_in_range(&mut self, from: usize, to: usize) {
        let hits: Vec<(String, Span)> = self.tokens[from..to.min(self.tokens.len())]
            .iter()
            .filter(|t| t.kind == TokenKind::Ident)
            .map(|t| (t.text(self.src).to_string(), t.span))
            .collect();
        for (lex, span) in hits {
            self.record_deprecated_option_ident(&lex, span);
        }
    }

    fn record_option_twice(&mut self, from: usize, to: usize) {
        let opts = top_options(&self.tokens, self.src, from, to);
        let mut seen: HashMap<String, Span> = HashMap::new();
        for opt in opts {
            let key = opt.ident.to_ascii_lowercase();
            if let Entry::Vacant(e) = seen.entry(key) {
                e.insert(opt.span);
            } else {
                self.model.option_twice.push((opt.ident, opt.span));
            }
        }
    }

    /// The same check on already-parsed rows. 7.1 counts one `name=value` per
    /// option, so a name inside a bracketed value (`parameters=[alpha, alpha]`) is
    /// not a repeat, while two options with the same name are.
    fn record_parsed_option_twice(&mut self, options: &[FamilyOption]) {
        let mut seen: HashMap<String, ()> = HashMap::new();
        for opt in options {
            let key = opt.name.to_ascii_lowercase();
            if seen.insert(key, ()).is_none() {
                continue;
            }
            self.model.option_twice.push((opt.name.clone(), opt.span));
        }
    }

    /// Record the statement when its option list is outside every production the
    /// grammar gives that command. 7.1's parser stops on the offending token, so
    /// the refuse points at the option's own name (or at the list's own token when
    /// the list is empty or missing).
    fn record_option_shape_refuse(
        &mut self,
        command: &str,
        options: &[FamilyOption],
        open_i: usize,
    ) {
        let Some(table) = crate::shape_gate::command_options(command) else {
            return;
        };
        if options.is_empty() {
            // `name()` — the grammar's list needs at least one option.
            let at = self.tokens[open_i].span;
            self.model
                .shape_refuses
                .push(ShapeRefuse::new(at, command, "at least one option"));
            return;
        }
        if let Some(refuse) = crate::shape_gate::option_refusal(self.src, command, options, table) {
            self.model.shape_refuses.push(refuse);
        }
    }

    fn skip_balanced(&mut self, open: TokenKind, close: TokenKind) -> Span {
        let start = self.bump().span.start;
        let mut depth = 1;
        while !self.at(TokenKind::Eof) {
            if self.at(open) {
                depth += 1;
                self.bump();
            } else if self.at(close) {
                depth -= 1;
                let tok = self.bump();
                if depth == 0 {
                    return Span {
                        start,
                        end: tok.span.end,
                    };
                }
            } else {
                self.bump();
            }
        }
        Span {
            start,
            end: self.current_start(),
        }
    }

    fn record_top_command(&mut self, lex: &str, span: Span) {
        if lex.eq_ignore_ascii_case("simul") {
            self.model.simul_spans.push(span);
            return;
        }
        let dest = if lex.eq_ignore_ascii_case("identification") {
            &mut self.model.identification_span
        } else if lex.eq_ignore_ascii_case("perfect_foresight_solver") {
            &mut self.model.perfect_foresight_solver_span
        } else if lex.eq_ignore_ascii_case("perfect_foresight_with_expectation_errors_solver") {
            &mut self.model.pfee_solver_span
        } else if lex.eq_ignore_ascii_case("extended_path") {
            &mut self.model.extended_path_span
        } else if lex.eq_ignore_ascii_case("method_of_moments") {
            &mut self.model.method_of_moments_span
        } else if lex.eq_ignore_ascii_case("sensitivity") {
            &mut self.model.sensitivity_span
        } else if lex.eq_ignore_ascii_case("check") {
            &mut self.model.check_span
        } else if lex.eq_ignore_ascii_case("steady") {
            &mut self.model.steady_span
        } else if lex.eq_ignore_ascii_case("stoch_simul") {
            &mut self.model.stoch_simul_span
        } else if lex.eq_ignore_ascii_case("estimation") {
            &mut self.model.estimation_span
        } else if lex.eq_ignore_ascii_case("calib_smoother") {
            &mut self.model.calib_smoother_span
        } else if lex.eq_ignore_ascii_case("perfect_foresight_setup") {
            &mut self.model.perfect_foresight_setup_span
        } else if lex.eq_ignore_ascii_case("perfect_foresight_with_expectation_errors_setup") {
            &mut self.model.pfee_setup_span
        } else if lex.eq_ignore_ascii_case("write_latex_steady_state_model") {
            &mut self.model.write_latex_steady_state_model_span
        } else if lex.eq_ignore_ascii_case("prior_function") {
            &mut self.model.prior_function_span
        } else if lex.eq_ignore_ascii_case("posterior_function") {
            &mut self.model.posterior_function_span
        } else {
            return;
        };
        if dest.is_none() {
            *dest = Some(span);
        }
    }

    fn record_model_option_flags(&mut self, from: usize, to: usize) {
        let hits: Vec<(String, Span)> = self.tokens[from..to.min(self.tokens.len())]
            .iter()
            .filter(|t| t.kind == TokenKind::Ident)
            .map(|t| (t.text(self.src).to_string(), t.span))
            .collect();
        for (lex, span) in hits {
            if lex.eq_ignore_ascii_case("use_dll") && self.model.use_dll_span.is_none() {
                self.model.use_dll_span = Some(span);
            } else if lex.eq_ignore_ascii_case("no_static") && self.model.no_static_span.is_none() {
                self.model.no_static_span = Some(span);
            } else if lex.eq_ignore_ascii_case("block") && self.model.model_block_option.is_none() {
                self.model.model_block_option = Some(span);
            }
        }
    }

    fn option_ident_in_range(&self, from: usize, to: usize, name: &str) -> bool {
        self.tokens[from..to.min(self.tokens.len())]
            .iter()
            .any(|t| t.kind == TokenKind::Ident && t.text(self.src).eq_ignore_ascii_case(name))
    }

    fn record_skipped_block_opener(&mut self) {
        let tok = self.tokens[self.i].clone();
        let lex = self.lexeme(&tok).to_string();
        if lex.eq_ignore_ascii_case("shock_paths") && self.model.shock_paths_span.is_none() {
            self.model.shock_paths_span = Some(tok.span);
        } else if lex.eq_ignore_ascii_case("perfect_foresight_controlled_paths")
            && self.model.perfect_foresight_controlled_paths_span.is_none()
        {
            self.model.perfect_foresight_controlled_paths_span = Some(tok.span);
        }
    }

    fn at(&self, kind: TokenKind) -> bool {
        self.tokens.get(self.i).is_some_and(|t| t.kind == kind)
    }

    /// Blocks the grammar hands to `… END ';'` and this parser does not read. Every
    /// pin `DYNARE_BLOCK` opener that no other branch parses belongs here, or its
    /// body rows fall through to the top-level statement walk: a `priors;` row is a
    /// bare name, and a declared head before `;` reads as a statement a top-level
    /// recogniser would then refuse on a file 7.1 accepts.
    fn at_skipped_block(&self) -> bool {
        const BLOCKS: &[&str] = &[
            "pac_target_info",
            "priors",
            "deterministic_trends",
            "estimated_params_remove",
            "verbatim",
        ];
        BLOCKS.iter().any(|kw| self.at_ident_ci(kw))
    }

    fn at_ident(&self, name: &str) -> bool {
        self.at(TokenKind::Ident) && self.lexeme(&self.tokens[self.i]).eq_ignore_ascii_case(name)
    }

    fn at_ident_ci(&self, name: &str) -> bool {
        self.at_ident(name)
    }

    fn peek_kind(&self, ahead: usize) -> Option<TokenKind> {
        self.tokens.get(self.i + ahead).map(|t| t.kind)
    }

    fn current_start(&self) -> u32 {
        self.tokens
            .get(self.i)
            .map(|t| t.span.start)
            .unwrap_or(self.src.len() as u32)
    }

    fn bump(&mut self) -> Token {
        let tok = self.tokens[self.i].clone();
        if tok.kind != TokenKind::Eof {
            self.i += 1;
        }
        tok
    }

    fn eat(&mut self, kind: TokenKind) {
        if self.at(kind) {
            self.bump();
        }
    }

    fn lexeme<'a>(&'a self, tok: &'a Token) -> &'a str {
        tok.text(self.src)
    }

    fn match_complementarity(&self, id: ExprId) -> Option<ComplementarityTriple> {
        let ExprKind::Binary { op, lhs, rhs } = &self.model.exprs.get(id).kind else {
            return None;
        };
        let op = *op;
        let lhs = *lhs;
        let rhs = *rhs;
        if !is_cmp(op) {
            return None;
        }
        if let ExprKind::Binary {
            op: inner_op,
            lhs: inner_l,
            rhs: inner_r,
        } = &self.model.exprs.get(lhs).kind
        {
            let inner_op = *inner_op;
            let inner_l = *inner_l;
            let inner_r = *inner_r;
            if is_cmp(inner_op) {
                if !same_cmp_dir(inner_op, op) {
                    return None;
                }
                if self.is_endo_now(inner_r)
                    && self.is_constant_bound(inner_l)
                    && self.is_constant_bound(rhs)
                {
                    let variable = self.endo_name(inner_r);
                    let (lower, upper) = if is_less(op) {
                        (self.bound_text(inner_l), self.bound_text(rhs))
                    } else {
                        (self.bound_text(rhs), self.bound_text(inner_l))
                    };
                    return Some(ComplementarityTriple {
                        variable,
                        lower_bound: Some(lower),
                        upper_bound: Some(upper),
                    });
                }
                return None;
            }
        }
        if self.is_endo_now(lhs) && self.is_constant_bound(rhs) {
            let bound = self.bound_text(rhs);
            let (lower_bound, upper_bound) = if is_greater(op) {
                (Some(bound), None)
            } else {
                (None, Some(bound))
            };
            return Some(ComplementarityTriple {
                variable: self.endo_name(lhs),
                lower_bound,
                upper_bound,
            });
        }
        if self.is_constant_bound(lhs) && self.is_endo_now(rhs) {
            let bound = self.bound_text(lhs);
            let (lower_bound, upper_bound) = if is_greater(op) {
                (None, Some(bound))
            } else {
                (Some(bound), None)
            };
            return Some(ComplementarityTriple {
                variable: self.endo_name(rhs),
                lower_bound,
                upper_bound,
            });
        }
        None
    }

    fn is_endo_now(&self, id: ExprId) -> bool {
        match &self.model.exprs.get(id).kind {
            ExprKind::Ident { name, timing, .. } if *timing == 0 => {
                self.model.endogenous.iter().any(|d| d.name == *name)
            }
            _ => false,
        }
    }

    fn is_constant_bound(&self, id: ExprId) -> bool {
        !self.model.exprs.walk_idents(id).any(|r| {
            self.model.endogenous.iter().any(|d| d.name == r.name)
                || self.model.exogenous.iter().any(|d| d.name == r.name)
                || self
                    .model
                    .deterministic_exogenous
                    .iter()
                    .any(|d| d.name == r.name)
        })
    }

    fn bound_text(&self, id: ExprId) -> String {
        let span = self.model.exprs.get(id).span;
        let start = span.start as usize;
        let end = (span.end as usize).min(self.src.len());
        if start >= end || start > self.src.len() {
            return String::new();
        }
        collapse_ws(&self.src[start..end])
    }

    fn endo_name(&self, id: ExprId) -> String {
        match &self.model.exprs.get(id).kind {
            ExprKind::Ident { name, .. } => self.intern.get(*name).to_string(),
            _ => String::new(),
        }
    }
}

fn skip_balanced_tokens(
    tokens: &[Token],
    mut i: usize,
    open: TokenKind,
    close: TokenKind,
) -> usize {
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

fn ident_in(tok: &Token, src: &str, names: &[&str]) -> bool {
    tok.kind == TokenKind::Ident && names.iter().any(|n| tok.text(src).eq_ignore_ascii_case(n))
}

fn next_ident(tokens: &[Token], src: &str, mut i: usize, end_i: usize) -> Option<(String, usize)> {
    while i < end_i && tokens[i].kind == TokenKind::Comma {
        i += 1;
    }
    if i >= end_i || tokens[i].kind != TokenKind::Ident {
        return None;
    }
    Some((tokens[i].text(src).to_string(), i + 1))
}

fn opener_at(tokens: &[Token], src: &str, i: usize) -> Option<(String, Span, usize)> {
    let tok = tokens.get(i)?;
    if tok.kind != TokenKind::Ident {
        return None;
    }
    let name = tok.text(src);
    let key = BLOCK_OPENERS
        .iter()
        .find(|k| name.eq_ignore_ascii_case(k))?
        .to_string();
    let mut j = i + 1;
    if tokens.get(j).is_some_and(|t| t.kind == TokenKind::LParen) {
        j = skip_balanced_tokens(tokens, j, TokenKind::LParen, TokenKind::RParen);
    }
    let semi = tokens.get(j)?;
    if semi.kind != TokenKind::Semi {
        return None;
    }
    Some((
        key,
        Span {
            start: tok.span.start,
            end: semi.span.end,
        },
        j,
    ))
}

fn is_end_semi(tokens: &[Token], src: &str, i: usize) -> bool {
    tokens[i].kind == TokenKind::Ident
        && tokens[i].text(src).eq_ignore_ascii_case("end")
        && tokens.get(i + 1).is_some_and(|t| t.kind == TokenKind::Semi)
}

fn complete_block_ranges(tokens: &[Token], src: &str) -> Vec<Span> {
    let mut ranges = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        if let Some((_, span, semi_i)) = opener_at(tokens, src, i) {
            let mut j = semi_i + 1;
            while j < tokens.len() {
                if is_end_semi(tokens, src, j) {
                    ranges.push(Span {
                        start: span.start,
                        end: tokens[j + 1].span.end,
                    });
                    break;
                }
                if opener_at(tokens, src, j).is_some() {
                    break;
                }
                j += 1;
            }
            i = semi_i + 1;
            continue;
        }
        i += 1;
    }
    ranges
}

fn inside_span(offset: u32, ranges: &[Span]) -> bool {
    ranges.iter().any(|r| offset >= r.start && offset < r.end)
}

fn at_line_start_ident(src: &str, span: Span) -> bool {
    let start = span.start as usize;
    let line_start = src[..start].rfind('\n').map(|i| i + 1).unwrap_or(0);
    src[line_start..start]
        .chars()
        .all(|c| c == ' ' || c == '\t')
}

fn trailing_code_line(tokens: &[Token], src: &str) -> Option<u32> {
    for tok in tokens {
        if ident_in(tok, src, TERMINAL_COMMANDS) {
            let line_start = src[..tok.span.start as usize]
                .bytes()
                .filter(|&b| b == b'\n')
                .count() as u32;
            return Some(line_start);
        }
    }
    None
}

fn leading_ident(s: &str) -> Option<&str> {
    let s = s.trim_start();
    let mut end = 0;
    for (i, c) in s.char_indices() {
        if i == 0 {
            if !(c.is_ascii_alphabetic() || c == '_') {
                return None;
            }
            end = i + c.len_utf8();
        } else if c.is_ascii_alphanumeric() || c == '_' {
            end = i + c.len_utf8();
        } else {
            break;
        }
    }
    if end == 0 {
        None
    } else {
        Some(&s[..end])
    }
}

fn strip_line_comment(line: &str) -> &str {
    if let Some(i) = line.find("//") {
        return line[..i].trim_end();
    }
    if let Some(i) = line.find('%') {
        return line[..i].trim_end();
    }
    line.trim_end()
}

fn looks_like_matlab(rhs: &str) -> bool {
    rhs.contains('\'')
        || rhs.contains('"')
        || rhs.contains('[')
        || rhs.contains(']')
        || rhs.contains('{')
        || rhs.contains('}')
        || rhs
            .split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
            .any(|w| matches!(w, "M_" | "oo_" | "options_"))
}

fn line_of(src: &str, byte: u32) -> (u32, &str) {
    let byte = byte.min(src.len() as u32) as usize;
    let start = src[..byte].rfind('\n').map(|i| i + 1).unwrap_or(0);
    let end = src[byte..]
        .find('\n')
        .map(|i| byte + i)
        .unwrap_or(src.len());
    let line_no = src[..start].bytes().filter(|&b| b == b'\n').count() as u32;
    (line_no, &src[start..end])
}

pub(crate) fn join_lexemes(src: &str, tokens: &[Token]) -> String {
    let mut out = String::new();
    let mut prev: Option<TokenKind> = None;
    for tok in tokens {
        if tok.kind == TokenKind::Eof {
            continue;
        }
        let piece = tok.text(src);
        if piece.is_empty() {
            continue;
        }
        if let Some(p) = prev {
            if needs_space(p, tok.kind) {
                out.push(' ');
            }
        }
        out.push_str(piece);
        prev = Some(tok.kind);
    }
    out
}

fn tight_right(kind: TokenKind) -> bool {
    matches!(
        kind,
        TokenKind::LParen
            | TokenKind::LBrack
            | TokenKind::Plus
            | TokenKind::Minus
            | TokenKind::Star
            | TokenKind::Slash
            | TokenKind::Caret
            | TokenKind::Hash
    )
}

fn tight_left(kind: TokenKind) -> bool {
    matches!(
        kind,
        TokenKind::LParen
            | TokenKind::RParen
            | TokenKind::LBrack
            | TokenKind::RBrack
            | TokenKind::Comma
            | TokenKind::Plus
            | TokenKind::Minus
            | TokenKind::Star
            | TokenKind::Slash
            | TokenKind::Caret
    )
}

fn needs_space(prev: TokenKind, next: TokenKind) -> bool {
    !tight_right(prev) && !tight_left(next)
}

fn equation_from_statement(raw: &str, span: Span) -> Option<Equation> {
    let (name, rest) = strip_leading_tags(raw);
    let text = collapse_ws(&rest);
    if text.is_empty() {
        return None;
    }
    if !text.chars().any(|c| c.is_ascii_alphabetic()) && !looks_like_numeric_equation(&text) {
        return None;
    }
    let (lhs, rhs) = match split_eq(&text) {
        Some((l, r)) => (l.to_string(), r.to_string()),
        None => (String::new(), String::new()),
    };
    Some(Equation {
        text,
        name,
        span,
        lhs,
        rhs,
        lhs_expr: None,
        rhs_expr: None,
        is_local: false,
        model_local: false,
        static_tag: false,
        dynamic_tag: false,
        tags: Vec::new(),
        tag_map: BTreeMap::new(),
        tag_twice: Vec::new(),
        complementarity: None,
    })
}

fn unquote_string(s: &str) -> String {
    let s = s.trim();
    let bytes = s.as_bytes();
    if bytes.len() >= 2
        && ((bytes[0] == b'\'' && *bytes.last().unwrap() == b'\'')
            || (bytes[0] == b'"' && *bytes.last().unwrap() == b'"'))
    {
        s[1..s.len() - 1].to_string()
    } else {
        s.to_string()
    }
}

fn is_cmp(op: BinOp) -> bool {
    matches!(op, BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge)
}

fn is_less(op: BinOp) -> bool {
    matches!(op, BinOp::Lt | BinOp::Le)
}

fn is_greater(op: BinOp) -> bool {
    matches!(op, BinOp::Gt | BinOp::Ge)
}

fn same_cmp_dir(a: BinOp, b: BinOp) -> bool {
    (is_less(a) && is_less(b)) || (is_greater(a) && is_greater(b))
}

fn strip_leading_tags(raw: &str) -> (String, String) {
    let mut s = raw.trim_start();
    let mut name = String::new();
    while s.starts_with('[') {
        let Some(end) = tag_end(s) else {
            break;
        };
        let tag = &s[..end];
        if name.is_empty() {
            if let Some(n) = tag_name_attr(tag) {
                name = n;
            }
        }
        s = s[end..].trim_start();
    }
    (name, s.to_string())
}

fn tag_end(s: &str) -> Option<usize> {
    let mut quote: Option<char> = None;
    for (i, c) in s.char_indices() {
        if let Some(q) = quote {
            if c == q {
                quote = None;
            }
            continue;
        }
        if c == '\'' || c == '"' {
            quote = Some(c);
        } else if c == ']' {
            return Some(i + 1);
        }
    }
    None
}

fn tag_name_attr(tag: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    let key = lower.find("name")?;
    let after = &tag[key + 4..];
    let after = after.trim_start();
    let after = after.strip_prefix('=')?.trim_start();
    let quote = after.chars().next()?;
    if quote != '\'' && quote != '"' {
        return None;
    }
    let rest = &after[quote.len_utf8()..];
    let end = rest.find(quote)?;
    Some(rest[..end].to_string())
}

fn split_eq(text: &str) -> Option<(&str, &str)> {
    // First `=` also matches `==`/`!=`/`<=`/`>=`; lhs/rhs strings are P-core
    // display slices, not comparison-aware. Do not use this for expr split.
    let idx = text.find('=')?;
    Some((text[..idx].trim(), text[idx + 1..].trim()))
}

fn collapse_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn is_ident(s: &str) -> bool {
    let mut chars = s.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    first.is_ascii_alphabetic() && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn looks_like_numeric_equation(text: &str) -> bool {
    let Some((lhs, rhs)) = split_eq(text) else {
        return false;
    };
    fn numericish(s: &str) -> bool {
        s.chars().all(|c| {
            c.is_ascii_digit() || matches!(c, '.' | '+' | '-' | '*' | '/' | '(' | ')' | ' ')
        })
    }
    numericish(lhs) && numericish(rhs)
}

fn is_integer_lexeme(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

/// What a `period_list` token is, in 7.1's token names.
#[derive(Clone, Copy, PartialEq, Eq)]
enum PeriodKind {
    Int,
    Float,
    Date,
    Minus,
    Other,
}

/// Values and optional weights parsed from one matched IRF row.
struct IrfValueWeights {
    values: Vec<Span>,
    value_exprs: Vec<ExprId>,
    weights: Vec<Span>,
    weight_exprs: Vec<ExprId>,
}

/// One integer or one date in a period list, before a `:`.
struct PeriodAtom {
    is_date: bool,
    span: Span,
    /// `2000Q1` is two of our tokens and one of theirs.
    tokens: usize,
}

/// A `<DYNARE_BLOCK>` keyword, so bison names the token rather than IDENTIFIER.
fn block_keyword_token(lex: &str) -> Option<&'static str> {
    const WORDS: &[(&str, &str)] = &[
        ("var", "VAR"),
        ("varexo", "VAREXO"),
        ("values", "VALUES"),
        ("weights", "WEIGHTS"),
        ("periods", "PERIODS"),
        ("end", "END"),
        ("overwrite", "OVERWRITE"),
        ("relative_irf", "RELATIVE_IRF"),
    ];
    WORDS
        .iter()
        .find(|(name, _)| lex.eq_ignore_ascii_case(name))
        .map(|(_, token)| *token)
}

fn is_builtin_function(name: &str) -> bool {
    const BUILTINS: &[&str] = &[
        "exp",
        "log",
        "ln",
        "log10",
        "sqrt",
        "cbrt",
        "sign",
        "abs",
        "sin",
        "cos",
        "tan",
        "asin",
        "acos",
        "atan",
        "sinh",
        "cosh",
        "tanh",
        "asinh",
        "acosh",
        "atanh",
        "max",
        "min",
        "normcdf",
        "normpdf",
        "erf",
        "erfc",
        "log2",
        "floor",
        "ceil",
        "round",
        "norminv",
        "logncdf",
        "pac_expectation",
        "var_expectation",
        "pac_target_nonstationary",
        "diff",
        "adl",
    ];
    BUILTINS.iter().any(|b| name.eq_ignore_ascii_case(b))
}

/// An assignment-block row whose first word is `end` (`end = 0;`). `endogenous`
/// is a different word.
fn statement_is_end_word(raw: &str) -> bool {
    let t = raw.trim();
    let Some(rest) = t.get(3..) else {
        return false;
    };
    t[..3].eq_ignore_ascii_case("end")
        && !rest
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse;

    fn parse_lhs(src: &str) -> (crate::model::Model, ExprId) {
        let model = parse(&format!("model;\n{src};\nend;\n"));
        assert_eq!(
            model.equations.len(),
            1,
            "expected one equation for {src:?}"
        );
        let id = model.equations[0].lhs_expr.expect("lhs_expr");
        (model, id)
    }

    fn kind(model: &crate::model::Model, id: ExprId) -> &ExprKind {
        &model.exprs.get(id).kind
    }

    fn name(model: &crate::model::Model, n: Name) -> &str {
        model.name(n)
    }

    #[test]
    fn timing_c_plus_one() {
        let (m, id) = parse_lhs("c(+1)");
        match kind(&m, id) {
            ExprKind::Ident {
                name: n, timing, ..
            } => {
                assert_eq!(name(&m, *n), "c");
                assert_eq!(*timing, 1);
            }
            other => panic!("expected Ident, got {other:?}"),
        }
    }

    #[test]
    fn timing_c_one_is_lead() {
        let (m, id) = parse_lhs("c(1)");
        match kind(&m, id) {
            ExprKind::Ident {
                name: n, timing, ..
            } => {
                assert_eq!(name(&m, *n), "c");
                assert_eq!(*timing, 1);
            }
            other => panic!("expected Ident, got {other:?}"),
        }
    }

    #[test]
    fn timing_k_minus_one() {
        let (m, id) = parse_lhs("k(-1)");
        match kind(&m, id) {
            ExprKind::Ident {
                name: n, timing, ..
            } => {
                assert_eq!(name(&m, *n), "k");
                assert_eq!(*timing, -1);
            }
            other => panic!("expected Ident, got {other:?}"),
        }
    }

    #[test]
    fn call_log_y() {
        let (m, id) = parse_lhs("log(y)");
        match kind(&m, id) {
            ExprKind::Call { callee, args } => {
                assert_eq!(name(&m, *callee), "log");
                assert_eq!(args.len(), 1);
                match kind(&m, args[0]) {
                    ExprKind::Ident {
                        name: n, timing, ..
                    } => {
                        assert_eq!(name(&m, *n), "y");
                        assert_eq!(*timing, 0);
                    }
                    other => panic!("expected Ident y, got {other:?}"),
                }
            }
            other => panic!("expected Call, got {other:?}"),
        }
    }

    #[test]
    fn log_one_is_call_not_timed_log() {
        let (m, id) = parse_lhs("log(1)");
        match kind(&m, id) {
            ExprKind::Call { callee, args } => {
                assert_eq!(name(&m, *callee), "log");
                assert_eq!(args.len(), 1);
                assert!(matches!(kind(&m, args[0]), ExprKind::Number));
            }
            other => panic!("expected Call, got {other:?}"),
        }
    }

    #[test]
    fn mul_pow_timing_exp_z_k_lag() {
        let (m, id) = parse_lhs("exp(z)*k(-1)^alppha");
        match kind(&m, id) {
            ExprKind::Binary {
                op: BinOp::Mul,
                lhs,
                rhs,
            } => {
                match kind(&m, *lhs) {
                    ExprKind::Call { callee, args } => {
                        assert_eq!(name(&m, *callee), "exp");
                        assert_eq!(args.len(), 1);
                        match kind(&m, args[0]) {
                            ExprKind::Ident {
                                name: n, timing, ..
                            } => {
                                assert_eq!(name(&m, *n), "z");
                                assert_eq!(*timing, 0);
                            }
                            other => panic!("expected Ident z, got {other:?}"),
                        }
                    }
                    other => panic!("expected Call exp, got {other:?}"),
                }
                match kind(&m, *rhs) {
                    ExprKind::Binary {
                        op: BinOp::Pow,
                        lhs: k,
                        rhs: a,
                    } => {
                        match kind(&m, *k) {
                            ExprKind::Ident {
                                name: n, timing, ..
                            } => {
                                assert_eq!(name(&m, *n), "k");
                                assert_eq!(*timing, -1);
                            }
                            other => panic!("expected Ident k, got {other:?}"),
                        }
                        match kind(&m, *a) {
                            ExprKind::Ident {
                                name: n, timing, ..
                            } => {
                                assert_eq!(name(&m, *n), "alppha");
                                assert_eq!(*timing, 0);
                            }
                            other => panic!("expected Ident alppha, got {other:?}"),
                        }
                    }
                    other => panic!("expected Pow, got {other:?}"),
                }
            }
            other => panic!("expected Mul, got {other:?}"),
        }
    }

    #[test]
    fn steady_state_and_expectation() {
        let (m, id) = parse_lhs("STEADY_STATE(y(+1))");
        match kind(&m, id) {
            ExprKind::SteadyState { arg } => match kind(&m, *arg) {
                ExprKind::Ident {
                    name: n, timing, ..
                } => {
                    assert_eq!(name(&m, *n), "y");
                    assert_eq!(*timing, 1);
                }
                other => panic!("expected Ident y(+1), got {other:?}"),
            },
            other => panic!("expected SteadyState, got {other:?}"),
        }

        let (m, id) = parse_lhs("EXPECTATION(-1)(x(+1))");
        match kind(&m, id) {
            ExprKind::Expectation { shift, arg } => {
                assert_eq!(*shift, -1);
                match kind(&m, *arg) {
                    ExprKind::Ident {
                        name: n, timing, ..
                    } => {
                        assert_eq!(name(&m, *n), "x");
                        assert_eq!(*timing, 1);
                    }
                    other => panic!("expected Ident x(+1), got {other:?}"),
                }
            }
            other => panic!("expected Expectation, got {other:?}"),
        }
    }

    #[test]
    fn comments_and_strings_are_not_ident_nodes() {
        let (m, id) = parse_lhs("a + /* sneaky_ident */ b");
        match kind(&m, id) {
            ExprKind::Binary {
                op: BinOp::Add,
                lhs,
                rhs,
            } => {
                assert!(matches!(kind(&m, *lhs), ExprKind::Ident { .. }));
                assert!(matches!(kind(&m, *rhs), ExprKind::Ident { .. }));
            }
            other => panic!("expected Add, got {other:?}"),
        }
        let refs: Vec<_> = m
            .exprs
            .walk_idents(id)
            .map(|r| m.name(r.name).to_string())
            .collect();
        assert_eq!(refs, vec!["a", "b"]);

        let (m, id) = parse_lhs("a + 'sneaky_ident'");
        match kind(&m, id) {
            ExprKind::Binary {
                op: BinOp::Add,
                lhs,
                rhs,
            } => {
                assert!(matches!(kind(&m, *lhs), ExprKind::Ident { .. }));
                assert!(matches!(kind(&m, *rhs), ExprKind::String));
            }
            other => panic!("expected Add, got {other:?}"),
        }
        let refs: Vec<_> = m
            .exprs
            .walk_idents(id)
            .map(|r| m.name(r.name).to_string())
            .collect();
        assert_eq!(refs, vec!["a"]);
    }

    #[test]
    fn endval_block_fills_entries() {
        let model = parse("endval; x = 1; end;");
        assert_eq!(model.endval.len(), 1);
        assert!(model.endval_block.is_some());
        assert_eq!(model.name(model.endval[0].name), "x");
        assert_eq!(model.endval[0].expression, "1");
    }

    #[test]
    fn static_dynamic_tags_set_and_text_drops_brackets() {
        let model = parse("model; [static] y = 1; [dynamic] y = 1; end;");
        assert_eq!(model.equations.len(), 2);
        assert_eq!(model.equations[0].text, "y = 1");
        assert_eq!(model.equations[0].tags, vec!["static".to_string()]);
        assert!(model.equations[0].static_tag);
        assert!(!model.equations[0].dynamic_tag);
        assert_eq!(model.equations[1].text, "y = 1");
        assert_eq!(model.equations[1].tags, vec!["dynamic".to_string()]);
        assert!(model.equations[1].dynamic_tag);
        assert!(!model.equations[1].static_tag);
    }

    #[test]
    fn model_options_use_dll_bytecode_no_static_linear() {
        let model =
            parse("var y; varexo e; model(use_dll, bytecode, no_static, linear); y = e; end;");
        assert!(model.is_linear);
        assert!(model.use_dll_span.is_some());
        assert!(model.no_static_span.is_some());
        assert!(model
            .deprecated_option_spans
            .iter()
            .any(|(o, _)| *o == DeprecatedOption::Bytecode));
    }

    #[test]
    fn extended_path_periods_flag() {
        let without = parse("var y; varexo e; model; y = e; end; extended_path;");
        assert!(without.extended_path_span.is_some());
        assert!(!without.extended_path_has_periods);
        let with = parse("var y; varexo e; model; y = e; end; extended_path(periods=10);");
        assert!(with.extended_path_span.is_some());
        assert!(with.extended_path_has_periods);
    }

    #[test]
    fn initval_all_values_required_and_after_endval() {
        let req =
            parse("var y; varexo e; model; y = e; end; initval(all_values_required); y = 0; end;");
        assert!(req.initval_all_values_required);
        assert!(!req.endval_all_values_required);
        assert!(req.initval_after_endval_span.is_none());
        let order =
            parse("var y; varexo e; model; y = e; end; endval; y = 0; end; initval; y = 0; end;");
        assert!(order.endval_block.is_some());
        assert!(order.initval_block.is_some());
        assert!(order.initval_after_endval_span.is_some());
    }

    #[test]
    fn write_latex_and_run_command_spans() {
        let model = parse(
            "var y; varexo e; model; y = e; end; write_latex_steady_state_model; check; steady; stoch_simul; estimation; calib_smoother; perfect_foresight_setup; perfect_foresight_solver; perfect_foresight_with_expectation_errors_setup; perfect_foresight_with_expectation_errors_solver;",
        );
        assert!(model.write_latex_steady_state_model_span.is_some());
        assert!(model.check_span.is_some());
        assert!(model.steady_span.is_some());
        assert!(model.stoch_simul_span.is_some());
        assert!(model.estimation_span.is_some());
        assert!(model.calib_smoother_span.is_some());
        assert!(model.perfect_foresight_setup_span.is_some());
        assert!(model.perfect_foresight_solver_span.is_some());
        assert!(model.pfee_setup_span.is_some());
        assert!(model.pfee_solver_span.is_some());
    }

    #[test]
    fn ramsey_constraints_body_is_parsed() {
        let model = parse(
            "var y; varexo e; model; y = e; end; ramsey_constraints; y > 0; end; stoch_simul;",
        );
        assert!(model.ramsey_constraints_span.is_some());
        assert_eq!(model.ramsey_constraints.len(), 1);
        assert!(model.ramsey_constraints[0].expr.is_some());
        assert!(model.stoch_simul_span.is_some());
    }

    #[test]
    fn shock_stderr_keeps_expr() {
        let model = parse(
            "var y; varexo e; parameters rho; rho = 0.5; model; y = e; end; shocks; var e; stderr rho; end;",
        );
        assert_eq!(model.shock_stmts.len(), 1);
        assert!(model.shock_stmts[0].rhs_expr.is_some());
        assert!(model.shock_stmts[0].rhs.is_none());
        match &model.shock_stmts[0].kind {
            ShockKind::Stderr(n) => assert_eq!(model.name(*n), "e"),
            other => panic!("expected Stderr, got {other:?}"),
        }
    }

    #[test]
    fn discretionary_instruments_option_present_vs_absent() {
        let src = "var y; varexo e; model; y = e; end; planner_objective y; ";
        let absent = parse(&format!("{src}discretionary_policy;"));
        assert!(!absent.discretionary_has_instruments_option);
        let present = parse(&format!("{src}discretionary_policy(instruments=(y));"));
        assert!(present.discretionary_has_instruments_option);
        let empty = parse(&format!("{src}discretionary_policy(instruments=());"));
        assert!(empty.discretionary_has_instruments_option);
        assert!(empty.instruments.is_empty());
    }
}
