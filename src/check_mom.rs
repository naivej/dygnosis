//! D-mom (0.6.0 02): the refuses 7.1 prints for the statements P-mom stores.
//!
//! The four `method_of_moments` check sentences (**E382**–**E385**), the
//! matched-moment walk at a `matched_moments` block's `end;` (**E386**), the name
//! sentences on the five blocks' rows (**E058** / **E317** / **E387**), and the
//! `matched_irfs` / `matched_irfs_weights` row sentences (**E388**–**E392**).
//!
//! Order is 7.1's. Everything printed while reading the file — a row's name
//! checks, the counts, the dates, the duplicate rows, and the moment walk — runs
//! before any check pass, so one of those refuses beats every
//! `method_of_moments` sentence. Within each stage the first refusal in file
//! order wins, as their run stops there.
//!
//! The walk runs on the tree their parser builds, which simplifies as it reads:
//! `y*1` folds to `y`, `y-y` to zero, `y^(2-1)` to `y`, while `y*1.0` keeps a
//! constant operand. Two details matter and are mirrored here: a constant's
//! identity is the text it was interned under (`1` and `1.0` are different
//! constants, and only `0` is their zero), and `*` walks the operand their node
//! list holds first, which puts a constant before a variable.

use std::cell::Cell;
use std::collections::{HashMap, HashSet};

use crate::diagnostic::{Diagnostic, Severity};
use crate::expr::{BinOp, ExprId, ExprKind, UnOp};
use crate::intern::Name;
use crate::model::{
    FamilyValueKind, IrfCalibrationRow, MatchedIrfsBlock, MatchedIrfsRow, MatchedIrfsWeight,
    MatchedIrfsWeightsBlock, MatchedMoment, Model, MomStatement,
};
use crate::span::Span;

const E382_MSG: &str = "The 'method_of_moments' statement requires a method to be supplied via the 'mom_method' option. Possible values are 'GMM', 'SMM', or 'IRF_MATCHING'.";
const E383_MSG: &str = "The 'method_of_moments' statement requires a data file to be supplied via the 'datafile' option.";
const E385_MSG: &str =
    "method_of_moments: can only use one of HP, one-sided HP, and bandpass filters";
const E386_PREFIX: &str = "Matched moment expression has incorrect format: ";
const E390_MSG: &str = "matched_irfs: the 'periods' and 'values' keywords are not followed by the same number of elements";
const E391_MSG: &str = "matched_irfs: the 'periods' and 'weights' keywords are not followed by the same number of elements";
const E392_MSG: &str = "matched_irfs: dates are not allowed in the 'periods' keyword";

/// The methods the grammar spells as a bare token.
const METHODS: &[&str] = &["GMM", "SMM", "IRF_MATCHING"];

/// The one refusal this family's run would print, or none.
pub fn check_mom(model: &Model) -> Vec<Diagnostic> {
    let ctx = Ctx::new(model);
    let mut units: Vec<Unit> = Vec::new();
    let mut seq = 0u32;
    for syn in &model.mom_syntax {
        push(
            &mut units,
            &mut seq,
            syn.span.start,
            0,
            err(syn.span, "E001", syn.message.clone()),
        );
    }
    collect_mom_statements(&ctx, &mut units, &mut seq);
    collect_matched_moments(&ctx, &mut units, &mut seq);
    collect_matched_irfs(&ctx, &mut units, &mut seq);
    collect_matched_irfs_weights(&ctx, &mut units, &mut seq);
    collect_calibration(&ctx, &mut units, &mut seq);
    if let Some(unit) = units.into_iter().min_by_key(|u| (u.at, u.order, u.seq)) {
        return vec![unit.diag];
    }
    check_statements(&ctx)
}

/// One parse-stage refusal: where 7.1's parser stops, which check inside that
/// spot it is, and the sentence.
struct Unit {
    at: u32,
    order: u32,
    seq: u32,
    diag: Diagnostic,
}

fn push(units: &mut Vec<Unit>, seq: &mut u32, at: u32, order: u32, diag: Diagnostic) {
    units.push(Unit {
        at,
        order,
        seq: *seq,
        diag,
    });
    *seq += 1;
}

/// Our wording for a shape 7.1 refuses with generic bison text, in the same form
/// the MS-SBVAR family uses (close call 1, picked **A**).
fn shape_message(subject: &str, expected: &str) -> String {
    crate::model::shape_refuse_wording(subject, expected)
}

fn err(span: Span, code: &str, message: impl Into<String>) -> Diagnostic {
    Diagnostic::new(span, Severity::Error, code, message)
}

// ---------------------------------------------------------------------------
// The parse stage
// ---------------------------------------------------------------------------

/// The shapes P-mom recorded but the grammar has no production for: an empty
/// `(…)`, and a `mom_method` whose value is not one of the three bare methods.
fn collect_mom_statements(ctx: &Ctx<'_>, units: &mut Vec<Unit>, seq: &mut u32) {
    for stmt in &ctx.model.mom_statements {
        if stmt.has_option_list && stmt.options.is_empty() {
            push(
                units,
                seq,
                stmt.span.start,
                0,
                err(
                    stmt.span,
                    "E001",
                    shape_message("method_of_moments", "at least one option"),
                ),
            );
            continue;
        }
        let Some(opt) = stmt
            .options
            .iter()
            .find(|opt| opt.name.eq_ignore_ascii_case("mom_method"))
        else {
            continue;
        };
        let spelled = opt.has_value
            && opt.value_kind == FamilyValueKind::Scalar
            && is_bare_method(&opt.value_text);
        if !spelled {
            push(
                units,
                seq,
                opt.span.start,
                0,
                err(
                    opt.span,
                    "E001",
                    shape_message("method_of_moments", "GMM, SMM, or IRF_MATCHING"),
                ),
            );
        }
    }
}

/// `mom_method=GMM` and friends: a bare identifier, one of the three methods.
/// The lexer's own tokens, so the case does not matter.
fn is_bare_method(text: &str) -> bool {
    METHODS
        .iter()
        .any(|method| text.eq_ignore_ascii_case(method))
}

/// `matched_moments`: a block the reader could not store a row for, then each
/// row's own scope refuse, then the walk at the block's `end;`.
fn collect_matched_moments(ctx: &Ctx<'_>, units: &mut Vec<Unit>, seq: &mut u32) {
    for block in &ctx.model.matched_moments_blocks {
        let rows: Vec<&MatchedMoment> = ctx
            .model
            .matched_moments
            .iter()
            .filter(|row| row.span.start >= block.start && row.span.start < block.end)
            .collect();
        // No stored row covers both the empty body and a body whose only row the
        // grammar has no production for: 7.1 refuses both while reading.
        if rows.is_empty() && !ctx.syntax_in(*block) {
            push(
                units,
                seq,
                block.start,
                0,
                err(
                    *block,
                    "E001",
                    shape_message("matched_moments", "at least one row"),
                ),
            );
            continue;
        }
        for row in &rows {
            let Some(expr) = row.expr else {
                continue;
            };
            // Their parser refuses the row itself when it names a mod-file local
            // or an external function, before the walk at `end;` runs.
            if let Some((pos, diag)) = ctx.row_scope_refusal(expr) {
                push(units, seq, pos, 0, diag);
            }
        }
        if let Some(diag) = walk_block(ctx, &rows, block.end) {
            push(units, seq, block.end, 4, diag);
        }
    }
}

fn collect_matched_irfs(ctx: &Ctx<'_>, units: &mut Vec<Unit>, seq: &mut u32) {
    for block in &ctx.model.matched_irfs {
        if block.rows.is_empty() && !ctx.syntax_in(block.span) {
            push(
                units,
                seq,
                block.span.start,
                0,
                err(
                    block.span,
                    "E001",
                    shape_message("matched_irfs", "at least one row"),
                ),
            );
            continue;
        }
        for (index, row) in block.rows.iter().enumerate() {
            let at = row.span.start;
            if let Some(diag) = irfs_row_names(ctx, row) {
                push(units, seq, at, 0, diag);
            } else if let Some(diag) = irfs_row_exprs(ctx, row) {
                push(units, seq, diag.span.start, 0, diag);
            } else if let Some(diag) = irfs_row_counts(row) {
                push(units, seq, at, 1, diag);
            } else if let Some(diag) = irfs_row_date(ctx, row) {
                push(units, seq, at, 2, diag);
            } else if let Some(diag) = irfs_pair_seen(ctx, block, index) {
                push(units, seq, at, 3, diag);
            }
        }
    }
}

/// `matched_irfs_weights`: each row's names, then its tuple.
fn collect_matched_irfs_weights(ctx: &Ctx<'_>, units: &mut Vec<Unit>, seq: &mut u32) {
    for block in &ctx.model.matched_irfs_weights {
        if block.rows.is_empty() && !ctx.syntax_in(block.span) {
            push(
                units,
                seq,
                block.span.start,
                0,
                err(
                    block.span,
                    "E001",
                    shape_message("matched_irfs_weights", "at least one row"),
                ),
            );
            continue;
        }
        for (index, row) in block.rows.iter().enumerate() {
            let at = row.span.start;
            if let Some(diag) = weights_row_names(ctx, row) {
                push(units, seq, at, 0, diag);
            } else if let Some(diag) = row.weight_expr.and_then(|id| ctx.outside_expr(id)) {
                push(units, seq, diag.span.start, 0, diag);
            } else if let Some(diag) = weights_tuple_seen(ctx, block, index) {
                push(units, seq, at, 3, diag);
            }
        }
    }
}

/// `moment_calibration` and `irf_calibration`: each row's name slots.
fn collect_calibration(ctx: &Ctx<'_>, units: &mut Vec<Unit>, seq: &mut u32) {
    for block in &ctx.model.moment_calibration {
        if block.rows.is_empty() && !ctx.syntax_in(block.span) {
            push(
                units,
                seq,
                block.span.start,
                0,
                err(
                    block.span,
                    "E001",
                    shape_message("moment_calibration", "at least one row"),
                ),
            );
            continue;
        }
        for row in &block.rows {
            let hit = endo_slot(ctx, row.first, row.first_span, "moment_calibration")
                .or_else(|| endo_slot(ctx, row.second, row.second_span, "moment_calibration"));
            if let Some(diag) = hit {
                push(units, seq, row.span.start, 0, diag);
            }
        }
    }
    for block in &ctx.model.irf_calibration {
        if block.rows.is_empty() && !ctx.syntax_in(block.span) {
            push(
                units,
                seq,
                block.span.start,
                0,
                err(
                    block.span,
                    "E001",
                    shape_message("irf_calibration", "at least one row"),
                ),
            );
            continue;
        }
        for row in &block.rows {
            let hit = endo_slot(ctx, row.endogenous, row.endogenous_span, "irf_calibration")
                .or_else(|| irf_calibration_shock(ctx, row));
            if let Some(diag) = hit {
                push(units, seq, row.span.start, 0, diag);
            }
        }
    }
}

/// A `matched_irfs` row's names, in the grammar's order — the endogenous slot
/// then the shock — whatever order they were written in.
fn irfs_row_names(ctx: &Ctx<'_>, row: &MatchedIrfsRow) -> Option<Diagnostic> {
    endo_slot(ctx, row.endogenous, row.endogenous_span, "matched_irfs")
        .or_else(|| exo_slot(ctx, row.exogenous, row.exogenous_span, "matched_irfs"))
}

/// A `values` or `weights` expression. Their type and lead checks run while the
/// expression is read, before the count sentences.
fn irfs_row_exprs(ctx: &Ctx<'_>, row: &MatchedIrfsRow) -> Option<Diagnostic> {
    row.value_exprs
        .iter()
        .chain(&row.weight_exprs)
        .find_map(|id| ctx.outside_expr(*id))
}

/// A `matched_irfs_weights` row's four names: each pair checks its endogenous
/// and then its shock, the left pair first.
fn weights_row_names(ctx: &Ctx<'_>, row: &MatchedIrfsWeight) -> Option<Diagnostic> {
    endo_slot(
        ctx,
        row.left_endo,
        row.left_endo_span,
        "matched_irfs_weights",
    )
    .or_else(|| exo_slot(ctx, row.left_exo, row.left_exo_span, "matched_irfs_weights"))
    .or_else(|| {
        endo_slot(
            ctx,
            row.right_endo,
            row.right_endo_span,
            "matched_irfs_weights",
        )
    })
    .or_else(|| {
        exo_slot(
            ctx,
            row.right_exo,
            row.right_exo_span,
            "matched_irfs_weights",
        )
    })
}

/// `periods` against `values`, then `periods` against `weights`. One weight is
/// legal: they copy it later.
fn irfs_row_counts(row: &MatchedIrfsRow) -> Option<Diagnostic> {
    if !row.periods.is_empty() && !row.values.is_empty() && row.periods.len() != row.values.len() {
        return Some(err(row.span, "E390", E390_MSG));
    }
    if !row.periods.is_empty() && row.weights.len() > 1 && row.periods.len() != row.weights.len() {
        return Some(err(row.span, "E391", E391_MSG));
    }
    None
}

/// A period entry written as a date. `1` and `1:2` are periods; `2000Q1` and
/// `2000Q1:2000Q4` are dates the row cannot take.
fn irfs_row_date(ctx: &Ctx<'_>, row: &MatchedIrfsRow) -> Option<Diagnostic> {
    for entry in &row.periods {
        let text = ctx.source(*entry);
        let dated = match text.split_once(':') {
            Some((first, second)) => {
                crate::model::dynare_date(first) && crate::model::dynare_date(second)
            }
            None => crate::model::dynare_date(text),
        };
        if dated {
            return Some(err(*entry, "E392", E392_MSG));
        }
    }
    None
}

/// The pair this block already holds, for their duplicate sentence. Each block
/// keeps its own map, so two blocks may repeat a pair.
fn irfs_pair_seen(ctx: &Ctx<'_>, block: &MatchedIrfsBlock, index: usize) -> Option<Diagnostic> {
    let row = &block.rows[index];
    let seen = block.rows[..index]
        .iter()
        .any(|earlier| earlier.endogenous == row.endogenous && earlier.exogenous == row.exogenous);
    seen.then(|| {
        err(
            row.span,
            "E388",
            format!(
                "matched_irfs: the pair endogenous {} with exogenous {} appears two times",
                ctx.model.name(row.endogenous),
                ctx.model.name(row.exogenous)
            ),
        )
    })
}

/// The six-part tuple this weights block already holds. Their sentence is their
/// own formatting: no spaces, each period as written.
fn weights_tuple_seen(
    ctx: &Ctx<'_>,
    block: &MatchedIrfsWeightsBlock,
    index: usize,
) -> Option<Diagnostic> {
    let row = &block.rows[index];
    let mine = tuple_key(row);
    let seen = block.rows[..index]
        .iter()
        .any(|earlier| tuple_key(earlier) == mine);
    seen.then(|| {
        err(
            row.span,
            "E389",
            format!(
                "matched_irfs: the tuple ({}({}),{},{}({}),{}) appears two times",
                ctx.model.name(row.left_endo),
                row.left_periods,
                ctx.model.name(row.left_exo),
                ctx.model.name(row.right_endo),
                row.right_periods,
                ctx.model.name(row.right_exo)
            ),
        )
    })
}

fn tuple_key(row: &MatchedIrfsWeight) -> (Name, String, Name, Name, String, Name) {
    (
        row.left_endo,
        row.left_periods.clone(),
        row.left_exo,
        row.right_endo,
        row.right_periods.clone(),
        row.right_exo,
    )
}

// ---------------------------------------------------------------------------
// The name slots
// ---------------------------------------------------------------------------

/// A name that must be endogenous.
fn endo_slot(ctx: &Ctx<'_>, name: Name, span: Span, block: &str) -> Option<Diagnostic> {
    if !ctx.declared(name) {
        return Some(undeclared(ctx, name, span, block));
    }
    if !ctx.endogenous(name) {
        return Some(err(
            span,
            "E317",
            format!("{} is not endogenous.", ctx.model.name(name)),
        ));
    }
    None
}

/// A name that must be exogenous, in the shock slot of `matched_irfs` and
/// `matched_irfs_weights`. A `varexo_det` is their other **E317** sentence.
fn exo_slot(ctx: &Ctx<'_>, name: Name, span: Span, block: &str) -> Option<Diagnostic> {
    if !ctx.declared(name) {
        return Some(undeclared(ctx, name, span, block));
    }
    // `varexo_det` names sit in both lists here, so test the deterministic one
    // first: only a plain `varexo` takes the shock slot.
    if ctx.deterministic_exogenous(name) {
        return Some(err(
            span,
            "E317",
            format!("{} is an exogenous deterministic.", ctx.model.name(name)),
        ));
    }
    if ctx.exogenous(name) {
        return None;
    }
    Some(err(
        span,
        "E387",
        format!("{} is not exogenous.", ctx.model.name(name)),
    ))
}

/// The `irf_calibration` shock. Their sentence names the **endogenous**: their
/// `ParsingDriver` passes `endo` into the format string (close call 2, **A**).
fn irf_calibration_shock(ctx: &Ctx<'_>, row: &IrfCalibrationRow) -> Option<Diagnostic> {
    let name = row.exogenous;
    if !ctx.declared(name) {
        return Some(undeclared(ctx, name, row.exogenous_span, "irf_calibration"));
    }
    if ctx.exogenous(name) {
        return None;
    }
    Some(err(
        row.endogenous_span,
        "E387",
        format!(
            "Variable {} is not an exogenous.",
            ctx.model.name(row.endogenous)
        ),
    ))
}

/// The shipped **E058** sentence: the editor names the undeclared entry and its
/// block, where their string is the generic `Unknown symbol`.
fn undeclared(ctx: &Ctx<'_>, name: Name, span: Span, block: &str) -> Diagnostic {
    err(
        span,
        "E058",
        format!(
            "Variable '{}' in {block} is not declared.",
            ctx.model.name(name)
        ),
    )
}

// ---------------------------------------------------------------------------
// The matched-moment walk
// ---------------------------------------------------------------------------

/// The walk over one `matched_moments` block, at the block's `end;`. The first
/// row whose expression fails supplies the sentence.
fn walk_block(ctx: &Ctx<'_>, rows: &[&MatchedMoment], end: u32) -> Option<Diagnostic> {
    for row in rows {
        let Some(expr) = row.expr else {
            continue;
        };
        if let Some(reason) = ctx.walk(expr, end) {
            return Some(err(
                ctx.model.exprs.get(expr).span,
                "E386",
                format!("{E386_PREFIX}{reason}"),
            ));
        }
    }
    None
}

/// The value the walk reasons about: the tree their parser builds, with the
/// simplifications it applies while reading. `index` stands for the position the
/// value's node holds in their node list, which decides operand order.
#[derive(Clone, Debug)]
enum Folded {
    Var {
        name: Name,
        timing: i32,
        index: u32,
    },
    Num {
        /// The text their constant table holds this value under. Two constants
        /// are the same node only when their texts match.
        canon: String,
        value: f64,
        index: u32,
    },
    UMinus(Box<Folded>),
    Plus(Box<Folded>, Box<Folded>),
    Minus(Box<Folded>, Box<Folded>),
    Times(Box<Folded>, Box<Folded>),
    Divide(Box<Folded>, Box<Folded>),
    Pow(Box<Folded>, Box<Folded>),
    /// A node the walk refuses as an operator: a comparison, `min`, `max`, and a
    /// `diff` with a lead (their `AddDiff` expands it into a subtraction).
    BinaryOp,
    /// Anything else: a call, `STEADY_STATE`, an expectation.
    Other,
    /// The row was refused while the tree was built (a denominator folding to
    /// their zero, `log(0)`); the shipped **E278** / **E276** path reports it.
    Refused,
}

impl PartialEq for Folded {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (
                Folded::Var {
                    name: a,
                    timing: at,
                    ..
                },
                Folded::Var {
                    name: b,
                    timing: bt,
                    ..
                },
            ) => a == b && at == bt,
            (Folded::Num { canon: a, .. }, Folded::Num { canon: b, .. }) => a == b,
            (Folded::UMinus(a), Folded::UMinus(b)) => a == b,
            (Folded::Plus(a1, a2), Folded::Plus(b1, b2))
            | (Folded::Minus(a1, a2), Folded::Minus(b1, b2))
            | (Folded::Times(a1, a2), Folded::Times(b1, b2))
            | (Folded::Divide(a1, a2), Folded::Divide(b1, b2))
            | (Folded::Pow(a1, a2), Folded::Pow(b1, b2)) => a1 == b1 && a2 == b2,
            (Folded::BinaryOp, Folded::BinaryOp) | (Folded::Other, Folded::Other) => true,
            _ => false,
        }
    }
}

impl Folded {
    fn is_zero(&self) -> bool {
        matches!(self, Folded::Num { canon, .. } if canon == "0")
    }

    fn is_one(&self) -> bool {
        matches!(self, Folded::Num { canon, .. } if canon == "1")
    }

    /// Their `MinusOne` is the unary-minus node over their one.
    fn is_minus_one(&self) -> bool {
        matches!(self, Folded::UMinus(inner) if inner.is_one())
    }

    fn index(&self) -> u32 {
        match self {
            Folded::Var { index, .. } | Folded::Num { index, .. } => *index,
            Folded::UMinus(inner) => inner.index(),
            _ => u32::MAX,
        }
    }
}

struct Ctx<'a> {
    model: &'a Model,
    endogenous: HashSet<Name>,
    exogenous: HashSet<Name>,
    deterministic_exogenous: HashSet<Name>,
    declared: HashSet<Name>,
    /// `#name` model-locals, a symbol type of their own: they are legal in the
    /// model tree and refused by the walk on their type.
    model_locals: HashSet<Name>,
    /// Names whose first appearance in the file is inside a `matched_moments`
    /// row. Their parser registers those as it reads the row, so the row is not
    /// refused for them; the walk reports them on their type.
    born_in_moment_row: HashSet<Name>,
    /// Where each `(name, timing)` first appears, standing in for their node
    /// order: their tree creates a node the first time the file mentions it.
    order: HashMap<(Name, i32), u32>,
    next_index: Cell<u32>,
}

/// The indices their predefined constants hold, all below any variable.
const ZERO_INDEX: u32 = 0;
const ONE_INDEX: u32 = 1;
const TWO_INDEX: u32 = 2;
const THREE_INDEX: u32 = 3;
/// The first index a value created while reading the file can hold.
const FIRST_FREE_INDEX: u32 = 4;

impl<'a> Ctx<'a> {
    fn new(model: &'a Model) -> Self {
        let endogenous: HashSet<Name> = model.endogenous.iter().map(|d| d.name).collect();
        let exogenous: HashSet<Name> = model.exogenous.iter().map(|d| d.name).collect();
        let deterministic_exogenous: HashSet<Name> = model
            .deterministic_exogenous
            .iter()
            .map(|d| d.name)
            .collect();
        // A mod-file local, a model-local, and a trend variable are all in their
        // symbol table, so a name slot reports their type sentence rather than
        // `Unknown symbol`.
        let model_locals: HashSet<Name> = model
            .equations
            .iter()
            .filter(|eq| eq.is_local)
            .filter_map(|eq| match eq.lhs_expr.map(|id| &model.exprs.get(id).kind) {
                Some(ExprKind::Ident { name, .. }) => Some(*name),
                _ => None,
            })
            .collect();
        let mut declared: HashSet<Name> = crate::check_d_ms::declared_names(model);
        declared.extend(model.mod_file_locals.iter().copied());
        declared.extend(model.excluded_endogenous.iter().map(|d| d.name));
        declared.extend(model.trend_vars.iter().map(|t| t.name));
        declared.extend(model_locals.iter().copied());
        // An external function name is in their symbol table too, so a name slot
        // reports its type sentence rather than `Unknown symbol`.
        declared.extend(model.external_function_names.iter().copied());

        // A name's node is created where the file first writes it, in token
        // order. Sorting the expression arena's identifiers by position mirrors
        // that closely enough to order two operands of one product.
        let mut idents: Vec<(u32, Name, i32)> = Vec::new();
        for (_, expr) in model.exprs.iter() {
            if let ExprKind::Ident {
                name,
                timing,
                ident_span,
                ..
            } = &expr.kind
            {
                idents.push((ident_span.start, *name, *timing));
            }
        }
        idents.sort_unstable_by_key(|(start, _, _)| *start);
        let mut order: HashMap<(Name, i32), u32> = HashMap::new();
        // Where the file first writes each name.
        let mut born_at: HashMap<Name, u32> = HashMap::new();
        let mut next = FIRST_FREE_INDEX;
        for (start, name, timing) in idents {
            born_at.entry(name).or_insert(start);
            order.entry((name, timing)).or_insert_with(|| {
                let index = next;
                next += 1;
                index
            });
        }
        // A name whose first appearance is inside a moment row is theirs to
        // register; one born anywhere else is already a mod-file local when a
        // row mentions it.
        let row_ranges: Vec<(u32, u32)> = model
            .matched_moments
            .iter()
            .map(|row| (row.span.start, row.span.end))
            .collect();
        let born_in_moment_row: HashSet<Name> = born_at
            .iter()
            .filter(|(_, at)| {
                row_ranges
                    .iter()
                    .any(|(start, end)| **at >= *start && **at < *end)
            })
            .map(|(name, _)| *name)
            .collect();
        Self {
            model,
            endogenous,
            exogenous,
            deterministic_exogenous,
            declared,
            model_locals,
            born_in_moment_row,
            order,
            next_index: Cell::new(next),
        }
    }

    fn declared(&self, name: Name) -> bool {
        self.declared.contains(&name)
    }

    /// Endogenous as of `at`: a later `model_remove` has not run yet.
    fn endogenous(&self, name: Name) -> bool {
        self.endogenous.contains(&name) || self.model.surgery_exit_after(name, 0)
    }

    /// A plain `varexo`. The parser records a `varexo_det` name in both lists, so
    /// the deterministic set is what separates the two.
    fn exogenous(&self, name: Name) -> bool {
        self.exogenous.contains(&name) && !self.deterministic_exogenous.contains(&name)
    }

    fn deterministic_exogenous(&self, name: Name) -> bool {
        self.deterministic_exogenous.contains(&name)
    }

    fn syntax_in(&self, block: Span) -> bool {
        self.model
            .mom_syntax
            .iter()
            .any(|syn| syn.span.start >= block.start && syn.span.start < block.end)
    }

    /// An expression outside `model`: a `#` local, a trend, an external function
    /// used as a name, or a parenthesized declared variable. A parameter is legal.
    fn outside_expr(&self, id: ExprId) -> Option<Diagnostic> {
        outside_model_expression_with_locals(self.model, id, &self.model_locals)
    }

    fn source(&self, span: Span) -> &str {
        self.model
            .source
            .get(span.start as usize..span.end as usize)
            .unwrap_or("")
    }

    /// A row of `matched_moments` is a model expression. A name the file first
    /// writes **inside a moment row** is registered by that row and read on: the
    /// walk refuses it later, at the block's `end;`, with
    /// `Variable {name} is not an endogenous`. A name already in their table when
    /// the row is read — born in the model block, in another block's expression
    /// slot, or by a declaration — is refused with the scope sentence instead.
    /// **E280** covers an external-function name, which their parser tests first.
    ///
    /// A `#` model-local is a symbol type of their own and is legal in the model
    /// tree, so it goes to the walk on its type.
    fn row_scope_refusal(&self, expr: ExprId) -> Option<(u32, Diagnostic)> {
        for ident in self.model.exprs.walk_idents(expr) {
            let name = self.model.name(ident.name);
            // An external function name is in their table before the row is read,
            // and their parser tests that type first — even though the name may
            // never have been written in an expression before.
            if self.model.external_function_names.contains(&ident.name) {
                return Some((
                    ident.span.start,
                    err(
                        ident.span,
                        "E280",
                        crate::model::external_function_in_model_message(name),
                    ),
                ));
            }
            // A lead on a `varexo_det` is refused while the expression is read,
            // before the walk at `end;`.
            if ident.timing != 0 && self.deterministic_exogenous.contains(&ident.name) {
                let end = ident.timing_span.map(|t| t.end).unwrap_or(ident.span.end);
                return Some((
                    ident.span.start,
                    err(
                        Span {
                            start: ident.span.start,
                            end,
                        },
                        "E024",
                        format!(
                            "Exogenous deterministic variable {name} cannot be given a lead or a lag"
                        ),
                    ),
                ));
            }
            if self.model_locals.contains(&ident.name)
                || self.born_in_moment_row.contains(&ident.name)
            {
                continue;
            }
            if self.model.mod_file_locals.contains(&ident.name) {
                return Some((
                    ident.span.start,
                    err(
                        ident.span,
                        "E281",
                        crate::model::mod_file_local_in_model_message(name),
                    ),
                ));
            }
        }
        None
    }

    /// The first reason the walk finds, in their order.
    fn walk(&self, expr: ExprId, at: u32) -> Option<String> {
        self.walk_folded(&self.fold(expr), at)
    }

    fn walk_folded(&self, folded: &Folded, at: u32) -> Option<String> {
        match folded {
            Folded::Var { name, .. } => {
                if self.endogenous.contains(name) || self.model.surgery_exit_after(*name, at) {
                    None
                } else {
                    Some(format!(
                        "Variable {} is not an endogenous",
                        self.model.name(*name)
                    ))
                }
            }
            // Their tree holds the operands sorted by node index, and every
            // constant is interned before any variable, so the constant side is
            // walked first.
            Folded::Times(first, second) => {
                let (first, second) = if first.index() <= second.index() {
                    (first, second)
                } else {
                    (second, first)
                };
                self.walk_folded(first, at)
                    .or_else(|| self.walk_folded(second, at))
            }
            Folded::Pow(left, right) => {
                if !matches!(**left, Folded::Var { .. }) {
                    return Some(
                        "First argument of power expression must be a variable".to_string(),
                    );
                }
                match &**right {
                    Folded::Num { value, .. } if *value > 0.0 && value.fract() == 0.0 => {}
                    _ => {
                        return Some(
                            "Second argument of power expression must be a positive integer"
                                .to_string(),
                        )
                    }
                }
                self.walk_folded(left, at)
            }
            Folded::Plus(..) | Folded::Minus(..) | Folded::Divide(..) | Folded::BinaryOp => {
                Some("Unsupported binary operator".to_string())
            }
            Folded::Refused => None,
            Folded::Num { .. } | Folded::UMinus(_) | Folded::Other => {
                Some("Unsupported expression".to_string())
            }
        }
    }

    /// Their tree as their parser builds it, with its simplifications applied.
    fn fold(&self, expr: ExprId) -> Folded {
        match &self.model.exprs.get(expr).kind {
            ExprKind::Ident { name, timing, .. } => Folded::Var {
                name: *name,
                timing: *timing,
                index: self
                    .order
                    .get(&(*name, *timing))
                    .copied()
                    .unwrap_or_else(|| self.fresh_index()),
            },
            ExprKind::Number => {
                let text = self.source(self.model.exprs.get(expr).span);
                match text.parse::<f64>() {
                    Ok(value) => self.num_written(value, text),
                    Err(_) => Folded::Other,
                }
            }
            ExprKind::Unary { op: UnOp::Pos, arg } => self.fold(*arg),
            ExprKind::Unary { op: UnOp::Neg, arg } => uminus(self.fold(*arg)),
            ExprKind::Binary { op, lhs, rhs } => {
                let left = self.fold(*lhs);
                let right = self.fold(*rhs);
                match op {
                    BinOp::Add => self.plus(left, right),
                    BinOp::Sub => self.minus(left, right),
                    BinOp::Mul => self.times(left, right),
                    BinOp::Div => self.divide(left, right),
                    BinOp::Pow => self.power(left, right),
                    // Comparisons fold only when both sides are constants.
                    _ => match (&left, &right) {
                        (Folded::Num { value: a, .. }, Folded::Num { value: b, .. }) => {
                            self.compare(*op, *a, *b)
                        }
                        _ => Folded::BinaryOp,
                    },
                }
            }
            ExprKind::Call { callee, args } => self.fold_call(*callee, args),
            ExprKind::SteadyState { .. } | ExprKind::Expectation { .. } => Folded::Other,
            ExprKind::String | ExprKind::Error => Folded::Other,
        }
    }

    fn fresh_index(&self) -> u32 {
        let index = self.next_index.get();
        self.next_index.set(index + 1);
        index
    }

    /// A literal keeps its text: their table keys a constant on what was
    /// written, so `1` and `1.0` are two nodes.
    fn num_written(&self, value: f64, text: &str) -> Folded {
        let canon = text.to_string();
        let index = match canon.as_str() {
            "0" => ZERO_INDEX,
            "1" => ONE_INDEX,
            "2" => TWO_INDEX,
            "3" => THREE_INDEX,
            _ => self.fresh_index(),
        };
        Folded::Num {
            canon,
            value,
            index,
        }
    }

    /// A value their tree computed while reading, keyed on its lossless text.
    fn num_computed(&self, value: f64) -> Folded {
        let canon = lossless(value);
        let index = match canon.as_str() {
            "0" => ZERO_INDEX,
            "1" => ONE_INDEX,
            "2" => TWO_INDEX,
            "3" => THREE_INDEX,
            _ => self.fresh_index(),
        };
        Folded::Num {
            canon,
            value,
            index,
        }
    }

    /// A call is a constant when every argument is one and the function is a
    /// plain mathematical one. `log(0)` and `log10(0)` refuse the file while the
    /// tree is built.
    fn fold_call(&self, callee: Name, args: &[ExprId]) -> Folded {
        let name = self.model.name(callee).to_ascii_lowercase();
        if name == "diff" {
            // Their `AddDiff` expands a diff over a lead into a subtraction.
            return match args.first() {
                Some(arg) if has_lead(self.model, *arg) => Folded::BinaryOp,
                _ => Folded::Other,
            };
        }
        let values: Option<Vec<f64>> = args
            .iter()
            .map(|arg| match self.fold(*arg) {
                Folded::Num { value, .. } => Some(value),
                _ => None,
            })
            .collect();
        let Some(values) = values else {
            // `min` and `max` are binary nodes; every other call is unary or
            // n-ary, which the walk refuses as an expression.
            return if (name == "min" || name == "max") && args.len() == 2 {
                Folded::BinaryOp
            } else {
                Folded::Other
            };
        };
        let unary = |f: fn(f64) -> f64| match values.as_slice() {
            [x] => Some(f(*x)),
            _ => None,
        };
        let value = match name.as_str() {
            "exp" => unary(f64::exp),
            "log" | "ln" => {
                if values.first() == Some(&0.0) {
                    return Folded::Refused;
                }
                unary(f64::ln)
            }
            "log10" => {
                if values.first() == Some(&0.0) {
                    return Folded::Refused;
                }
                unary(f64::log10)
            }
            "sqrt" => unary(f64::sqrt),
            "cbrt" => unary(f64::cbrt),
            "abs" => unary(f64::abs),
            "sign" => unary(f64::signum),
            "sin" => unary(f64::sin),
            "cos" => unary(f64::cos),
            "tan" => unary(f64::tan),
            "asin" => unary(f64::asin),
            "acos" => unary(f64::acos),
            "atan" => unary(f64::atan),
            "sinh" => unary(f64::sinh),
            "cosh" => unary(f64::cosh),
            "tanh" => unary(f64::tanh),
            "asinh" => unary(f64::asinh),
            "acosh" => unary(f64::acosh),
            "atanh" => unary(f64::atanh),
            "min" => match values.as_slice() {
                [a, b] => Some(a.min(*b)),
                _ => None,
            },
            "max" => match values.as_slice() {
                [a, b] => Some(a.max(*b)),
                _ => None,
            },
            _ => None,
        };
        match value {
            Some(value) if value.is_finite() => {
                if value < 0.0 {
                    Folded::UMinus(Box::new(self.num_computed(-value)))
                } else {
                    self.num_computed(value)
                }
            }
            Some(_) => Folded::Other,
            None => Folded::Other,
        }
    }

    /// Their `AddPlus`.
    fn plus(&self, first: Folded, second: Folded) -> Folded {
        if second.is_zero() {
            return first;
        }
        if first.is_zero() {
            return second;
        }
        if let Folded::UMinus(inner) = second {
            return self.minus(first, *inner);
        }
        if let Folded::UMinus(inner) = first {
            return self.minus(second, *inner);
        }
        if let Folded::Minus(left, right) = &first {
            if **right == second {
                return (**left).clone();
            }
        }
        if let Folded::Minus(left, right) = &second {
            if **right == first {
                return (**left).clone();
            }
        }
        self.eval(&first, &second, BinOp::Add)
            .unwrap_or(Folded::Plus(Box::new(first), Box::new(second)))
    }

    /// Their `AddMinus`.
    fn minus(&self, first: Folded, second: Folded) -> Folded {
        if second.is_zero() {
            return first;
        }
        if first.is_zero() {
            return uminus(second);
        }
        if first == second {
            return self.zero();
        }
        if let Folded::UMinus(inner) = second {
            return self.plus(first, *inner);
        }
        if let Folded::Plus(left, right) = &first {
            if **right == second {
                return (**left).clone();
            }
            if **left == second {
                return (**right).clone();
            }
        }
        self.eval(&first, &second, BinOp::Sub)
            .unwrap_or(Folded::Minus(Box::new(first), Box::new(second)))
    }

    /// Their `AddTimes`.
    fn times(&self, first: Folded, second: Folded) -> Folded {
        if first.is_zero() || second.is_zero() {
            return self.zero();
        }
        if first.is_one() {
            return second;
        }
        if second.is_one() {
            return first;
        }
        if first.is_minus_one() {
            return uminus(second);
        }
        if second.is_minus_one() {
            return uminus(first);
        }
        if let Folded::Divide(left, right) = &first {
            if **right == second {
                return (**left).clone();
            }
        }
        if let Folded::Divide(left, right) = &second {
            if **right == first {
                return (**left).clone();
            }
        }
        self.eval(&first, &second, BinOp::Mul)
            .unwrap_or(Folded::Times(Box::new(first), Box::new(second)))
    }

    /// Their `AddDivide`. A denominator folding to their zero refuses the row
    /// while the tree is built; the shipped **E278** path reports that.
    fn divide(&self, first: Folded, second: Folded) -> Folded {
        if second.is_one() {
            return first;
        }
        if second.is_zero() {
            return Folded::Refused;
        }
        if first.is_zero() {
            return self.zero();
        }
        if first == second {
            return self.one();
        }
        if let Folded::Divide(left, right) = &second {
            if left.is_one() {
                return self.times(first, (**right).clone());
            }
        }
        if let Folded::Times(left, right) = &first {
            if **right == second {
                return (**left).clone();
            }
            if **left == second {
                return (**right).clone();
            }
        }
        self.eval(&first, &second, BinOp::Div)
            .unwrap_or(Folded::Divide(Box::new(first), Box::new(second)))
    }

    /// Their `AddPower`.
    fn power(&self, first: Folded, second: Folded) -> Folded {
        if second.is_zero() {
            return self.one();
        }
        if first.is_zero() {
            return self.zero();
        }
        if first.is_one() {
            return self.one();
        }
        if second.is_one() {
            return first;
        }
        self.eval(&first, &second, BinOp::Pow)
            .unwrap_or(Folded::Pow(Box::new(first), Box::new(second)))
    }

    /// Their constant reduction: both operands constant, so the node becomes one.
    fn eval(&self, first: &Folded, second: &Folded, op: BinOp) -> Option<Folded> {
        let (Folded::Num { value: a, .. }, Folded::Num { value: b, .. }) = (first, second) else {
            return None;
        };
        let value = match op {
            BinOp::Add => a + b,
            BinOp::Sub => a - b,
            BinOp::Mul => a * b,
            BinOp::Div => a / b,
            BinOp::Pow => a.powf(*b),
            _ => return None,
        };
        if !value.is_finite() {
            return None;
        }
        Some(if value < 0.0 {
            Folded::UMinus(Box::new(self.num_computed(-value)))
        } else {
            self.num_computed(value)
        })
    }

    fn compare(&self, op: BinOp, a: f64, b: f64) -> Folded {
        let hit = match op {
            BinOp::Lt => a < b,
            BinOp::Gt => a > b,
            BinOp::Le => a <= b,
            BinOp::Ge => a >= b,
            BinOp::EqEq => a == b,
            BinOp::Ne => a != b,
            _ => return Folded::BinaryOp,
        };
        if hit {
            self.one()
        } else {
            self.zero()
        }
    }

    fn zero(&self) -> Folded {
        Folded::Num {
            canon: "0".to_string(),
            value: 0.0,
            index: ZERO_INDEX,
        }
    }

    fn one(&self) -> Folded {
        Folded::Num {
            canon: "1".to_string(),
            value: 1.0,
            index: ONE_INDEX,
        }
    }
}

/// Parse-time symbol rules for ordinary expressions outside `model`.
/// Semi-structural discount and deterministic-trend expressions share this
/// grammar with moment option expressions.
pub(crate) fn outside_model_expression(model: &Model, id: ExprId) -> Option<Diagnostic> {
    let model_locals: HashSet<Name> = model
        .equations
        .iter()
        .filter(|eq| eq.is_local)
        .filter_map(|eq| match eq.lhs_expr.map(|id| &model.exprs.get(id).kind) {
            Some(ExprKind::Ident { name, .. }) => Some(*name),
            _ => None,
        })
        .collect();
    outside_model_expression_with_locals(model, id, &model_locals)
}

fn outside_model_expression_with_locals(
    model: &Model,
    id: ExprId,
    model_locals: &HashSet<Name>,
) -> Option<Diagnostic> {
    for ident in model.exprs.walk_idents(id) {
        let name = model.name(ident.name);
        let external = model.external_function_names.contains(&ident.name);
        if let Some(timing) = ident.timing_span {
            if external {
                // `helper(1)` is a call, not a lead.
                continue;
            }
            if symbol_exists_before(model, ident.name, ident.span.start) {
                return Some(err(
                    Span {
                        start: ident.span.start,
                        end: timing.end,
                    },
                    "E001",
                    format!(
                        "Using variable {name} with a lead or a lag is not allowed in this context"
                    ),
                ));
            }
            // A first use such as ghost(-1) is an ad hoc function call in an
            // ordinary expression. Dynare checks the value later, if needed.
            continue;
        }
        if external {
            return Some(err(
                ident.span,
                "E279",
                format!(
                    "Symbol '{name}' is the name of a MATLAB/Octave function, and cannot be used as a variable."
                ),
            ));
        }
        if model_locals.contains(&ident.name) {
            return Some(err(
                ident.span,
                "E282",
                format!(
                    "Variable {name} not allowed outside model declaration. Its scope is only inside model."
                ),
            ));
        }
        if model
            .trend_vars
            .iter()
            .any(|trend| trend.name == ident.name)
        {
            return Some(err(
                ident.span,
                "E310",
                format!(
                    "Variable {name} not allowed outside model declaration, because it is a trend variable."
                ),
            ));
        }
    }
    None
}

fn symbol_exists_before(model: &Model, name: Name, at: u32) -> bool {
    model
        .endogenous
        .iter()
        .chain(&model.exogenous)
        .chain(&model.deterministic_exogenous)
        .chain(&model.parameters)
        .chain(&model.model_local_variables)
        .chain(&model.excluded_endogenous)
        .any(|decl| decl.name == name && decl.span.start < at)
        || model
            .trend_vars
            .iter()
            .any(|trend| trend.name == name && trend.span.start < at)
        || model.equations.iter().any(|equation| {
            equation.is_local
                && equation.span.start < at
                && equation.lhs_expr.is_some_and(|lhs| {
                    matches!(&model.exprs.get(lhs).kind, ExprKind::Ident { name: local, .. } if *local == name)
                })
        })
        || (model.mod_file_locals.contains(&name)
            && model.exprs.iter().any(|(_, expr)| {
                matches!(&expr.kind, ExprKind::Ident { name: prior, .. } if *prior == name)
                    && expr.span.start < at
            }))
}

/// Their `AddUMinus`.
fn uminus(arg: Folded) -> Folded {
    if arg.is_zero() {
        return arg;
    }
    if let Folded::UMinus(inner) = arg {
        return *inner;
    }
    Folded::UMinus(Box::new(arg))
}

/// Their `double_to_string_lossless`. Rust's shortest round-trip form matches it
/// for the values arithmetic over written constants produces here.
fn lossless(value: f64) -> String {
    if value == value.trunc() && value.abs() < 1e15 {
        format!("{}", value as i64)
    } else {
        format!("{value}")
    }
}

/// Whether the expression carries a lead, which their `AddDiff` always expands.
fn has_lead(model: &Model, expr: ExprId) -> bool {
    model.exprs.walk_idents(expr).any(|ident| ident.timing > 0)
}

// ---------------------------------------------------------------------------
// The check pass
// ---------------------------------------------------------------------------

/// The four `method_of_moments` sentences, in their `checkPass` order. Their two
/// analytic flags are sticky across statements, so an earlier GMM statement
/// silences a later `analytic_*` one.
fn check_statements(ctx: &Ctx<'_>) -> Vec<Diagnostic> {
    let mut gmm = false;
    for stmt in &ctx.model.mom_statements {
        let method = stmt
            .options
            .iter()
            .find(|opt| opt.name.eq_ignore_ascii_case("mom_method"))
            .map(|opt| opt.value_text.clone());
        let Some(method) = method else {
            return vec![err(stmt.span, "E382", E382_MSG)];
        };
        if (method.eq_ignore_ascii_case("GMM") || method.eq_ignore_ascii_case("SMM"))
            && !has_option(stmt, "datafile")
        {
            return vec![err(stmt.span, "E383", E383_MSG)];
        }
        if method.eq_ignore_ascii_case("GMM") {
            gmm = true;
        }
        for option in ["analytic_standard_errors", "analytic_jacobian"] {
            if has_flag(stmt, option) && !gmm {
                return vec![err(
                    flag_span(stmt, option),
                    "E384",
                    format!("The {option} statement requires the GMM option."),
                )];
            }
        }
        let filters = ["hp_filter", "one_sided_hp_filter", "bandpass_filter"]
            .iter()
            .filter(|name| has_option(stmt, name))
            .count();
        if filters > 1 {
            return vec![err(stmt.span, "E385", E385_MSG)];
        }
    }
    Vec::new()
}

fn has_option(stmt: &MomStatement, name: &str) -> bool {
    stmt.options
        .iter()
        .any(|opt| opt.name.eq_ignore_ascii_case(name))
}

fn flag_span(stmt: &MomStatement, name: &str) -> Span {
    stmt.options
        .iter()
        .find(|opt| opt.name.eq_ignore_ascii_case(name))
        .map(|opt| opt.span)
        .unwrap_or(stmt.span)
}

/// Both `analytic_*` options are bare flags in the grammar.
fn has_flag(stmt: &MomStatement, name: &str) -> bool {
    stmt.options
        .iter()
        .any(|opt| opt.name.eq_ignore_ascii_case(name) && !opt.has_value)
}
