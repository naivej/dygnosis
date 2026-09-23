//! The shape gate the MS-SBVAR family shares (0.5.4 04 F-check).
//!
//! 7.1's grammar gives each command's option list, each block body row and each
//! dotted head a fixed set of productions. A statement written outside them is a
//! parse-stage `syntax error, unexpected …` that names no construct, so the report
//! is our own wording under **E001**.
//!
//! The tables are read off the pin's own grammar (`DynareBison.yy` /
//! `DynareFlex.ll`, `9c61fb6e`), so an option is accepted exactly when the
//! statement's production carries it and its value is written in the shape that
//! production produces. They are **not** the product catalog (`catalog_data.rs`),
//! which lists the same names for a different purpose and is a version behind the
//! pin in places.
//!
//! Two shapes are deliberately outside this file. A `restriction` body: the
//! grammar spells it, and 7.1's check pass then crashes on an expression with no
//! `coeff(…)` term, so it must stay silent. And every command outside the family:
//! `stoch_simul(nonsense=1);` is 7.1's `syntax error, unexpected IDENTIFIER`, and
//! the catalog records it with no owner (close call 2 keeps it out).

use crate::model::{FamilyOption, FamilyValueKind, ShapeRefuse};

/// The value shapes the pin's option productions produce.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    /// A bare flag.
    Flag,
    /// A signed or unsigned number.
    Number,
    /// An unsigned number only.
    Uint,
    /// `vec_int_number` or `vec_int`: a bare unsigned integer **or** a bracketed
    /// list of them. Both alternatives are real productions, so `equations=1` is
    /// as legal as `equations=[1 2]`.
    UintOrVector,
    /// An unsigned number that may be fractional (`non_negative_number`).
    Positive,
    /// A bare name (`symbol`): never a number and never a quoted string.
    Symbol,
    /// `filename`: a name **or** a quoted string, but never a number.
    Filename,
    /// A `DATE`, optionally `+ INT_NUMBER`.
    Date,
    /// `[a, b]` of names.
    NameList,
    /// `[1 2 3]`.
    Vector,
    /// `[[1, 2, 0.5], …]`.
    Matrix,
    /// `(a, b)` of names.
    ParenNames,
    /// `A1:B10`.
    Range,
    /// A full `expression`, which almost anything can be.
    AnyValue,
    /// One word from a fixed list.
    Word(&'static [&'static str]),
    /// `upper_cholesky` / `lower_cholesky` / a name — never a quoted string.
    RestrictionFile,
}

/// The words the pin's token rules accept in a value position.
pub mod words {
    /// `prior_distribution`.
    pub const PRIOR_SHAPES: &[&str] = &[
        "beta",
        "gamma",
        "normal",
        "inv_gamma",
        "inv_gamma1",
        "inv_gamma2",
        "uniform",
        "dirichlet",
        "weibull",
    ];
    /// `o_parameter_set`.
    pub const PARAMETER_SETS: &[&str] = &[
        "prior_mode",
        "prior_mean",
        "posterior_mean",
        "posterior_mode",
        "posterior_median",
        "mle_mode",
        "calibration",
    ];
    /// `o_freq`.
    pub const FREQUENCIES: &[&str] = &["monthly", "quarterly"];
    /// `o_specification`.
    pub const SPECIFICATIONS: &[&str] = &["sims_zha", "none"];
}

const DATA_OPTIONS: &[(&str, &[Shape])] = &[
    ("file", &[Shape::Filename]),
    ("series", &[Shape::Symbol]),
    ("first_obs", &[Shape::Date]),
    ("last_obs", &[Shape::Date]),
    ("nobs", &[Shape::Uint]),
    ("xls_sheet", &[Shape::Symbol, Shape::Filename]),
    ("xls_range", &[Shape::Range]),
];

const MARKOV_SWITCHING_OPTIONS: &[(&str, &[Shape])] = &[
    ("chain", &[Shape::Uint]),
    ("duration", &[Shape::Positive, Shape::Vector]),
    ("restrictions", &[Shape::Matrix]),
    ("number_of_regimes", &[Shape::Uint]),
    ("number_of_lags", &[Shape::Uint]),
    ("parameters", &[Shape::NameList]),
];

const SVAR_OPTIONS: &[(&str, &[Shape])] = &[
    ("coefficients", &[Shape::Flag]),
    ("variances", &[Shape::Flag]),
    ("equations", &[Shape::UintOrVector]),
    ("chain", &[Shape::Uint]),
];

const CONDITIONAL_FORECAST_OPTIONS: &[(&str, &[Shape])] = &[
    ("periods", &[Shape::Uint]),
    ("replic", &[Shape::Uint]),
    ("conf_sig", &[Shape::Positive]),
    ("controlled_varexo", &[Shape::ParenNames]),
    ("parameter_set", &[Shape::Word(words::PARAMETER_SETS)]),
];

const MS_ESTIMATION_OPTIONS: &[(&str, &[Shape])] = &[
    ("coefficients_prior_hyperparameters", &[Shape::Vector]),
    ("freq", &[Shape::Uint, Shape::Word(words::FREQUENCIES)]),
    ("initial_year", &[Shape::Uint]),
    ("initial_subperiod", &[Shape::Uint]),
    ("final_year", &[Shape::Uint]),
    ("final_subperiod", &[Shape::Uint]),
    ("datafile", &[Shape::Filename]),
    ("xls_sheet", &[Shape::Symbol, Shape::Filename]),
    ("xls_range", &[Shape::Range]),
    ("nlags", &[Shape::Uint]),
    ("cross_restrictions", &[Shape::Flag]),
    ("contemp_reduced_form", &[Shape::Flag]),
    ("no_bayesian_prior", &[Shape::Flag]),
    ("alpha", &[Shape::Positive]),
    ("beta", &[Shape::Positive]),
    ("gsig2_lmdm", &[Shape::Uint]),
    ("specification", &[Shape::Word(words::SPECIFICATIONS)]),
    ("output_file_tag", &[Shape::Filename]),
    ("file_tag", &[Shape::Filename]),
    ("no_create_init", &[Shape::Flag]),
    ("convergence_starting_value", &[Shape::Positive]),
    ("convergence_ending_value", &[Shape::Positive]),
    ("convergence_increment_value", &[Shape::Positive]),
    ("max_iterations_starting_value", &[Shape::Uint]),
    ("max_iterations_increment_value", &[Shape::Positive]),
    ("max_block_iterations", &[Shape::Uint]),
    ("max_repeated_optimization_runs", &[Shape::Uint]),
    ("function_convergence_criterion", &[Shape::Positive]),
    ("parameter_convergence_criterion", &[Shape::Positive]),
    ("number_of_large_perturbations", &[Shape::Uint]),
    ("number_of_small_perturbations", &[Shape::Uint]),
    (
        "number_of_posterior_draws_after_perturbation",
        &[Shape::Uint],
    ),
    ("max_number_of_stages", &[Shape::Uint]),
    ("random_function_convergence_criterion", &[Shape::Positive]),
    ("random_parameter_convergence_criterion", &[Shape::Positive]),
];

const MS_SIMULATION_OPTIONS: &[(&str, &[Shape])] = &[
    ("output_file_tag", &[Shape::Filename]),
    ("file_tag", &[Shape::Filename]),
    ("mh_replic", &[Shape::Uint]),
    ("drop", &[Shape::Uint]),
    ("thinning_factor", &[Shape::Uint]),
    ("adaptive_mh_draws", &[Shape::Uint]),
    ("save_draws", &[Shape::Flag]),
];

const MS_COMPUTE_MDD_OPTIONS: &[(&str, &[Shape])] = &[
    ("output_file_tag", &[Shape::Filename]),
    ("file_tag", &[Shape::Filename]),
    ("simulation_file_tag", &[Shape::Filename]),
    ("proposal_type", &[Shape::Uint]),
    ("proposal_lower_bound", &[Shape::Number]),
    ("proposal_upper_bound", &[Shape::Number]),
    ("proposal_draws", &[Shape::Uint]),
    ("use_mean_center", &[Shape::Flag]),
];

const MS_COMPUTE_PROBABILITIES_OPTIONS: &[(&str, &[Shape])] = &[
    ("output_file_tag", &[Shape::Filename]),
    ("file_tag", &[Shape::Filename]),
    ("filtered_probabilities", &[Shape::Flag]),
    ("real_time_smoothed", &[Shape::Flag]),
];

const MS_IRF_OPTIONS: &[(&str, &[Shape])] = &[
    ("output_file_tag", &[Shape::Filename]),
    ("file_tag", &[Shape::Filename]),
    ("simulation_file_tag", &[Shape::Filename]),
    ("parameter_uncertainty", &[Shape::Flag]),
    ("horizon", &[Shape::Uint]),
    ("filtered_probabilities", &[Shape::Flag]),
    ("error_band_percentiles", &[Shape::Vector]),
    ("shock_draws", &[Shape::Uint]),
    ("shocks_per_parameter", &[Shape::Uint]),
    ("thinning_factor", &[Shape::Uint]),
    ("free_parameters", &[Shape::Vector]),
    ("median", &[Shape::Flag, Shape::Number]),
    ("regime", &[Shape::Uint]),
    ("regimes", &[Shape::Flag]),
];

const MS_FORECAST_OPTIONS: &[(&str, &[Shape])] = &[
    ("output_file_tag", &[Shape::Filename]),
    ("file_tag", &[Shape::Filename]),
    ("simulation_file_tag", &[Shape::Filename]),
    ("data_obs_nbr", &[Shape::Uint]),
    ("error_band_percentiles", &[Shape::Vector]),
    ("shock_draws", &[Shape::Uint]),
    ("shocks_per_parameter", &[Shape::Uint]),
    ("thinning_factor", &[Shape::Uint]),
    ("free_parameters", &[Shape::Vector]),
    ("median", &[Shape::Flag, Shape::Number]),
    ("regime", &[Shape::Uint]),
    ("regimes", &[Shape::Flag]),
    ("parameter_uncertainty", &[Shape::Flag]),
    ("horizon", &[Shape::Uint]),
];

const MS_VARIANCE_DECOMPOSITION_OPTIONS: &[(&str, &[Shape])] = &[
    ("output_file_tag", &[Shape::Filename]),
    ("file_tag", &[Shape::Filename]),
    ("simulation_file_tag", &[Shape::Filename]),
    ("filtered_probabilities", &[Shape::Flag]),
    ("no_error_bands", &[Shape::Flag]),
    ("error_band_percentiles", &[Shape::Vector]),
    ("shock_draws", &[Shape::Uint]),
    ("shocks_per_parameter", &[Shape::Uint]),
    ("thinning_factor", &[Shape::Uint]),
    ("free_parameters", &[Shape::Vector]),
    ("regime", &[Shape::Uint]),
    ("regimes", &[Shape::Flag]),
    ("parameter_uncertainty", &[Shape::Flag]),
    ("horizon", &[Shape::Uint]),
];

const SBVAR_OPTIONS: &[(&str, &[Shape])] = &[
    ("datafile", &[Shape::Filename]),
    ("freq", &[Shape::Uint, Shape::Word(words::FREQUENCIES)]),
    ("initial_year", &[Shape::Uint]),
    ("initial_subperiod", &[Shape::Uint]),
    ("final_year", &[Shape::Uint]),
    ("final_subperiod", &[Shape::Uint]),
    ("data", &[Shape::Filename]),
    ("vlist", &[Shape::Uint]),
    ("vlistlog", &[Shape::ParenNames]),
    ("vlistper", &[Shape::Uint]),
    ("restriction_fname", &[Shape::RestrictionFile]),
    ("nlags", &[Shape::Uint]),
    ("cross_restrictions", &[Shape::Flag]),
    ("contemp_reduced_form", &[Shape::Flag]),
    ("real_pseudo_forecast", &[Shape::Uint]),
    ("no_bayesian_prior", &[Shape::Flag]),
    ("dummy_obs", &[Shape::Uint]),
    ("nstates", &[Shape::Uint]),
    ("indxscalesstates", &[Shape::Uint]),
    ("alpha", &[Shape::Positive]),
    ("beta", &[Shape::Positive]),
    ("gsig2_lmdm", &[Shape::Uint]),
    ("q_diag", &[Shape::Positive]),
    ("flat_prior", &[Shape::Uint]),
    ("ncsk", &[Shape::Uint]),
    ("nstd", &[Shape::Uint]),
    ("ninv", &[Shape::Uint]),
    ("indxparr", &[Shape::Uint]),
    ("indxovr", &[Shape::Uint]),
    ("aband", &[Shape::Uint]),
    ("indxap", &[Shape::Uint]),
    ("apband", &[Shape::Uint]),
    ("indximf", &[Shape::Uint]),
    ("indxfore", &[Shape::Uint]),
    ("foreband", &[Shape::Uint]),
    ("indxgforehat", &[Shape::Uint]),
    ("indxgimfhat", &[Shape::Uint]),
    ("indxestima", &[Shape::Uint]),
    ("indxgdls", &[Shape::Uint]),
    ("eq_ms", &[Shape::Uint]),
    ("cms", &[Shape::Uint]),
    ("ncms", &[Shape::Uint]),
    ("eq_cms", &[Shape::Uint]),
    ("tlindx", &[Shape::Uint]),
    ("tlnumber", &[Shape::Uint]),
    ("cnum", &[Shape::Uint]),
    ("forecast", &[Shape::Uint]),
    ("coefficients_prior_hyperparameters", &[Shape::Vector]),
];

const SMOOTHER2HISTVAL_OPTIONS: &[(&str, &[Shape])] = &[
    ("invars", &[Shape::ParenNames]),
    ("period", &[Shape::Uint]),
    ("outfile", &[Shape::Filename]),
    ("outvars", &[Shape::ParenNames]),
];

const PRIOR_OPTIONS: &[(&str, &[Shape])] = &[
    ("shift", &[Shape::Number]),
    ("mean", &[Shape::Number]),
    ("median", &[Shape::Flag, Shape::Number]),
    ("stdev", &[Shape::Positive]),
    ("truncate", &[Shape::Vector]),
    ("variance", &[Shape::AnyValue]),
    ("mode", &[Shape::Number]),
    ("interval", &[Shape::Vector]),
    ("shape", &[Shape::Word(words::PRIOR_SHAPES)]),
    ("domain", &[Shape::Vector]),
];

const JOINT_PRIOR_OPTIONS: &[(&str, &[Shape])] = &[
    ("shift", &[Shape::Number]),
    ("mean", &[Shape::Vector]),
    ("median", &[Shape::Flag, Shape::Number]),
    ("stdev", &[Shape::Positive]),
    ("truncate", &[Shape::Vector]),
    ("variance", &[Shape::Matrix]),
    ("mode", &[Shape::Number]),
    ("interval", &[Shape::Vector]),
    ("shape", &[Shape::Word(words::PRIOR_SHAPES)]),
    ("domain", &[Shape::Vector]),
];

const DOTTED_OPTIONS: &[(&str, &[Shape])] = &[
    ("jscale", &[Shape::Positive]),
    ("init", &[Shape::Number]),
    ("bounds", &[Shape::Vector]),
];

const FORECAST_OPTIONS: &[(&str, &[Shape])] = &[
    ("periods", &[Shape::Uint]),
    ("conf_sig", &[Shape::Positive]),
    ("nograph", &[Shape::Flag]),
    ("graph", &[Shape::Flag]),
    ("nodisplay", &[Shape::Flag]),
    ("graph_format", &[Shape::ParenNames]),
];

/// Value shapes shared by the handed-over decomposition commands. Command
/// membership still comes from the catalog, since their option sets differ.
const DECOMPOSITION_VALUE_SHAPES: &[(&str, &[Shape])] = &[
    ("colormap", &[Shape::Symbol]),
    ("parameter_set", &[Shape::Word(words::PARAMETER_SETS)]),
    ("periods", &[Shape::Uint]),
    ("first_obs", &[Shape::Uint]),
    ("nobs", &[Shape::Uint]),
    ("init_state", &[Shape::Uint]),
    ("fig_name", &[Shape::Filename]),
    ("plot_init_date", &[Shape::Date]),
    ("plot_end_date", &[Shape::Date]),
    ("type", &[Shape::Word(&["qoq", "yoy", "aoa"])]),
    ("nograph", &[Shape::Flag]),
    ("nodisplay", &[Shape::Flag]),
    ("detail_plot", &[Shape::Flag]),
    ("with_epilogue", &[Shape::Flag]),
    ("steadystate", &[Shape::Flag]),
    ("write_xls", &[Shape::Flag]),
    ("interactive", &[Shape::Flag]),
    ("screen_shocks", &[Shape::Flag]),
    ("diff", &[Shape::Flag]),
    ("flip", &[Shape::Flag]),
];

/// What the grammar takes for a named command's option list, or `None` when the
/// command is not one whose list this gate knows.
pub fn command_options(command: &str) -> Option<&'static [(&'static str, &'static [Shape])]> {
    let table: &'static [(&'static str, &'static [Shape])] = if command.eq_ignore_ascii_case("data")
    {
        DATA_OPTIONS
    } else if command.eq_ignore_ascii_case("markov_switching") {
        MARKOV_SWITCHING_OPTIONS
    } else if command.eq_ignore_ascii_case("svar") {
        SVAR_OPTIONS
    } else if command.eq_ignore_ascii_case("conditional_forecast") {
        CONDITIONAL_FORECAST_OPTIONS
    } else if command.eq_ignore_ascii_case("ms_estimation") {
        MS_ESTIMATION_OPTIONS
    } else if command.eq_ignore_ascii_case("ms_simulation") {
        MS_SIMULATION_OPTIONS
    } else if command.eq_ignore_ascii_case("ms_compute_mdd") {
        MS_COMPUTE_MDD_OPTIONS
    } else if command.eq_ignore_ascii_case("ms_compute_probabilities") {
        MS_COMPUTE_PROBABILITIES_OPTIONS
    } else if command.eq_ignore_ascii_case("ms_irf") {
        MS_IRF_OPTIONS
    } else if command.eq_ignore_ascii_case("ms_forecast") {
        MS_FORECAST_OPTIONS
    } else if command.eq_ignore_ascii_case("ms_variance_decomposition") {
        MS_VARIANCE_DECOMPOSITION_OPTIONS
    } else if command.eq_ignore_ascii_case("sbvar") {
        SBVAR_OPTIONS
    } else if command.eq_ignore_ascii_case("smoother2histval") {
        SMOOTHER2HISTVAL_OPTIONS
    } else {
        return None;
    };
    Some(table)
}

/// The prior option tables, split by head form.
pub fn prior_options(joint: bool) -> &'static [(&'static str, &'static [Shape])] {
    if joint {
        JOINT_PRIOR_OPTIONS
    } else {
        PRIOR_OPTIONS
    }
}

pub fn dotted_options() -> &'static [(&'static str, &'static [Shape])] {
    DOTTED_OPTIONS
}

/// The syntax checks handed over with the nine trailing-list commands. The
/// catalog decides membership for decomposition option sets; known values not
/// covered by a shape row remain with their established parser/check owner.
pub fn handed_option_refusal(
    src: &str,
    command: &str,
    options: &[FamilyOption],
) -> Option<ShapeRefuse> {
    if command.eq_ignore_ascii_case("forecast") {
        return option_refusal(src, command, options, FORECAST_OPTIONS);
    }
    let known = crate::catalog::command_options(command);
    for opt in options {
        if !known
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case(&opt.name))
        {
            let message = if opt.name.eq_ignore_ascii_case("with_epilogue") {
                "syntax error, unexpected WITH_EPILOGUE"
            } else {
                "syntax error, unexpected IDENTIFIER"
            };
            return Some(ShapeRefuse::official(opt.span, command, message));
        }
        if let Some((_, shapes)) = DECOMPOSITION_VALUE_SHAPES
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(&opt.name))
        {
            if !row_fits(src, shapes, opt) {
                if shapes.contains(&Shape::Date)
                    && opt.value_text.bytes().all(|b| b.is_ascii_digit())
                    && !opt.value_text.is_empty()
                {
                    return Some(ShapeRefuse::official(
                        opt.value_span,
                        command,
                        "syntax error, unexpected INT_NUMBER, expecting DATE",
                    ));
                }
                return Some(ShapeRefuse::new(
                    opt.span,
                    command,
                    "a value written in the shape that option has",
                ));
            }
        }
    }
    None
}

/// Whether one option row matches one production of its table entry.
fn row_fits(src: &str, shapes: &[Shape], opt: &FamilyOption) -> bool {
    shapes.iter().any(|shape| shape_fits(src, *shape, opt))
}

fn shape_fits(src: &str, shape: Shape, opt: &FamilyOption) -> bool {
    match shape {
        Shape::Flag => !opt.has_value,
        Shape::Number => opt.has_value && signed(&opt.value_text),
        Shape::Uint => opt.has_value && unsigned(&opt.value_text),
        // `vec_int_number` is the bare form; `vec_int` the bracketed one. `[]` has
        // no production, a sign is not in the rule, and `name` is not a value the
        // production takes.
        Shape::UintOrVector => {
            opt.has_value
                && if opt.value_kind == FamilyValueKind::Vector {
                    let e = entries(&opt.value_text);
                    !e.is_empty() && e.iter().all(|x| unsigned(x))
                } else {
                    opt.value_kind == FamilyValueKind::Scalar && unsigned(&opt.value_text)
                }
        }
        Shape::Positive => {
            opt.has_value
                && opt.value_kind == FamilyValueKind::Scalar
                && !opt.value_text.trim_start().starts_with('-')
                && signed(&opt.value_text)
        }
        // `symbol` in the grammar's own `symbol` production, which takes an
        // identifier or one of its reserved word tokens — never a number (that is
        // `INT_NUMBER`) and never a quoted string (that is `QUOTED_STRING`).
        Shape::Symbol => opt.has_value && is_name(src, opt),
        // `filename` is `symbol | QUOTED_STRING`: a name or a quoted string, but a
        // bare number is neither.
        Shape::Filename => {
            opt.has_value
                && matches!(
                    opt.value_kind,
                    FamilyValueKind::Scalar | FamilyValueKind::Range
                )
                && (is_name(src, opt) || is_quoted(src, opt))
        }
        Shape::Date => opt.has_value && opt.value_kind == FamilyValueKind::Date,
        // `'[' symbol_list ']'`: the grammar spells the list whatever the names
        // are. Whether they are *declared* is the check pass's business (and an
        // undeclared one in `parameters=[…]` crashes their test — see the module
        // comment), so this only asks that the brackets hold something.
        Shape::NameList => {
            opt.has_value
                && opt.value_kind == FamilyValueKind::NameList
                && !entries(&opt.value_text).is_empty()
        }
        // `vec_value_1` also needs at least one entry.
        Shape::Vector => {
            opt.has_value
                && matches!(
                    opt.value_kind,
                    FamilyValueKind::Vector | FamilyValueKind::NameList
                )
                && !entries(&opt.value_text).is_empty()
        }
        // `vec_of_vec_value` is a bracketed list of bracketed rows.
        Shape::Matrix => {
            opt.has_value
                && opt.value_kind == FamilyValueKind::Matrix
                && !entries(&opt.value_text).is_empty()
        }
        Shape::ParenNames => {
            // `'(' symbol_list ')'` reads names, and the parser's own
            // `FamilyValueKind` records a `(`-list of names as a `NameList` exactly
            // as a `[`-list does, so the bracket character in the source is what
            // tells them apart. The list needs at least one *name*: `(1)` has no
            // production, and `()` none either.
            opt.has_value
                && value_starts_with(src, opt, '(')
                && !entries(&opt.value_text).is_empty()
                && entries(&opt.value_text).iter().all(|e| is_name_text(e))
        }
        Shape::Range => opt.has_value && opt.value_kind == FamilyValueKind::Range,
        Shape::AnyValue => opt.has_value && !opt.value_text.is_empty(),
        Shape::Word(words) => {
            opt.has_value && words.iter().any(|w| opt.value_text.eq_ignore_ascii_case(w))
        }
        // `o_restriction_fname` is `IDENTIFIER | UPPER_CHOLESKY | LOWER_CHOLESKY`:
        // a bare name only. A quoted string and a number are both syntax errors.
        Shape::RestrictionFile => opt.has_value && is_name(src, opt),
    }
}

/// Whether the option's value is written as a bare name in the source: its own
/// text is an identifier, and the source's first character there says so. The
/// parser's `FamilyValueKind::Scalar` covers a name, a number and a quoted string
/// alike, so the source is what tells them apart.
fn is_name(src: &str, opt: &FamilyOption) -> bool {
    opt.value_kind == FamilyValueKind::Scalar
        && !is_quoted(src, opt)
        && !opt.value_text.is_empty()
        && opt
            .value_text
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
}

/// Whether the option's value is written as a quoted string in the source.
fn is_quoted(src: &str, opt: &FamilyOption) -> bool {
    src.get(opt.value_span.start as usize..opt.value_span.end as usize)
        .is_some_and(|text| text.trim_start().starts_with(['\'', '"']))
}

/// Whether one entry of a bracketed list is a bare name: an identifier or one of
/// the grammar's word tokens, never a number.
fn is_name_text(entry: &str) -> bool {
    entry
        .trim()
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
}

/// The entries of a bracketed value's text, split on commas and whitespace.
/// `[]` and a bare scalar give none.
fn entries(text: &str) -> Vec<String> {
    let trimmed = text.trim();
    let Some(inner) = trimmed
        .strip_prefix(['[', '('])
        .and_then(|t| t.strip_suffix([']', ')']))
    else {
        return vec![trimmed.to_string()];
    };
    inner
        .split([',', ' ', '\t'])
        .map(str::trim)
        .filter(|piece| !piece.is_empty())
        .map(str::to_string)
        .collect()
}

/// Whether the option's value starts with `ch` in the source. The parser's value
/// text collapses the brackets' contents, so the source is the only place the
/// bracket character survives.
fn value_starts_with(src: &str, opt: &FamilyOption, ch: char) -> bool {
    src.get(opt.value_span.start as usize..opt.value_span.end as usize)
        .is_some_and(|text| text.trim_start().starts_with(ch))
}

fn unsigned(text: &str) -> bool {
    let trimmed = text.trim();
    !trimmed.is_empty() && trimmed.bytes().all(|b| b.is_ascii_digit())
}

fn signed(text: &str) -> bool {
    let trimmed = text.trim();
    let rest = trimmed.strip_prefix(['-', '+']).unwrap_or(trimmed);
    if rest.is_empty() {
        return false;
    }
    let mut dot = 0;
    for b in rest.bytes() {
        if b == b'.' {
            dot += 1;
        } else if !b.is_ascii_digit() {
            return false;
        }
    }
    dot <= 1
}

/// Whether one statement's option list is written the way the named command's
/// productions spell it: non-empty, every name carried, every value in the shape
/// its production gives it.
///
/// This is the **one** home for that question. `check_d_ms`'s per-command guards
/// call it rather than keeping a second copy of the tables, so a table edit cannot
/// fix the sweep and miss the guards.
pub fn options_spellable(
    src: &str,
    command: &str,
    options: &[FamilyOption],
    table: &'static [(&'static str, &'static [Shape])],
) -> bool {
    !options.is_empty() && option_refusal(src, command, options, table).is_none()
}

/// The same for one of the commands whose table this module holds.
pub fn command_options_spellable(src: &str, command: &str, options: &[FamilyOption]) -> bool {
    match command_options(command) {
        Some(table) => options_spellable(src, command, options, table),
        None => true,
    }
}

/// The dotted `prior` body's own tables, keyed on the head form.
pub fn prior_options_spellable(src: &str, options: &[FamilyOption], joint: bool) -> bool {
    let subject = if joint { "[…].prior" } else { "prior" };
    options_spellable(src, subject, options, prior_options(joint))
}

/// The first option row of `options` the named command's grammar cannot spell,
/// as a refusal pointing at that option's own name span.
///
/// `src` is the model's own source: one shape test (the `(`-list) needs the
/// bracket character, which the collapsed value text has already dropped.
pub fn option_refusal(
    src: &str,
    command: &str,
    options: &[FamilyOption],
    table: &'static [(&'static str, &'static [Shape])],
) -> Option<ShapeRefuse> {
    for opt in options {
        let Some((_, shapes)) = table
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(&opt.name))
        else {
            return Some(ShapeRefuse::new(
                opt.span,
                command.to_string(),
                "one of the option names this statement carries",
            ));
        };
        if !row_fits(src, shapes, opt) {
            if shapes.contains(&Shape::Date)
                && opt.value_text.bytes().all(|b| b.is_ascii_digit())
                && !opt.value_text.is_empty()
            {
                return Some(ShapeRefuse::official(
                    opt.value_span,
                    command,
                    "syntax error, unexpected INT_NUMBER, expecting DATE",
                ));
            }
            return Some(ShapeRefuse::new(
                opt.span,
                command.to_string(),
                "a value written in the shape that option has",
            ));
        }
    }
    None
}
