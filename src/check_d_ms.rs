//! MS-SBVAR family refusals (0.5.4 02): one code per distinct official sentence.
//!
//! The MS-SBVAR messages were copied from Dynare 7.1 (`9c61fb6e`) and
//! checked against the 7.2 pin, including their misspellings (`transitions probabilities`, `probabilites`,
//! `subsample statement`), their missing full stops, and the bare backticks
//! around `parameter_set`. The records are the ones 01 P-parse built
//! (`Model::data_statements`, `ms_statements`, `dotted_statements`,
//! `svar_identifications`, `conditional_forecast_paths`).
//!
//! Three rules keep the codes honest.
//!
//! - **One statement, one Error.** The pin exits at the first refusal inside a
//!   statement's parse or check pass, so each section stops at its first fire, in
//!   the order their own code tests things.
//! - **A shape the grammar cannot spell is not a shape we refuse.** Where a
//!   spelling never reaches an official message (`data();`, `svar(constants)`,
//!   `svar(chain=1, equations=[])`, a negative or fractional value where the
//!   grammar takes a bare unsigned literal, an unknown option name), the section
//!   is silent rather than borrow a neighbour's digit. The catalog's **0.5.4 04
//!   F-check** owns those shapes.
//! - **Two shapes crash 7.1 with no message** (`parameters=[<undeclared>]` and a
//!   `restriction` with no `coeff(…)` term). They stay silent on purpose: there is
//!   no official text to copy.
//!
//! The sentences that already ship stay where they live: **E058** and **E317** in
//! [`check_d_open`](crate::check_d_open), **E059** in [`diag_shape`](crate::diag_shape).
//! This module publishes the head and row tests they read, and stands down
//! wherever their sentences fire, so no shape is reported twice.

use std::collections::HashSet;

use crate::diagnostic::{Diagnostic, Severity};
use crate::intern::Name;
use crate::model::{
    ConditionalForecastPath, DataStatement, DottedHead, DottedKind, DottedStatement, FamilyOption,
    FamilyValueKind, Model, MsStatement, ShapeRefuse, SvarEquation, SvarIdentification,
    SvarIdentificationElement,
};
use crate::span::Span;

/// The pinned preprocessor reads the whole file, then runs each statement's check pass in file
/// order. Both phases stop at their first refusal, so the walk does too.
///
/// A shape the grammar cannot spell is a **parse**-stage refuse: its parser
/// stops on it before the run reaches any check pass, whatever line the check
/// pass would have been on. So the shape sweep joins the parse phase and is
/// ordered against its units by file position.
pub fn check_d_ms(model: &Model) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    if check_parse_phase(model, &mut out) {
        return out;
    }
    check_check_phase(model, &mut out);
    out
}

/// The shape refusals the family's own records carry, in file order.
/// Statement-level rows live on `Model::shape_refuses`; each block keeps its own.
fn shape_refuses(model: &Model) -> Vec<&ShapeRefuse> {
    let mut rows: Vec<&ShapeRefuse> = model.shape_refuses.iter().collect();
    for block in &model.svar_identifications {
        rows.extend(block.shape_refuses.iter());
    }
    for block in &model.conditional_forecast_paths {
        rows.extend(block.shape_refuses.iter());
    }
    rows.sort_by_key(|refuse| (refuse.span.start, refuse.span.end));
    rows
}

/// Every parsed family statement's span, whatever record holds it.
fn statement_spans(model: &Model) -> Vec<Span> {
    model
        .ms_statements
        .iter()
        .map(|s| s.span)
        .chain(model.data_statements.iter().map(|s| s.span))
        .chain(model.dotted_statements.iter().map(|s| s.span))
        .chain(model.svar_identifications.iter().map(|b| b.span))
        .chain(model.conditional_forecast_paths.iter().map(|b| b.span))
        .collect()
}

/// The start of the innermost parsed statement that contains `refuse`, or the
/// refuse's own start when it sits outside every one.
fn containing_statement_start(spans: &[Span], refuse: &ShapeRefuse) -> u32 {
    spans
        .iter()
        .filter(|span| refuse.span.start >= span.start && refuse.span.start < span.end)
        .map(|span| span.start)
        .min()
        .unwrap_or(refuse.span.start)
}

/// The first shape refuse that belongs to the statement starting at `at`, in file
/// order.
///
/// The pinned parser reads a statement's tokens left to right and stops at the first
/// one its grammar cannot spell, so a shape refuse anywhere inside a statement
/// pre-empts every sentence that statement's own actions or check pass would
/// print — including the ones the grammar's actions print while parsing
/// (`The value passed to the chain option …`). Within the parse stage the refuse
/// keeps its own position, because there it competes with other parse actions in
/// file order.
fn shape_refuse_in_statement(model: &Model, at: u32, end: u32) -> Option<&ShapeRefuse> {
    shape_refuses(model)
        .into_iter()
        .find(|refuse| refuse.span.start >= at && refuse.span.start < end)
}

/// The check-phase unit's own end, for scoping a shape refuse to its statement.
fn unit_end(model: &Model, unit: &CheckUnit) -> u32 {
    match unit {
        CheckUnit::Data(stmt) => stmt.span.end,
        CheckUnit::MsEstimation(stmt)
        | CheckUnit::ConditionalForecast(stmt)
        | CheckUnit::Markov(stmt)
        | CheckUnit::MutuallyExclusive(stmt) => stmt.span.end,
        CheckUnit::Identification(block) => block.span.end,
        CheckUnit::Prior(stmt) => stmt.span.end,
    }
    .max(1)
    .min(model.source.len() as u32)
}

/// A known token-level syntax error keeps the official text. Otherwise a
/// generic bison refusal uses our short wording naming the command or option.
fn shape_refuse_message(refuse: &ShapeRefuse) -> String {
    refuse
        .official_message
        .map(str::to_string)
        .unwrap_or_else(|| crate::model::shape_refuse_wording(&refuse.subject, refuse.expected))
}

/// The refusals the pin prints while reading the file, in file order. Returns `true`
/// when one fired.
fn check_parse_phase(model: &Model, out: &mut Vec<Diagnostic>) -> bool {
    // Every parse-time unit of the family, ordered by where it starts. The shape
    // refusals are units of their own, so a refuse written before a sentence's
    // statement wins and one written after it loses — the pinned parser stops at the
    // first of the two in file order.
    enum Unit<'a> {
        Shape(&'a ShapeRefuse),
        Markov(&'a MsStatement),
        Svar(&'a MsStatement),
        Identification(&'a SvarIdentification),
        Paths(&'a crate::model::ConditionalForecastPaths),
        Prior(&'a DottedStatement),
        TopAssignment(&'a crate::model::Assignment),
    }
    let mut units: Vec<(u32, Unit)> = Vec::new();
    let spans = statement_spans(model);
    for refuse in shape_refuses(model) {
        let at = containing_statement_start(&spans, refuse);
        units.push((at, Unit::Shape(refuse)));
    }
    // A top-level `symbol = …;` reaches the grammar only when the head is declared,
    // and the pin refuses it while parsing when the symbol is not a parameter.
    for assignment in &model.helper_assignments {
        if declared_names(model).contains(&assignment.name) {
            units.push((assignment.span.start, Unit::TopAssignment(assignment)));
        }
    }
    for stmt in &model.ms_statements {
        if stmt.command.eq_ignore_ascii_case("markov_switching") {
            units.push((stmt.span.start, Unit::Markov(stmt)));
        } else if stmt.command.eq_ignore_ascii_case("svar") {
            units.push((stmt.span.start, Unit::Svar(stmt)));
        }
    }
    for block in &model.svar_identifications {
        units.push((block.span.start, Unit::Identification(block)));
    }
    for block in &model.conditional_forecast_paths {
        units.push((block.span.start, Unit::Paths(block)));
    }
    for stmt in &model.dotted_statements {
        units.push((stmt.span.start, Unit::Prior(stmt)));
    }
    units.sort_by_key(|(at, _)| *at);
    for (_, unit) in units {
        let fired = match unit {
            Unit::Shape(refuse) => {
                push(out, refuse.span, "E001", shape_refuse_message(refuse));
                true
            }
            Unit::Markov(stmt) => check_markov_switching_parse(model, stmt, out),
            Unit::Svar(stmt) => check_svar(&model.source, stmt, out),
            Unit::Identification(block) => {
                if let Some((span, code, message)) = identification_body_refusal(model, block) {
                    push(out, span, code, message);
                    true
                } else {
                    false
                }
            }
            Unit::Paths(block) => check_conditional_forecast_paths(model, block, out),
            Unit::Prior(stmt) => check_prior_head(model, stmt, out),
            Unit::TopAssignment(assignment) => check_top_assignment(model, assignment, out),
        };
        if fired {
            return true;
        }
    }
    false
}

/// The check-pass units, so a shape refuse can be scoped to the statement it
/// belongs to.
enum CheckUnit<'a> {
    Data(&'a DataStatement),
    MsEstimation(&'a MsStatement),
    ConditionalForecast(&'a MsStatement),
    Markov(&'a MsStatement),
    Identification(&'a SvarIdentification),
    MutuallyExclusive(&'a MsStatement),
    Prior(&'a DottedStatement),
}

/// The refusals the pin prints in the check pass, in file order.
fn check_check_phase(model: &Model, out: &mut Vec<Diagnostic>) {
    let mut units: Vec<(u32, CheckUnit)> = Vec::new();
    for stmt in &model.data_statements {
        units.push((stmt.span.start, CheckUnit::Data(stmt)));
    }
    for stmt in &model.ms_statements {
        let unit = if stmt.command.eq_ignore_ascii_case("ms_estimation") {
            CheckUnit::MsEstimation(stmt)
        } else if stmt.command.eq_ignore_ascii_case("conditional_forecast") {
            CheckUnit::ConditionalForecast(stmt)
        } else if stmt.command.eq_ignore_ascii_case("markov_switching") {
            CheckUnit::Markov(stmt)
        } else {
            CheckUnit::MutuallyExclusive(stmt)
        };
        units.push((stmt.span.start, unit));
    }
    for block in &model.svar_identifications {
        units.push((block.span.start, CheckUnit::Identification(block)));
    }
    for stmt in &model.dotted_statements {
        units.push((stmt.span.start, CheckUnit::Prior(stmt)));
    }
    units.sort_by_key(|(at, _)| *at);
    let mut chains: i32 = 0;
    let mut second_identification = false;
    for (at, unit) in units {
        // The pinned parser stops inside the statement before its check pass runs, so a
        // shape refuse written anywhere in this statement beats every sentence the
        // statement would print here.
        let end = unit_end(model, &unit);
        if let Some(refuse) = shape_refuse_in_statement(model, at, end) {
            push(out, refuse.span, "E001", shape_refuse_message(refuse));
            return;
        }
        match unit {
            CheckUnit::Data(stmt) => {
                if check_data(&model.source, stmt, out) {
                    return;
                }
            }
            CheckUnit::MsEstimation(stmt) => {
                if check_ms_estimation(stmt, out) {
                    return;
                }
            }
            CheckUnit::ConditionalForecast(stmt) => {
                if check_conditional_forecast(stmt, out) {
                    return;
                }
            }
            // Their chain counter lives on `ModFileStructure`, so it runs across
            // the whole file, not per statement.
            CheckUnit::Markov(stmt) => {
                if check_markov_switching_chain_and_sums(stmt, &mut chains, out) {
                    return;
                }
            }
            CheckUnit::Identification(block) => {
                if check_svar_identification_block(block, second_identification, out) {
                    return;
                }
                second_identification = true;
            }
            CheckUnit::MutuallyExclusive(stmt) => {
                if check_ms_mutual_exclusion(stmt, out) {
                    return;
                }
            }
            CheckUnit::Prior(stmt) => {
                if check_prior_body(model, stmt, out) {
                    return;
                }
            }
        }
    }
}

fn push(out: &mut Vec<Diagnostic>, span: Span, code: &str, message: impl Into<String>) {
    out.push(Diagnostic::new(span, Severity::Error, code, message));
}

fn option<'a>(stmt: &'a MsStatement, name: &str) -> Option<&'a FamilyOption> {
    stmt.options
        .iter()
        .find(|opt| opt.name.eq_ignore_ascii_case(name))
}

fn has(stmt: &MsStatement, name: &str) -> bool {
    option(stmt, name).is_some()
}

fn option_of<'a>(options: &'a [FamilyOption], name: &str) -> Option<&'a FamilyOption> {
    options
        .iter()
        .find(|opt| opt.name.eq_ignore_ascii_case(name))
}

fn name_set<'a>(decls: impl Iterator<Item = &'a crate::model::Decl>) -> HashSet<Name> {
    decls.map(|d| d.name).collect()
}

/// Every declared symbol, for the "is not declared" tests.
pub(crate) fn declared_names(model: &Model) -> HashSet<Name> {
    name_set(
        model
            .endogenous
            .iter()
            .chain(&model.exogenous)
            .chain(&model.deterministic_exogenous)
            .chain(&model.parameters)
            .chain(&model.predetermined),
    )
}

pub(crate) fn endogenous_names(model: &Model) -> HashSet<Name> {
    name_set(model.endogenous.iter())
}

pub(crate) fn parameter_names(model: &Model) -> HashSet<Name> {
    name_set(model.parameters.iter())
}

/// A value the grammar spells as a bare unsigned integer, or `None` when 7.1
/// cannot spell it at all — a signed or fractional number is a syntax error, and
/// a syntax error carries no message to copy.
fn unsigned_int(text: &str) -> Option<i64> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

fn plain_int(text: &str) -> Option<i64> {
    let trimmed = text.trim();
    if trimmed.is_empty() || !trimmed.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    trimmed.parse().ok()
}

fn any_float(text: &str) -> Option<f64> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    trimmed.parse().ok()
}

// ---------------------------------------------------------------------------
// `data`
// ---------------------------------------------------------------------------

/// The estimation / MS-SBVAR `data` statement. Their `checkPass` tests `nobs`
/// before the file / series pair. Returns `true` when it refused.
fn check_data(src: &str, stmt: &DataStatement, out: &mut Vec<Diagnostic>) -> bool {
    if !data_options_spellable(src, stmt) {
        return false;
    }
    let nobs = option_of(&stmt.options, "nobs");
    if nobs
        .and_then(|opt| unsigned_int(&opt.value_text))
        .is_some_and(|n| n <= 0)
    {
        let span = nobs.map(|opt| opt.span).unwrap_or(stmt.span);
        push(
            out,
            span,
            "E340",
            "The nobs option of the data statement only accepts positive integers.",
        );
        return true;
    }
    let file = option_of(&stmt.options, "file");
    let series = option_of(&stmt.options, "series");
    if file.is_none() && series.is_none() {
        push(
            out,
            stmt.span,
            "E338",
            "The file or series option must be passed to the data statement.",
        );
        return true;
    }
    if file.is_some() && series.is_some() {
        push(
            out,
            stmt.span,
            "E339",
            "The file and series options cannot be used simultaneously in the data statement.",
        );
        return true;
    }
    false
}

/// Whether every option of this `data` statement is written in a shape the
/// grammar produces. The tables live in [`crate::shape_gate`], so this asks the
/// sweep's own question rather than keeping a second copy of them.
fn data_options_spellable(src: &str, stmt: &DataStatement) -> bool {
    crate::shape_gate::command_options_spellable(src, "data", &stmt.options)
}

// ---------------------------------------------------------------------------
// `ms_estimation`, `conditional_forecast`
// ---------------------------------------------------------------------------

/// The one `ms_estimation` sentence, whatever combination is missing. A bare
/// `ms_estimation;` reaches it too. Returns `true` when it refused.
fn check_ms_estimation(stmt: &MsStatement, out: &mut Vec<Diagnostic>) -> bool {
    if has(stmt, "no_create_init") || (has(stmt, "datafile") && has(stmt, "initial_year")) {
        return false;
    }
    push(
        out,
        stmt.span,
        "E341",
        "If you do not pass no_create_init to ms_estimation, you must pass the datafile and initial_year options.",
    );
    true
}

/// `parameter_set` is required. The `datafile` option arrives after 7.2 and is
/// bison junk at the pin, so nothing here reads it. Returns `true` when it
/// refused.
fn check_conditional_forecast(stmt: &MsStatement, out: &mut Vec<Diagnostic>) -> bool {
    if stmt.options.is_empty() || has(stmt, "parameter_set") {
        return false;
    }
    push(
        out,
        stmt.span,
        "E342",
        "You must pass the `parameter_set` option to conditional_forecast",
    );
    true
}

// ---------------------------------------------------------------------------
// `conditional_forecast_paths`
// ---------------------------------------------------------------------------

/// Their row order is the name's type first (their `check_symbol_is_endogenous`,
/// which **E058** / **E317** own), then declared-twice, then the periods /
/// values counts. A row whose `periods` / `values` list is empty is a parse-stage
/// syntax error, and 7.1's parser stops at it before any count is read — so the
/// block's own shape refuses are ordered against the rows here. Returns `true`
/// when it refused.
fn check_conditional_forecast_paths(
    model: &Model,
    block: &crate::model::ConditionalForecastPaths,
    out: &mut Vec<Diagnostic>,
) -> bool {
    // A row whose name their type check refuses stops the block before any count
    // is read, and so does a row the grammar cannot spell.
    if cfp_first_bad_row(model, &block.rows).is_some() {
        return false;
    }
    let endogenous = endogenous_names(model);
    let mut seen: HashSet<Name> = HashSet::new();
    for row in &block.rows {
        // A malformed row is the parser's own refuse; it comes first, because a
        // syntax error stops the run before the sentence on a later row.
        if let Some(refuse) = block
            .shape_refuses
            .iter()
            .find(|refuse| refuse.span.start >= row.span.start && refuse.span.start < row.span.end)
        {
            push(out, refuse.span, "E001", shape_refuse_message(refuse));
            return true;
        }
        if !endogenous.contains(&row.name) {
            continue;
        }
        if !seen.insert(row.name) {
            push(
                out,
                row.span,
                "E344",
                format!(
                    "shocks/conditional_forecast_paths: variable {} declared twice",
                    model.name(row.name)
                ),
            );
            return true;
        }
        if row.periods.len() != row.values.len() {
            push(
                out,
                row.span,
                "E343",
                format!(
                    "shocks/conditional_forecast_paths: variable {}: number of periods is different from number of shock values",
                    model.name(row.name)
                ),
            );
            return true;
        }
    }
    // A refuse that sits outside every row's span — the empty body, or an
    // `exogenize` / `endogenize` row the walker skipped — is reported after them.
    if let Some(refuse) = block.shape_refuses.first() {
        push(out, refuse.span, "E001", shape_refuse_message(refuse));
        return true;
    }
    false
}

/// The first `var` row of one block whose name their type check refuses, as
/// `(span, name, undeclared)`.
///
/// The callers are the shipped **E058** (a name the reader does not declare) and
/// **E317** (a declared name that is not endogenous).
pub(crate) fn cfp_first_bad_row(
    model: &Model,
    block: &[ConditionalForecastPath],
) -> Option<(Span, Name, bool)> {
    let declared = declared_names(model);
    let endogenous = endogenous_names(model);
    for row in block {
        if !declared.contains(&row.name) {
            return Some((row.span, row.name, true));
        }
        if !endogenous.contains(&row.name) {
            return Some((row.span, row.name, false));
        }
    }
    None
}

// ---------------------------------------------------------------------------
// `markov_switching`
// ---------------------------------------------------------------------------

/// The `markov_switching` refusals 7.1 prints while parsing: the `parameters`
/// option list, the three required options and the two positivity tests. The
/// statement's constructor — which runs the `restrictions` rules — is reached
/// only when none of those fired. Returns `true` when it refused.
fn check_markov_switching_parse(
    model: &Model,
    stmt: &MsStatement,
    out: &mut Vec<Diagnostic>,
) -> bool {
    if stmt.options.is_empty() {
        return false;
    }
    if check_ms_parameters(model, stmt, out) {
        return true;
    }
    for name in ["chain", "number_of_regimes", "duration"] {
        if !has(stmt, name) {
            push(
                out,
                stmt.span,
                "E345",
                format!("A '{name}' option must be passed to the 'markov_switching' statement."),
            );
            return true;
        }
    }
    // Only `0` reaches their positivity tests: a negative or fractional value is
    // a parse syntax error, and `duration` has no positivity test at all.
    let chain = option(stmt, "chain").and_then(|opt| unsigned_int(&opt.value_text));
    if chain == Some(0) {
        push(
            out,
            stmt.span,
            "E346",
            "The value passed to the chain option must be greater than zero.",
        );
        return true;
    }
    let regimes = option(stmt, "number_of_regimes").and_then(|opt| unsigned_int(&opt.value_text));
    if regimes == Some(0) {
        push(
            out,
            stmt.span,
            "E347",
            "The value passed to the number_of_regimes option must be greater than zero.",
        );
        return true;
    }
    let (Some(_chain), Some(regimes)) = (chain, regimes) else {
        return false;
    };

    let Some(restrictions) = option(stmt, "restrictions") else {
        return false;
    };
    if !restrictions_spellable(&model.source, restrictions) {
        return false;
    }
    let rows = matrix_rows(&restrictions.value_text).unwrap_or_default();
    let mut map: Vec<(i64, i64, f64)> = Vec::new();
    for row in &rows {
        if row.len() != 3 {
            push(
                out,
                restrictions.span,
                "E350",
                "restrictions in the subsample statement must be specified in the form [current_period_regime, next_period_regime, transition_probability]",
            );
            return true;
        }
        let (Some(from), Some(to)) = (plain_int(&row[0]), plain_int(&row[1])) else {
            return false;
        };
        let Some(probability) = any_float(&row[2]) else {
            return false;
        };
        if from > regimes || to > regimes {
            push(
                out,
                restrictions.span,
                "E351",
                "the regimes specified in the restrictions option must be <= the number of regimes specified in the number_of_regimes option",
            );
            return true;
        }
        if map.iter().any(|(f, t, _)| *f == from && *t == to) {
            push(
                out,
                restrictions.span,
                "E352",
                format!("two restrictions were given for: {from}, {to}"),
            );
            return true;
        }
        if probability > 1.0 {
            push(
                out,
                restrictions.span,
                "E353",
                format!("the transition probability, {probability} must be less than 1"),
            );
            return true;
        }
        map.push((from, to, probability));
    }
    false
}

/// The `markov_switching` check-pass refusals. Their chain counter lives on
/// `ModFileStructure`, so it runs across the whole file: each statement must
/// carry the next chain number, beginning at 1. Returns `true` when it refused.
fn check_markov_switching_chain_and_sums(
    stmt: &MsStatement,
    chains: &mut i32,
    out: &mut Vec<Diagnostic>,
) -> bool {
    let Some(chain) = option(stmt, "chain").and_then(|opt| unsigned_int(&opt.value_text)) else {
        return false;
    };
    *chains += 1;
    if *chains as i64 != chain {
        push(
            out,
            stmt.span,
            "E348",
            "The markov_switching chain option takes consecutive integers beginning at 1.",
        );
        return true;
    }
    let restrictions = option(stmt, "restrictions");
    let Some(regimes) = option(stmt, "number_of_regimes")
        .and_then(|opt| unsigned_int(&opt.value_text))
        .map(|n| n as usize)
    else {
        return false;
    };
    let Some(restrictions) = restrictions else {
        return false;
    };
    if restrictions.value_kind != FamilyValueKind::Matrix {
        return false;
    }
    let rows = matrix_rows(&restrictions.value_text).unwrap_or_default();
    let map: Vec<(i64, i64, f64)> = rows
        .iter()
        .filter_map(|row| {
            Some((
                plain_int(&row[0])?,
                plain_int(&row[1])?,
                any_float(&row[2])?,
            ))
        })
        .collect();
    if let Some((code, message)) = regime_sum_refusal(&map, regimes) {
        push(out, stmt.span, code, message);
        return true;
    }
    false
}

/// Whether the `restrictions` list is written in the shape their grammar gives
/// it: a bracketed list of bracketed number rows. Anything else is a syntax
/// error or a row their `stoi` / `stod` catch, which prints a sentence of its
/// own that no 0.5.4 row claims.
fn restrictions_spellable(src: &str, restrictions: &FamilyOption) -> bool {
    crate::shape_gate::options_spellable(
        src,
        "markov_switching",
        std::slice::from_ref(restrictions),
        crate::shape_gate::command_options("markov_switching").unwrap_or(&[]),
    )
}

/// The `parameters=[…]` option list. Returns `true` when it fired.
///
/// An undeclared name in the list crashes 7.1 with no `ERROR:` line at all, so
/// that shape stays silent on purpose. Their span is the whole option
/// (`parameters=[Y]`), not just the name inside it.
fn check_ms_parameters(model: &Model, stmt: &MsStatement, out: &mut Vec<Diagnostic>) -> bool {
    let Some(parameters) = option(stmt, "parameters") else {
        return false;
    };
    let declared = declared_names(model);
    let parameters_set = parameter_names(model);
    for (name, _span) in &parameters.names {
        if !declared.contains(name) {
            return false;
        }
        if !parameters_set.contains(name) {
            let span = Span {
                start: parameters.span.start,
                end: parameters.value_span.end.max(parameters.span.end),
            };
            push(
                out,
                span,
                "E349",
                format!(
                    "Variables passed to the parameters option of the markov_switching statement must be parameters. Caused by: {}",
                    model.name(*name)
                ),
            );
            return true;
        }
    }
    false
}

/// Their row / column sum refusal, whichever their loop reaches first.
fn regime_sum_refusal(
    map: &[(i64, i64, f64)],
    regimes: usize,
) -> Option<(&'static str, &'static str)> {
    for regime in 1..=regimes as i64 {
        let row: Vec<f64> = map
            .iter()
            .filter(|(from, _, _)| *from == regime)
            .map(|(_, _, p)| *p)
            .collect();
        if let Some(refusal) = sum_refusal(&row, regimes) {
            return Some(refusal);
        }
        let column: Vec<f64> = map
            .iter()
            .filter(|(_, to, _)| *to == regime)
            .map(|(_, _, p)| *p)
            .collect();
        if let Some(refusal) = sum_refusal(&column, regimes) {
            return Some(refusal);
        }
    }
    None
}

/// One literal covers both the row sum and the column sum; their misspelling on
/// the partial row is kept.
const SUM_ALL: &str =
    "When all transitions probabilities are specified for a certain regime, they must sum to 1";
const SUM_PARTIAL: &str =
    "When transition probabilites are not specified for every regime, their sum must be < 1";

fn sum_refusal(entries: &[f64], regimes: usize) -> Option<(&'static str, &'static str)> {
    if entries.len() == regimes {
        if entries.iter().sum::<f64>() != 1.0 {
            return Some(("E354", SUM_ALL));
        }
    } else if entries.iter().sum::<f64>() >= 1.0 {
        return Some(("E355", SUM_PARTIAL));
    }
    None
}

// ---------------------------------------------------------------------------
// `svar_identification`
// ---------------------------------------------------------------------------

/// The body's refusals, which 7.1 prints while parsing, then the two the block's
/// The two refusals the `svar_identification` block's own check pass prints.
/// The body's refusals run while parsing, so they are reached only when the body
/// itself is one 7.1 accepts. Returns `true` when it refused.
fn check_svar_identification_block(
    block: &SvarIdentification,
    previous_identification: bool,
    out: &mut Vec<Diagnostic>,
) -> bool {
    if let Some((span, code, message)) =
        identification_block_refusal(block, previous_identification)
    {
        push(out, span, code, message);
        return true;
    }
    false
}

/// The block's first refusal as `(span, code, message)`, or `None`.
fn identification_block_refusal(
    block: &SvarIdentification,
    previous_identification: bool,
) -> Option<(Span, &'static str, String)> {
    if previous_identification {
        return Some((
            block.span,
            "E356",
            "You may only have one svar_identification block in your .mod file.".to_string(),
        ));
    }
    let upper = block
        .elements
        .iter()
        .any(|element| matches!(element, SvarIdentificationElement::UpperCholesky { .. }));
    let lower = block
        .elements
        .iter()
        .any(|element| matches!(element, SvarIdentificationElement::LowerCholesky { .. }));
    if upper && lower {
        return Some((
            block.span,
            "E357",
            "Within the svar_identification statement, you may only have one of upper_cholesky and lower_cholesky.".to_string(),
        ));
    }
    None
}

/// The first body name one identification block names that the reader does not
/// declare, as `(span, name)`, for the shipped **E058**.
///
/// Their parse stops at that name, so no body sentence of this module may fire
/// on such a block.
pub(crate) fn identification_undeclared_name(
    model: &Model,
    block: &SvarIdentification,
) -> Option<(Span, String)> {
    let declared = declared_names(model);
    for (span, name) in identification_names(model, block) {
        let known = model
            .intern
            .lookup(&name)
            .is_some_and(|id| declared.contains(&id));
        if !known {
            return Some((span, name));
        }
    }
    None
}

/// Every name one identification block's `equation` rows and `restriction`
/// expressions carry, in source order.
fn identification_names(model: &Model, block: &SvarIdentification) -> Vec<(Span, String)> {
    let mut out = Vec::new();
    for element in &block.elements {
        match element {
            SvarIdentificationElement::ExclusionLag { equations, .. } => {
                for row in equations {
                    for (name, span) in &row.names {
                        out.push((*span, model.name(*name).to_string()));
                    }
                }
            }
            SvarIdentificationElement::Restriction { span, .. } => {
                for (name, _lag, term) in restriction_terms(model, *span) {
                    out.push((term, name));
                }
            }
            _ => {}
        }
    }
    out
}

/// The first body refusal 7.1 prints while reading the body, as `(span, code,
/// message)`. `None` when the body is one it accepts.
///
/// Their order: an `equation` row is read (its number, then its names), and the
/// lag is only combined — and its repeat test run — after that element's rows.
/// A name they do not declare has the shipped **E058**, so their run stops there
/// and this walk stops too.
///
/// A row the grammar has no production for is a syntax error, so it stops the
/// run before any sentence on that element: a leading `equation` row never
/// reaches `equation numbers must be greater than or equal to 1.`.
fn identification_body_refusal(
    model: &Model,
    block: &SvarIdentification,
) -> Option<(Span, &'static str, String)> {
    let declared = declared_names(model);
    let mut lags: Vec<i32> = Vec::new();
    for element in &block.elements {
        let element_span = match element {
            SvarIdentificationElement::ExclusionLag { span, .. } => *span,
            SvarIdentificationElement::Restriction { span, .. } => *span,
            SvarIdentificationElement::ExclusionConstants { span }
            | SvarIdentificationElement::UpperCholesky { span }
            | SvarIdentificationElement::LowerCholesky { span } => *span,
        };
        if let Some(refuse) = block.shape_refuses.iter().find(|refuse| {
            refuse.span.start >= element_span.start && refuse.span.start < element_span.end
        }) {
            return Some((refuse.span, "E001", shape_refuse_message(refuse)));
        }
        match element {
            SvarIdentificationElement::ExclusionLag { lag, equations, .. } => {
                if let Some(refusal) = equation_rows_refusal(model, equations, &declared) {
                    return Some(refusal);
                }
                // The grammar needs at least one `equation` row before the lag is
                // combined.
                if equations.is_empty() {
                    continue;
                }
                let Some(lag) = lag else { continue };
                if lags.contains(lag) {
                    return Some((
                        lag_span(equations),
                        "E358",
                        format!("lag {lag} used more than once."),
                    ));
                }
                lags.push(*lag);
            }
            SvarIdentificationElement::Restriction { span, .. } => {
                if let Some(refusal) = qi_ri_refusal(model, *span) {
                    return Some(refusal);
                }
            }
            _ => {}
        }
    }
    // A refuse that belongs to no element — an empty body — is reported after them.
    if let Some(refuse) = block.shape_refuses.first() {
        return Some((refuse.span, "E001", shape_refuse_message(refuse)));
    }
    None
}

/// The `equation N, name…;` rows under one lag. Their order is the equation
/// number, then the name list, and a name they do not declare ends the run.
///
/// Their `lag … used more than once.` range runs from the repeated lag's own row
/// through its last `equation` row, which the parsed element spans stop just
/// short of — and an element with no row at all never reaches that sentence.
fn equation_rows_refusal(
    model: &Model,
    equations: &[SvarEquation],
    declared: &HashSet<Name>,
) -> Option<(Span, &'static str, String)> {
    let mut numbers: Vec<i64> = Vec::new();
    for row in equations {
        if let Some(number) = row.number {
            let number = number as i64;
            if number < 1 {
                return Some((
                    row.span,
                    "E360",
                    "equation numbers must be greater than or equal to 1.".to_string(),
                ));
            }
            if numbers.contains(&number) {
                return Some((
                    equations_span(equations),
                    "E359",
                    format!(
                        "equation number {number} referenced more than once under a single lag."
                    ),
                ));
            }
            numbers.push(number);
        }
        // The body name is only checked to exist here; a wrong type is accepted.
        // An undeclared one is **E058**'s row and stops their run.
        if row.names.iter().any(|(name, _)| !declared.contains(name)) {
            return None;
        }
        let mut names: Vec<Name> = Vec::new();
        for (name, _span) in &row.names {
            if names.contains(name) {
                return Some((
                    row.span,
                    "E361",
                    format!("{} restriction added twice.", model.name(*name)),
                ));
            }
            names.push(*name);
        }
    }
    None
}

/// The range of an `exclusion lag` element's `equation` rows: its first row
/// through its last. `equations` is never empty where this is called, so no span
/// is ever placed at offset 0.
fn equations_span(equations: &[SvarEquation]) -> Span {
    match (equations.first(), equations.last()) {
        (Some(first), Some(last)) => Span {
            start: first.span.start,
            end: last.span.end,
        },
        // Unreachable: every caller checks `!equations.is_empty()` first.
        _ => Span { start: 1, end: 1 },
    }
}

/// The same range as [`equations_span`], through the last row's terminating `;`
/// — their `lag` message quotes the whole element.
fn lag_span(equations: &[SvarEquation]) -> Span {
    equations_span(equations)
}

/// A `restriction` whose `coeff` terms mix Qi (lag 0) with Ri (lag > 0). The
/// first term decides the kind, exactly as their driver does, and a term's own
/// name is checked before its lag is compared.
///
/// An expression with no `coeff(…)` term at all crashes 7.1 rather than
/// refusing, so it stays silent.
fn qi_ri_refusal(model: &Model, element_span: Span) -> Option<(Span, &'static str, String)> {
    let declared = declared_names(model);
    let terms = restriction_terms(model, element_span);
    let is_declared = |name: &str| {
        model
            .intern
            .lookup(name)
            .is_some_and(|id| declared.contains(&id))
    };
    let first = terms.first()?;
    if !is_declared(&first.0) {
        return None;
    }
    let starts_with_qi = first.1 == 0;
    for (name, lag, span) in &terms {
        if !is_declared(name) {
            return None;
        }
        if starts_with_qi == (*lag == 0) {
            continue;
        }
        return Some((
            sign_span(model, *span),
            "E362",
            "SVAR_IDENTIFICATION: a single restrictions must affect either Qi or Ri, but not both"
                .to_string(),
        ));
    }
    None
}

/// A `coeff(…)` term with the operator in front of it when the source has one;
/// their own column range covers the operator too.
fn sign_span(model: &Model, span: Span) -> Span {
    let bytes = model.source.as_bytes();
    let mut start = span.start as usize;
    let mut i = start;
    while i > 0 && bytes.get(i - 1).is_some_and(|b| b.is_ascii_whitespace()) {
        i -= 1;
    }
    if i > 0 && matches!(bytes.get(i - 1), Some(b'+') | Some(b'-')) {
        start = i - 1;
    }
    Span {
        start: start as u32,
        end: span.end,
    }
}

/// Every `coeff(name, lag)` term of one `restriction` element, in source order.
/// An occurrence that is not that shape yields no terms at all, because 7.1
/// refuses such a restriction with a syntax error rather than with a message.
fn restriction_terms(model: &Model, element_span: Span) -> Vec<(String, i64, Span)> {
    let Some(text) = model
        .source
        .get(element_span.start as usize..element_span.end as usize)
    else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut i = 0;
    while i < text.len() {
        if !at_ident(&text[i..], "coeff") {
            i += 1;
            continue;
        }
        match read_coeff_term(text, i) {
            Some((name, lag, end)) => {
                out.push((
                    name,
                    lag,
                    Span {
                        start: element_span.start + i as u32,
                        end: element_span.start + end as u32,
                    },
                ));
                i = end;
            }
            None => return Vec::new(),
        }
    }
    out
}

/// `coeff(name, lag)` starting at `from`: the name, the lag, and the offset one
/// past the `)`.
fn read_coeff_term(text: &str, from: usize) -> Option<(String, i64, usize)> {
    let bytes = text.as_bytes();
    let mut i = from + "coeff".len();
    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
        i += 1;
    }
    if bytes.get(i) != Some(&b'(') {
        return None;
    }
    i += 1;
    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
        i += 1;
    }
    let name_start = i;
    while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
        i += 1;
    }
    let name = text.get(name_start..i)?.to_string();
    if name.is_empty() {
        return None;
    }
    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
        i += 1;
    }
    if bytes.get(i) != Some(&b',') {
        return None;
    }
    i += 1;
    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
        i += 1;
    }
    let lag_start = i;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    if lag_start == i {
        return None;
    }
    let lag: i64 = text.get(lag_start..i)?.parse().ok()?;
    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
        i += 1;
    }
    if bytes.get(i) != Some(&b')') {
        return None;
    }
    Some((name, lag, i + 1))
}

/// Whether `text` starts with `name` and does not continue the identifier.
fn at_ident(text: &str, name: &str) -> bool {
    let Some(rest) = text.strip_prefix(name) else {
        return false;
    };
    match rest.as_bytes().first() {
        Some(c) => !(c.is_ascii_alphanumeric() || *c == b'_'),
        None => true,
    }
}

// ---------------------------------------------------------------------------
// `svar`
// ---------------------------------------------------------------------------

/// One refusal per statement, in their order. Returns `true` when it refused.
fn check_svar(src: &str, stmt: &MsStatement, out: &mut Vec<Diagnostic>) -> bool {
    if stmt.options.is_empty() {
        return false;
    }
    // `constants` is a token but not one of `svar`'s options, so every spelling
    // of it is a syntax error that never reaches their own `constants` sentence.
    if !svar_options_spellable(src, stmt) {
        return false;
    }
    let coefficients = has(stmt, "coefficients");
    let variances = has(stmt, "variances");
    if !coefficients && !variances {
        push(
            out,
            stmt.span,
            "E363",
            "You must pass one of 'coefficients', 'variances', or 'constants'.",
        );
        return true;
    }
    if coefficients && variances {
        push(
            out,
            stmt.span,
            "E364",
            "You may only pass one of 'coefficients', 'variances', or 'constants'.",
        );
        return true;
    }
    if !has(stmt, "chain") {
        push(
            out,
            stmt.span,
            "E365",
            "A 'chain' option must be passed to the 'svar' statement.",
        );
        return true;
    }
    let chain = option(stmt, "chain").and_then(|opt| unsigned_int(&opt.value_text));
    if chain == Some(0) {
        push(
            out,
            stmt.span,
            "E366",
            "The value passed to the 'chain' option must be greater than zero.",
        );
        return true;
    }
    // A value the grammar cannot spell never reaches their test.
    if chain.is_none() {
        return false;
    }
    let Some(equations) = option(stmt, "equations") else {
        return false;
    };
    // `equations=0` (the bare `vec_int_number` form) and `equations=[0, 1]` (the
    // bracketed `vec_int` form) reach the same sentence: their `checkPass` loops
    // over whichever the grammar produced.
    let values = match bracketed_entries(&equations.value_text) {
        Some(entries) => entries,
        None if !equations.value_text.trim().is_empty() => {
            vec![equations.value_text.trim().to_string()]
        }
        None => return false,
    };
    // `equations=[]` is a syntax error in 7.1, and an empty list holds no value
    // their `<= 0` test could name.
    if values.is_empty() {
        return false;
    }
    for value in &values {
        let Some(number) = plain_int(value) else {
            return false;
        };
        if number <= 0 {
            push(
                out,
                stmt.span,
                "E367",
                "The value(s) passed to the 'equations' option must be greater than zero.",
            );
            return true;
        }
    }
    false
}

/// The four option names and the shapes `svar` accepts. `constants` is a token
/// the production does not take, a signed or fractional `chain` and a non-vector
/// `equations` are syntax errors, so no sentence here may fire on them.
fn svar_options_spellable(src: &str, stmt: &MsStatement) -> bool {
    crate::shape_gate::command_options_spellable(src, "svar", &stmt.options)
}

// ---------------------------------------------------------------------------
// the four `ms_*` mutual-exclusion sentences
// ---------------------------------------------------------------------------

fn check_ms_mutual_exclusion(stmt: &MsStatement, out: &mut Vec<Diagnostic>) -> bool {
    let both = |a: &str, b: &str| has(stmt, a) && has(stmt, b);
    let message = if stmt
        .command
        .eq_ignore_ascii_case("ms_compute_probabilities")
    {
        both("real_time_smoothed", "filtered_probabilities").then_some((
            "E368",
            "You may only pass one of real_time_smoothed and filtered_probabilities to ms_compute_probabilities.",
        ))
    } else if stmt.command.eq_ignore_ascii_case("ms_irf") {
        (both("regime", "regimes")
            || both("filtered_probabilities", "regime")
            || both("filtered_probabilities", "regimes"))
        .then_some((
            "E369",
            "You may only pass one of regime, regimes and filtered_probabilities to ms_irf",
        ))
    } else if stmt.command.eq_ignore_ascii_case("ms_forecast") {
        both("regime", "regimes").then_some((
            "E370",
            "You may only pass one of regime and regimes to ms_forecast",
        ))
    } else if stmt
        .command
        .eq_ignore_ascii_case("ms_variance_decomposition")
    {
        (both("regime", "regimes")
            || both("filtered_probabilities", "regime")
            || both("filtered_probabilities", "regimes"))
        .then_some((
            "E371",
            "You may only pass one of regime, regimes and filtered_probabilities to ms_variance_decomposition",
        ))
    } else {
        None
    };
    if let Some((code, text)) = message {
        push(out, stmt.span, code, text);
        return true;
    }
    false
}

// ---------------------------------------------------------------------------
// the dotted `prior` statement
// ---------------------------------------------------------------------------

/// One `std(…)` / `corr(…)` prior head name, with what their parse makes of it.
///
/// These are the heads whose names 7.1 checks against the symbol table without
/// first asking for a parameter. Their `check_symbol_is_endogenous_or_exogenous`
/// runs three tests in one walk, and each prints a sentence of its own:
/// `check_symbol_existence` (**E058**), the `exogenousDet` arm (their
/// `N is an exogenous deterministic.`), and the fall-through (**E059**).
pub(crate) struct PriorHeadName {
    pub span: Span,
    pub name: Name,
    /// What their parse makes of this name.
    pub verdict: PriorHeadVerdict,
}

/// What their parse makes of one `std(…)` / `corr(…)` prior head name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PriorHeadVerdict {
    /// A plain `varexo`: accepted, no sentence.
    Exogenous,
    /// Accepted as an endogenous or exogenous name.
    Endogenous,
    /// The reader declares no such symbol — **E058**'s row.
    Undeclared,
    /// Declared `varexo_det` — their `N is an exogenous deterministic.` This
    /// slice closes it through **E317**, which already carries their
    /// `is not …` sentence for the family's wrong-type rows.
    ExogenousDeterministic,
    /// Declared, but neither endogenous nor exogenous — **E059**'s row.
    NotEndogenousOrExogenous,
}

/// The names a `std(…)` / `corr(…)` prior head carries that their parse can
/// refuse, in source order. The head's name checks run before the subsample
/// name is looked up, so a subsample form (`std(e).beta.prior(…)`) carries the
/// same verdicts; what it refuses *after* them — `A subsample statement has not
/// been issued for e` — belongs to **0.7**.
pub(crate) fn prior_std_corr_head_names(
    model: &Model,
    stmt: &DottedStatement,
) -> Vec<PriorHeadName> {
    if stmt.kind != DottedKind::Prior {
        return Vec::new();
    }
    let declared = declared_names(model);
    let endogenous = endogenous_names(model);
    let exogenous = name_set(model.exogenous.iter());
    let deterministic = name_set(model.deterministic_exogenous.iter());
    let pairs: Vec<(Span, Name)> = match &stmt.head {
        DottedHead::Std {
            first, first_span, ..
        } => vec![(*first_span, *first)],
        DottedHead::Corr {
            first,
            first_span,
            second,
            second_span,
            ..
        } => vec![(*first_span, *first), (*second_span, *second)],
        _ => return Vec::new(),
    };
    pairs
        .into_iter()
        .map(|(span, name)| {
            let verdict = if !declared.contains(&name) {
                PriorHeadVerdict::Undeclared
            } else if endogenous.contains(&name) {
                PriorHeadVerdict::Endogenous
            } else if deterministic.contains(&name) {
                PriorHeadVerdict::ExogenousDeterministic
            } else if exogenous.contains(&name) {
                PriorHeadVerdict::Exogenous
            } else {
                PriorHeadVerdict::NotEndogenousOrExogenous
            };
            PriorHeadName {
                span,
                name,
                verdict,
            }
        })
        .collect()
}

/// The prior refusals 7.1 prints while parsing: the plain and bracketed heads'
/// parameter test (**E378**) and the top-level `y = 3;` assignment, which their
/// `ParsingDriver::init_param` sends through the same `check_symbol_is_parameter`
/// the head does. Returns `true` when it refused.
///
/// The `std` / `corr` heads and the subsample forms are read and refused while
/// parsing too; their sentences belong to **E058** / **E059** (or to no 0.5.4 row
/// at all), so nothing here fires on them.
fn check_prior_head(model: &Model, stmt: &DottedStatement, out: &mut Vec<Diagnostic>) -> bool {
    if stmt.kind != DottedKind::Prior || !has_prior_body(model, stmt) {
        return false;
    }
    if head_refused_while_parsing(model, stmt) {
        return false;
    }
    let parameters = parameter_names(model);
    for name in prior_parameter_names(&stmt.head) {
        if !parameters.contains(&name) {
            push(
                out,
                stmt.span,
                "E378",
                format!("{} is not a parameter", model.name(name)),
            );
            return true;
        }
    }
    false
}

/// The same sentence on the top-level assignment. 7.1's `init_param` runs
/// `check_symbol_is_parameter` while parsing the statement, so `y = 3;` with a
/// declared `y` is refused exactly as `y.prior(…)` is. An undeclared head never
/// reaches it — the pin's lexer sends that line to native MATLAB. Returns `true`
/// when it refused.
fn check_top_assignment(
    model: &Model,
    assignment: &crate::model::Assignment,
    out: &mut Vec<Diagnostic>,
) -> bool {
    if parameter_names(model).contains(&assignment.name) {
        return false;
    }
    push(
        out,
        assignment.span,
        "E378",
        format!("{} is not a parameter", model.name(assignment.name)),
    );
    true
}

/// The prior body sentences of one statement, in 7.1's order: the joint head's
/// name count (**E377**) opens their check pass, the body's shape sentences
/// follow, and the `corr(A,B)` mixed-type rule (**E379**) is last — their
/// `CorrPriorStatement::checkPass` runs the basic pass first. Returns `true` when
/// it refused.
fn check_prior_body(model: &Model, stmt: &DottedStatement, out: &mut Vec<Diagnostic>) -> bool {
    if stmt.kind != DottedKind::Prior || !has_prior_body(model, stmt) {
        return false;
    }
    // Whatever their parse refuses inside the head stops the run before any of
    // these sentences.
    if head_refused_while_parsing(model, stmt) {
        return false;
    }
    let joint = matches!(stmt.head, DottedHead::Vec { .. });
    if !prior_body_spellable(&model.source, &stmt.options, joint) {
        return false;
    }
    if let DottedHead::Vec { names } = &stmt.head {
        if names.len() < 2 {
            push(
                out,
                stmt.span,
                "E377",
                "you must pass at least two parameters to the joint prior statement",
            );
            return true;
        }
    }
    if prior_shape_refusal(stmt, joint, out) {
        return true;
    }
    if let DottedHead::Corr { first, second, .. } = &stmt.head {
        let endogenous = endogenous_names(model);
        if endogenous.contains(first) != endogenous.contains(second) {
            push(
                out,
                stmt.span,
                "E379",
                format!(
                    "In the corr(A,B).prior statement, A and B must be of the same type. In your case, {} and {} are of different types.",
                    model.name(*first),
                    model.name(*second)
                ),
            );
            return true;
        }
    }
    false
}

/// Whether their parse stops inside the head, before any sentence this module
/// owns: a `std(…)` / `corr(…)` head carrying a name it refuses, or any subsample
/// form (`std(e).beta.prior(…)`, `alpha.b.prior(…)`), whose next sentence —
/// `A subsample statement has not been issued for e` — belongs to **0.7**.
///
/// Every one of those head refuses is a sentence another code carries, so this
/// only decides whether the body sentences may run.
fn head_refused_while_parsing(model: &Model, stmt: &DottedStatement) -> bool {
    let endogenous = endogenous_names(model);
    let exogenous = name_set(model.exogenous.iter());
    let refused = |name: &Name| !endogenous.contains(name) && !exogenous.contains(name);
    match &stmt.head {
        DottedHead::Std { first, second, .. } => second.is_some() || refused(first),
        DottedHead::Corr {
            first,
            second,
            third,
            ..
        } => third.is_some() || refused(first) || refused(second),
        DottedHead::Param { second, .. } => second.is_some(),
        // Every name of a bracketed head is declared: the pin's lexer sends the
        // line to native MATLAB otherwise, so no statement is recorded at all.
        DottedHead::Vec { .. } => false,
    }
}

/// The names a plain or bracketed head carries, which 7.1 checks with
/// `check_symbol_is_parameter`.
fn prior_parameter_names(head: &DottedHead) -> Vec<Name> {
    match head {
        DottedHead::Param { first, .. } => vec![*first],
        DottedHead::Vec { names } => names.iter().map(|(name, _)| *name).collect(),
        _ => Vec::new(),
    }
}

/// The body sentences. The single form takes exactly one of `stdev` /
/// `variance` and a two-value `domain`; the joint form checks only `shape`,
/// `mean` / `mode` and its four-value `domain`.
fn prior_shape_refusal(stmt: &DottedStatement, joint: bool, out: &mut Vec<Diagnostic>) -> bool {
    if option_of(&stmt.options, "shape").is_none() {
        push(
            out,
            stmt.span,
            "E372",
            "You must pass the shape option to the prior statement.",
        );
        return true;
    }
    if option_of(&stmt.options, "mean").is_none() && option_of(&stmt.options, "mode").is_none() {
        push(
            out,
            stmt.span,
            "E373",
            "You must pass at least one of mean and mode to the prior statement.",
        );
        return true;
    }
    if !joint {
        let stdev = option_of(&stmt.options, "stdev").is_some();
        let variance = option_of(&stmt.options, "variance").is_some();
        if stdev == variance {
            push(
                out,
                stmt.span,
                "E374",
                "You must pass exactly one of stdev and variance to the prior statement.",
            );
            return true;
        }
    }
    let Some(domain) = option_of(&stmt.options, "domain") else {
        return false;
    };
    let values = bracketed_entries(&domain.value_text).unwrap_or_default();
    // `domain=[]` is a syntax error in 7.1 and holds no value to count.
    if values.is_empty() {
        return false;
    }
    let wanted = if joint { 4 } else { 2 };
    if values.len() != wanted {
        let (code, message) = if joint {
            (
                "E376",
                "You must pass exactly four values to the domain option.",
            )
        } else {
            (
                "E375",
                "You must pass exactly two values to the domain option.",
            )
        };
        push(out, domain.span, code, message);
        return true;
    }
    false
}

/// Whether every option is one of the prior option names, written in the shape
/// the grammar gives it. An unknown option name, a flag with no value, a
/// bracketed value outside the vector options and `mean=[…]` on the single form
/// are all syntax errors.
fn prior_body_spellable(src: &str, options: &[FamilyOption], joint: bool) -> bool {
    crate::shape_gate::prior_options_spellable(src, options, joint)
}

/// Whether the statement's own text carries a `prior(…)` body rather than the
/// `prior = prior` copy form or a bare `alpha.prior;`.
fn has_prior_body(model: &Model, stmt: &DottedStatement) -> bool {
    model
        .source
        .get(stmt.span.start as usize..stmt.span.end as usize)
        .is_some_and(|text| text.contains('('))
}

/// One bracketed list's entries as written.
fn bracketed_entries(text: &str) -> Option<Vec<String>> {
    let trimmed = text.trim();
    let inner = trimmed.strip_prefix('[')?.strip_suffix(']')?;
    let mut out = Vec::new();
    for piece in inner.split([',', ' ', '\t']) {
        if piece.is_empty() {
            continue;
        }
        out.push(piece.to_string());
    }
    Some(out)
}

/// One matrix option's rows. `None` when the text is not a bracketed list of
/// bracketed rows.
fn matrix_rows(text: &str) -> Option<Vec<Vec<String>>> {
    let trimmed = text.trim();
    let inner = trimmed.strip_prefix('[')?.strip_suffix(']')?;
    let mut rows = Vec::new();
    let mut depth = 0i32;
    let mut current: Option<String> = None;
    for ch in inner.chars() {
        match ch {
            '[' => {
                depth += 1;
                if depth == 1 {
                    current = Some(String::new());
                } else {
                    return None;
                }
            }
            ']' => {
                depth -= 1;
                if depth != 0 {
                    return None;
                }
                let body = current.take()?;
                rows.push(
                    body.split(',')
                        .map(|piece| piece.trim().to_string())
                        .filter(|piece| !piece.is_empty())
                        .collect(),
                );
            }
            ',' if depth == 0 => {}
            _ => {
                let body = current.as_mut()?;
                body.push(ch);
            }
        }
    }
    if depth != 0 || rows.is_empty() {
        return None;
    }
    Some(rows)
}
