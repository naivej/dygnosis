//! Pinned Dynare 7.2 macro expression parser and evaluator.
//!
//! Grammar and values follow `preprocessor/src/macro` at `991946e9`.
//! Numbers are reals. Ranges materialize to arrays. `asin` is `atan`.

use std::collections::HashMap;

use crate::macro_expand::{MacroEvalError, MacroVal};

/// Elements in one range or array value.
const RANGE_CAP: usize = 10_000;
/// User-function recursion. `f()=f()` stops here.
const CALL_DEPTH_CAP: usize = 32;
/// Pratt-parser frames. One parenthesis uses about two frames.
const PARSE_DEPTH_CAP: u16 = 128;
/// Stored expression-tree depth. A child already at this depth is not boxed.
const TREE_DEPTH_CAP: u16 = 128;
/// Evaluation spine. The tree cap stops a deeper walk before evaluation.
const EXPR_DEPTH_CAP: usize = 128;
/// Stored value-tree depth. Distinct I211 name from expression depth.
pub(crate) const VALUE_DEPTH_CAP: usize = 128;
/// One set operation may compare at most this many pairs.
const SET_WORK_CAP: usize = RANGE_CAP * 8;
/// One macro string, checked before the allocation.
pub(crate) const STRING_CAP: usize = 1_048_576;
/// Operations in one root: nodes, iterations, set pairs, and allocated units.
pub(crate) const MACRO_WORK_CAP: usize = 1_000_000;
/// Cumulative emitted text for the execution worker's `spend_output` calls.
pub(crate) const MACRO_OUTPUT_CAP: usize = 8 * 1024 * 1024;
/// Nested loop bodies and included files. Checked before the recursive call.
pub(crate) const EXEC_DEPTH_CAP: usize = 64;

/// Shared per-root limit. Expression evaluation, loop iterations, include
/// recursion, and emitted text spend the same counters.
#[derive(Debug)]
pub(crate) struct MacroBudget {
    work_left: usize,
    output_left: usize,
    /// Nested `@#for` bodies and `@#include` files on this root.
    pub(crate) exec_depth: usize,
}

impl MacroBudget {
    pub(crate) fn new() -> Self {
        Self {
            work_left: MACRO_WORK_CAP,
            output_left: MACRO_OUTPUT_CAP,
            exec_depth: 0,
        }
    }

    /// Available work before the next file read, clone, or parse allocation.
    pub(crate) fn remaining_work(&self) -> usize {
        self.work_left
    }

    /// Count an operation. `amount` is the work about to be done.
    pub(crate) fn spend_work(&mut self, amount: usize) -> Result<(), MacroEvalError> {
        if amount > self.work_left {
            return Err(MacroEvalError::Limit("iteration work"));
        }
        self.work_left -= amount;
        Ok(())
    }

    /// Count emitted text. This does not count an internal value.
    pub(crate) fn spend_output(&mut self, bytes: usize) -> Result<(), MacroEvalError> {
        if bytes > self.output_left {
            return Err(MacroEvalError::Limit("output size"));
        }
        self.output_left -= bytes;
        Ok(())
    }

    fn reserve_string(&mut self, bytes: usize) -> Result<(), MacroEvalError> {
        if bytes > STRING_CAP {
            return Err(MacroEvalError::Limit("string size"));
        }
        self.spend_work(bytes.max(1))
    }
}

/// Pinned `ostringstream << setprecision(15)` (printf `%g`, 15 significant digits).
pub(crate) fn format_real(value: f64) -> String {
    if value.is_nan() {
        return "nan".into();
    }
    if value.is_infinite() {
        return if value.is_sign_negative() {
            "-inf".into()
        } else {
            "inf".into()
        };
    }
    if value == 0.0 {
        return if value.is_sign_negative() {
            "-0".into()
        } else {
            "0".into()
        };
    }
    let neg = value.is_sign_negative();
    let scientific = format!("{:.14e}", value.abs());
    let (digits, exp) = split_scientific(&scientific);
    let body = if !(-4..15).contains(&exp) {
        format!("{}e{exp:+03}", trim_significant(&digits))
    } else {
        fixed_significant(&digits, exp)
    };
    if neg {
        format!("-{body}")
    } else {
        body
    }
}

/// `(digits, exponent)` from Rust's `d.dddde±exp` scientific form.
fn split_scientific(scientific: &str) -> (Vec<char>, i32) {
    let (coef, exp) = scientific
        .split_once(['e', 'E'])
        .unwrap_or((scientific, "0"));
    let exp: i32 = exp.parse().unwrap_or(0);
    let mut digits: Vec<char> = coef.chars().filter(|ch| ch.is_ascii_digit()).collect();
    let whole = coef.find('.').unwrap_or(coef.len());
    if whole > 1 {
        // `10.0eN` is `1.0eN+1` after the extra integer digit.
        let shift = (whole - 1) as i32;
        return (digits, exp + shift);
    }
    if digits.is_empty() {
        digits.push('0');
    }
    (digits, exp)
}

fn trim_significant(digits: &[char]) -> String {
    let mut rest: String = digits.get(1..).unwrap_or_default().iter().collect();
    while rest.ends_with('0') {
        rest.pop();
    }
    let mut text = String::new();
    text.push(*digits.first().unwrap_or(&'0'));
    if !rest.is_empty() {
        text.push('.');
        text.push_str(&rest);
    }
    text
}

fn fixed_significant(digits: &[char], exp: i32) -> String {
    let digits = if digits.is_empty() {
        vec!['0']
    } else {
        digits.to_vec()
    };
    if exp >= 0 {
        let whole = (exp as usize) + 1;
        let mut text = String::new();
        for index in 0..whole {
            text.push(*digits.get(index).unwrap_or(&'0'));
        }
        let frac: String = digits.get(whole..).unwrap_or_default().iter().collect();
        let frac = frac.trim_end_matches('0');
        if !frac.is_empty() {
            text.push('.');
            text.push_str(frac);
        }
        return text;
    }
    let zeros = (-exp - 1) as usize;
    let mut text = String::from("0.");
    text.extend(std::iter::repeat_n('0', zeros));
    text.extend(digits.iter().copied());
    while text.ends_with('0') {
        text.pop();
    }
    if text.ends_with('.') {
        text.pop();
    }
    text
}

/// Compatibility printer. A limit panics; call [`render_macro_val_budget`]
/// with the root budget when the caller can propagate `MacroEvalError`.
#[allow(dead_code)]
pub(crate) fn interpolate(value: &MacroVal) -> String {
    render_fresh(value, RenderStyle::Interpolate)
}

/// `echomacrovars` printing. `matlab` selects the `(save)` quoting.
/// A limit panics; call [`render_macro_val_budget`] to propagate it.
#[allow(dead_code)]
pub(crate) fn print_value(value: &MacroVal, matlab: bool) -> String {
    render_fresh(value, RenderStyle::Print { matlab })
}

/// Printed length of [`print_value`]. Does not allocate the composite string
/// and does not spend.
#[allow(dead_code)]
pub(crate) fn plan_macro_render(value: &MacroVal, matlab: bool) -> Result<usize, MacroEvalError> {
    measure_val(value)?;
    Ok(render_len(value, RenderStyle::Print { matlab }))
}

/// `@{…}` and `@#echo` use `to_string`: strings inside arrays are not quoted.
pub(crate) fn render_interpolation_budget(
    value: &MacroVal,
    budget: &mut MacroBudget,
) -> Result<String, MacroEvalError> {
    render_charged(value, RenderStyle::Interpolate, budget, true)
}

/// Render `value` and charge the root budget before the string exists.
/// Output bytes are spent once. The caller does not spend them again.
pub(crate) fn render_macro_val_budget(
    value: &MacroVal,
    matlab: bool,
    budget: &mut MacroBudget,
) -> Result<String, MacroEvalError> {
    render_charged(value, RenderStyle::Print { matlab }, budget, true)
}

#[derive(Clone, Copy)]
enum RenderStyle {
    Interpolate,
    Print { matlab: bool },
}

fn render_fresh(value: &MacroVal, style: RenderStyle) -> String {
    match render_charged(value, style, &mut MacroBudget::new(), false) {
        Ok(text) => text,
        Err(MacroEvalError::Limit(name)) => panic!(
            "macro rendering hit the {name} limit; call render_macro_val_budget with the root budget"
        ),
        Err(error) => panic!("macro rendering failed: {error:?}"),
    }
}

fn render_stored(value: &MacroVal, budget: &mut MacroBudget) -> Result<String, MacroEvalError> {
    render_charged(value, RenderStyle::Interpolate, budget, false)
}

fn render_charged(
    value: &MacroVal,
    style: RenderStyle,
    budget: &mut MacroBudget,
    charge_output: bool,
) -> Result<String, MacroEvalError> {
    reject_unmapped_bytes(value)?;
    let cost = measure_val(value)?;
    let len = render_len(value, style);
    if !charge_output && len > STRING_CAP {
        return Err(MacroEvalError::Limit("string size"));
    }
    budget.spend_work(cost.nodes.max(1).saturating_add(len))?;
    if charge_output {
        budget.spend_output(len)?;
    }
    let mut out = String::with_capacity(len);
    render_walk(value, style, &mut |chunk| out.push_str(chunk));
    Ok(out)
}

fn render_len(value: &MacroVal, style: RenderStyle) -> usize {
    let mut len = 0usize;
    render_walk(value, style, &mut |chunk| {
        len = len.saturating_add(chunk.len());
    });
    len
}

enum RenderItem<'a> {
    Val(&'a MacroVal),
    Static(&'static str),
    Slice(&'a str),
    Owned(String),
}

fn render_walk(value: &MacroVal, style: RenderStyle, sink: &mut dyn FnMut(&str)) {
    let mut stack = vec![RenderItem::Val(value)];
    while let Some(item) = stack.pop() {
        match item {
            RenderItem::Static(text) => sink(text),
            RenderItem::Slice(text) => sink(text),
            RenderItem::Owned(text) => sink(&text),
            RenderItem::Val(value) => push_render(&mut stack, value, style),
        }
    }
}

fn push_render<'a>(stack: &mut Vec<RenderItem<'a>>, value: &'a MacroVal, style: RenderStyle) {
    match value {
        MacroVal::Bool(true) => stack.push(RenderItem::Static("true")),
        MacroVal::Bool(false) => stack.push(RenderItem::Static("false")),
        MacroVal::Int(number) => stack.push(RenderItem::Owned(format_real(*number as f64))),
        MacroVal::Real(number) => stack.push(RenderItem::Owned(format_real(*number))),
        MacroVal::Bytes(_) => {}
        MacroVal::Text(text) => match style {
            RenderStyle::Interpolate => stack.push(RenderItem::Slice(text)),
            RenderStyle::Print { matlab } => {
                let quote = if matlab { "'" } else { "\"" };
                stack.push(RenderItem::Static(quote));
                stack.push(RenderItem::Slice(text));
                stack.push(RenderItem::Static(quote));
            }
        },
        MacroVal::Array(items) => {
            let (open, close) = match style {
                RenderStyle::Print { matlab: true } => ("{", "}"),
                _ => ("[", "]"),
            };
            push_render_list(stack, items, open, close);
        }
        MacroVal::Tuple(items) => {
            let (open, close) = match style {
                RenderStyle::Print { matlab: true } => ("{", "}"),
                _ => ("(", ")"),
            };
            push_render_list(stack, items, open, close);
        }
        MacroVal::Function { .. } | MacroVal::Unresolved => {}
    }
}

fn push_render_list<'a>(
    stack: &mut Vec<RenderItem<'a>>,
    items: &'a [MacroVal],
    open: &'static str,
    close: &'static str,
) {
    stack.push(RenderItem::Static(close));
    if let Some((last, rest)) = items.split_last() {
        stack.push(RenderItem::Val(last));
        for item in rest.iter().rev() {
            stack.push(RenderItem::Static(", "));
            stack.push(RenderItem::Val(item));
        }
    }
    stack.push(RenderItem::Static(open));
}

struct ValCost {
    nodes: usize,
    bytes: usize,
    function_params: usize,
}

/// Heap walk. Rejects an over-deep or oversized value before any clone.
fn measure_val(value: &MacroVal) -> Result<ValCost, MacroEvalError> {
    let mut stack = vec![(value, 1usize)];
    let mut nodes = 0usize;
    let mut bytes = 0usize;
    let mut function_params = 0usize;
    while let Some((value, depth)) = stack.pop() {
        if depth > VALUE_DEPTH_CAP {
            return Err(MacroEvalError::Limit("value depth"));
        }
        nodes = nodes.saturating_add(1);
        if nodes > MACRO_WORK_CAP {
            return Err(MacroEvalError::Limit("iteration work"));
        }
        match value {
            MacroVal::Int(_) | MacroVal::Real(_) | MacroVal::Bool(_) | MacroVal::Unresolved => {}
            MacroVal::Text(text) => {
                bytes = bytes.saturating_add(text.len());
                if bytes > STRING_CAP {
                    return Err(MacroEvalError::Limit("string size"));
                }
            }
            MacroVal::Bytes(text) => {
                bytes = bytes.saturating_add(text.len());
                if bytes > STRING_CAP {
                    return Err(MacroEvalError::Limit("string size"));
                }
            }
            MacroVal::Function { params, body } => {
                function_params = function_params.saturating_add(params.len());
                bytes = bytes.saturating_add(body.len());
                for param in params {
                    bytes = bytes.saturating_add(param.len());
                }
                if bytes > STRING_CAP {
                    return Err(MacroEvalError::Limit("string size"));
                }
            }
            MacroVal::Array(items) | MacroVal::Tuple(items) => {
                if items.len() > RANGE_CAP {
                    return Err(MacroEvalError::Limit("collection size"));
                }
                let next = depth.saturating_add(1);
                for item in items {
                    stack.push((item, next));
                }
            }
        }
    }
    Ok(ValCost {
        nodes,
        bytes,
        function_params,
    })
}

/// Copy a stored value only after depth, bytes, and work have been accepted.
pub(crate) fn clone_macro_val(
    value: &MacroVal,
    budget: &mut MacroBudget,
) -> Result<MacroVal, MacroEvalError> {
    let cost = measure_val(value)?;
    budget.spend_work(cost.nodes.max(1).saturating_add(cost.bytes))?;
    Ok(value.clone())
}

/// Private occurrence payload. Flat preorder avoids JSON's nested-value depth
/// limit and keeps exact real bits separate from the public 15-digit printer.
#[derive(serde::Serialize, serde::Deserialize)]
enum ReplayAtom {
    RealBits(u64),
    Int(i64),
    Bool(bool),
    Text(String),
    Bytes(Vec<u8>),
    Tuple(usize),
    Array(usize),
    Function { params: Vec<String>, body: String },
    Unresolved,
}

pub(crate) fn encode_replay_value(
    value: &MacroVal,
    budget: &mut MacroBudget,
) -> Result<String, MacroEvalError> {
    let cost = measure_val(value)?;
    let function_params = cost.function_params;
    // JSON can escape each input byte as six bytes. The fixed part covers tag,
    // punctuation, count/bits digits, and each function-parameter separator.
    let encoded_bound = cost
        .bytes
        .saturating_mul(6)
        .saturating_add(cost.nodes.saturating_mul(64))
        .saturating_add(function_params.saturating_mul(4))
        .saturating_add(2);
    let allocation =
        encoded_bound
            .saturating_add(cost.bytes)
            .saturating_add(cost.nodes.saturating_mul(
                std::mem::size_of::<ReplayAtom>() + std::mem::size_of::<&MacroVal>(),
            ))
            .saturating_add(function_params.saturating_mul(std::mem::size_of::<String>()));
    budget.spend_work(allocation.saturating_add(cost.nodes))?;
    let mut atoms = Vec::with_capacity(cost.nodes);
    let mut pending = vec![value];
    while let Some(value) = pending.pop() {
        atoms.push(match value {
            MacroVal::Real(number) => ReplayAtom::RealBits(number.to_bits()),
            MacroVal::Int(number) => ReplayAtom::Int(*number),
            MacroVal::Bool(value) => ReplayAtom::Bool(*value),
            MacroVal::Text(value) => ReplayAtom::Text(value.clone()),
            MacroVal::Bytes(value) => ReplayAtom::Bytes(value.clone()),
            MacroVal::Tuple(items) => {
                pending.extend(items.iter().rev());
                ReplayAtom::Tuple(items.len())
            }
            MacroVal::Array(items) => {
                pending.extend(items.iter().rev());
                ReplayAtom::Array(items.len())
            }
            MacroVal::Function { params, body } => ReplayAtom::Function {
                params: params.clone(),
                body: body.clone(),
            },
            MacroVal::Unresolved => ReplayAtom::Unresolved,
        });
    }
    let encoded =
        serde_json::to_string(&atoms).map_err(|_| MacroEvalError::Limit("source mapping"))?;
    debug_assert!(encoded.len() <= encoded_bound);
    Ok(encoded)
}

pub(crate) fn decode_replay_value(
    encoded: &str,
    budget: &mut MacroBudget,
) -> Result<MacroVal, MacroEvalError> {
    // Each atom has at least eight fixed JSON bytes; each function parameter
    // has at least three quote/separator bytes. These distinct parts fund both
    // geometrically growing vectors. Three extra units cover strings and the
    // parser's scratch buffer. Fixed slack covers minimum small-Vec capacities.
    let per_byte = (2 * std::mem::size_of::<ReplayAtom>())
        .div_ceil(8)
        .max((2 * std::mem::size_of::<String>()).div_ceil(3))
        + 3;
    let parse_bound = encoded.len().saturating_mul(per_byte).saturating_add(
        4 * std::mem::size_of::<ReplayAtom>() + 4 * std::mem::size_of::<String>() + 8,
    );
    budget.spend_work(parse_bound)?;
    let atoms: Vec<ReplayAtom> =
        serde_json::from_str(encoded).map_err(|_| MacroEvalError::Limit("source mapping"))?;
    if atoms.is_empty() || atoms.len() > MACRO_WORK_CAP {
        return Err(MacroEvalError::Limit("source mapping"));
    }
    budget.spend_work(
        atoms
            .len()
            .saturating_mul(std::mem::size_of::<(MacroVal, usize)>())
            .saturating_add(atoms.len()),
    )?;
    let mut values: Vec<(MacroVal, usize)> = Vec::with_capacity(atoms.len());
    for atom in atoms.into_iter().rev() {
        let (value, depth) = match atom {
            ReplayAtom::RealBits(bits) => (MacroVal::Real(f64::from_bits(bits)), 1),
            ReplayAtom::Int(number) => (MacroVal::Int(number), 1),
            ReplayAtom::Bool(value) => (MacroVal::Bool(value), 1),
            ReplayAtom::Text(value) => (MacroVal::Text(value), 1),
            ReplayAtom::Bytes(value) => (MacroVal::Bytes(value), 1),
            ReplayAtom::Function { params, body } => (MacroVal::Function { params, body }, 1),
            ReplayAtom::Unresolved => (MacroVal::Unresolved, 1),
            ReplayAtom::Tuple(count) | ReplayAtom::Array(count) => {
                if count > RANGE_CAP || count > values.len() {
                    return Err(MacroEvalError::Limit("source mapping"));
                }
                let depth = values[values.len() - count..]
                    .iter()
                    .map(|(_, depth)| *depth)
                    .max()
                    .unwrap_or(0)
                    .saturating_add(1);
                if depth > VALUE_DEPTH_CAP {
                    return Err(MacroEvalError::Limit("value depth"));
                }
                budget.spend_work(
                    count
                        .saturating_mul(std::mem::size_of::<MacroVal>())
                        .saturating_add(1),
                )?;
                let tuple = matches!(atom, ReplayAtom::Tuple(_));
                let mut items = Vec::with_capacity(count);
                for _ in 0..count {
                    items.push(
                        values
                            .pop()
                            .ok_or(MacroEvalError::Limit("source mapping"))?
                            .0,
                    );
                }
                (
                    if tuple {
                        MacroVal::Tuple(items)
                    } else {
                        MacroVal::Array(items)
                    },
                    depth,
                )
            }
        };
        values.push((value, depth));
    }
    if values.len() != 1 {
        return Err(MacroEvalError::Limit("source mapping"));
    }
    let value = values
        .pop()
        .ok_or(MacroEvalError::Limit("source mapping"))?
        .0;
    measure_val(&value)?;
    Ok(value)
}

fn reject_unmapped_bytes(value: &MacroVal) -> Result<(), MacroEvalError> {
    let mut stack = vec![value];
    while let Some(value) = stack.pop() {
        match value {
            MacroVal::Bytes(_) => return Err(MacroEvalError::Limit("non-UTF-8 byte slice")),
            MacroVal::Array(items) | MacroVal::Tuple(items) => stack.extend(items.iter()),
            _ => {}
        }
    }
    Ok(())
}

fn text_from_bytes(bytes: Vec<u8>) -> MacroVal {
    match String::from_utf8(bytes) {
        Ok(text) => MacroVal::Text(text),
        Err(error) => MacroVal::Bytes(error.into_bytes()),
    }
}

fn seal_value(value: MacroVal) -> Result<MacroVal, MacroEvalError> {
    measure_val(&value)?;
    Ok(value)
}

/// Compatibility wrapper. Expansion passes one root budget to
/// [`eval_macro_expr_budget`]. This entry remains for a caller that has not
/// moved yet.
#[allow(dead_code)]
pub(crate) fn eval_macro_expr(
    source: &str,
    defines: &mut HashMap<String, MacroVal>,
) -> Result<MacroVal, MacroEvalError> {
    let mut budget = MacroBudget::new();
    eval_macro_expr_budget(source, defines, &mut budget)
}

pub(crate) fn eval_macro_expr_budget(
    source: &str,
    defines: &mut HashMap<String, MacroVal>,
    budget: &mut MacroBudget,
) -> Result<MacroVal, MacroEvalError> {
    let source = prepare_source(source, budget)?;
    let expr = parse_expr(&source)?;
    eval_expr(&expr, defines, None, 0, 0, budget)
}

/// Interpolation uses the tokenizer's `eval` state: newlines are whitespace,
/// while `//` and directive continuations remain expression tokens.
pub(crate) fn eval_macro_expr_interpolation_budget(
    source: &str,
    defines: &mut HashMap<String, MacroVal>,
    budget: &mut MacroBudget,
) -> Result<MacroVal, MacroEvalError> {
    charge_source(budget, source)?;
    let expr = parse_expr_context(source, ExprContext::Interpolation)?;
    eval_expr(&expr, defines, None, 0, 0, budget)
}

#[allow(dead_code)]
pub(crate) fn check_macro_syntax(source: &str) -> Result<(), MacroEvalError> {
    check_macro_syntax_budget(source, &mut MacroBudget::new())
}

pub(crate) fn check_macro_syntax_budget(
    source: &str,
    budget: &mut MacroBudget,
) -> Result<(), MacroEvalError> {
    let source = prepare_source(source, budget)?;
    parse_expr(&source).map(|_| ())
}

pub(crate) fn check_macro_syntax_interpolation_budget(
    source: &str,
    budget: &mut MacroBudget,
) -> Result<(), MacroEvalError> {
    charge_source(budget, source)?;
    parse_expr_context(source, ExprContext::Interpolation).map(|_| ())
}

/// `ifdef` and `ifndef` inspect a written variable expression's name.
pub(crate) fn macro_condition_variable_budget(
    source: &str,
    budget: &mut MacroBudget,
) -> Result<Option<String>, MacroEvalError> {
    let source = prepare_source(source, budget)?;
    Ok(variable_name(&parse_expr(&source)?).map(str::to_owned))
}

/// Validate the pinned `expr IN expr [WHEN expr]` loop header before execution.
pub(crate) fn check_macro_for_header_budget(
    source: &str,
    budget: &mut MacroBudget,
) -> Result<Vec<String>, MacroEvalError> {
    let source = prepare_source(source, budget)?;
    let mut lexer = Lexer::new(&source, ExprContext::Directive);
    let vars = lexer.parse_bp_with_colon(51)?;
    if !lexer.eat("in") {
        return Err(lexer.or_fail(MacroEvalError::SyntaxUnexpected(lexer.peek.name())));
    }
    lexer.parse_bp_with_colon(0)?;
    if lexer.eat("when") {
        lexer.parse_bp_with_colon(0)?;
    }
    lexer.take_fail()?;
    if lexer.peek != Tok::End {
        return Err(MacroEvalError::SyntaxUnexpected(lexer.peek.name()));
    }
    if let Some(name) = variable_name(&vars) {
        return Ok(vec![name.to_owned()]);
    }
    let Expr::Tuple(items) = vars else {
        return Err(official(
            "E062",
            "For loop indices must be a variable or a tuple",
        ));
    };
    items
        .iter()
        .map(|item| {
            variable_name(item)
                .map(str::to_owned)
                .ok_or_else(|| official("E062", "For loop indices must be variables"))
        })
        .collect()
}

/// Validate a macro variable or function definition without evaluating it.
pub(crate) fn check_macro_definition_budget(
    source: &str,
    budget: &mut MacroBudget,
) -> Result<(), MacroEvalError> {
    let source = prepare_source(source, budget)?;
    let mut lexer = Lexer::new(&source, ExprContext::Directive);
    if !matches!(lexer.peek, Tok::Ident(_)) {
        return Err(MacroEvalError::SyntaxUnexpected(lexer.peek.name()));
    }
    lexer.bump();
    let function = lexer.eat("(");
    if function && !lexer.eat(")") {
        loop {
            if !matches!(lexer.peek, Tok::Ident(_)) {
                return Err(MacroEvalError::SyntaxUnexpected(lexer.peek.name()));
            }
            lexer.bump();
            if lexer.eat(")") {
                break;
            }
            if !lexer.eat(",") {
                return Err(MacroEvalError::SyntaxUnexpected(lexer.peek.name()));
            }
        }
    }
    if lexer.eat("=") {
        lexer.parse_bp_with_colon(0)?;
    } else if function || lexer.peek != Tok::End {
        return Err(MacroEvalError::SyntaxUnexpected(lexer.peek.name()));
    }
    lexer.take_fail()?;
    if lexer.peek != Tok::End {
        return Err(MacroEvalError::SyntaxUnexpected(lexer.peek.name()));
    }
    Ok(())
}

/// Print a function body the way `Environment::print` prints an expression.
#[allow(dead_code)]
pub(crate) fn print_expression(source: &str) -> Result<String, MacroEvalError> {
    print_expression_budget(source, &mut MacroBudget::new())
}

pub(crate) fn print_expression_budget(
    source: &str,
    budget: &mut MacroBudget,
) -> Result<String, MacroEvalError> {
    let source = prepare_source(source, budget)?;
    let expr = parse_expr(&source)?;
    let mut len = 0usize;
    print_walk(&expr, &mut |chunk| len = len.saturating_add(chunk.len()));
    if len > STRING_CAP {
        return Err(MacroEvalError::Limit("string size"));
    }
    budget.spend_work(len.max(1))?;
    let mut out = String::with_capacity(len);
    print_walk(&expr, &mut |chunk| out.push_str(chunk));
    Ok(out)
}

fn charge_source(budget: &mut MacroBudget, source: &str) -> Result<(), MacroEvalError> {
    if source.len() > STRING_CAP {
        return Err(MacroEvalError::Limit("string size"));
    }
    budget.spend_work(source.len().max(1))
}

fn prepare_source(source: &str, budget: &mut MacroBudget) -> Result<String, MacroEvalError> {
    charge_source(budget, source)?;
    Ok(unfold_continuations(source))
}

pub(crate) fn unfold_continuations(source: &str) -> String {
    let bytes = source.as_bytes();
    let mut out = String::with_capacity(source.len());
    let mut i = 0;
    let mut quoted = false;
    while i < bytes.len() {
        if bytes[i] == b'"' {
            quoted = !quoted;
        }
        if !quoted && bytes[i] == b'\\' && i + 1 < bytes.len() && bytes[i + 1] == b'\\' {
            let mut j = i + 2;
            while j < bytes.len() && (bytes[j] == b' ' || bytes[j] == b'\t') {
                j += 1;
            }
            let mut k = j;
            if k + 1 < bytes.len() && bytes[k] == b'/' && bytes[k + 1] == b'/' {
                while k < bytes.len() && bytes[k] != b'\n' && bytes[k] != b'\r' {
                    k += 1;
                }
                j = k;
            }
            if j < bytes.len() && (bytes[j] == b'\n' || bytes[j] == b'\r') {
                if bytes[j] == b'\r' && j + 1 < bytes.len() && bytes[j + 1] == b'\n' {
                    j += 2;
                } else {
                    j += 1;
                }
                out.push(' ');
                i = j;
                continue;
            }
        }
        let ch = source[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

#[derive(Clone, Debug)]
enum Expr {
    Bool(bool),
    Real(f64),
    Text(String),
    Var(String),
    Index {
        name: String,
        indexes: Vec<Expr>,
    },
    Call {
        name: String,
        args: Vec<Expr>,
    },
    Builtin(Builtin, Vec<Expr>),
    Defined(String),
    Array(Vec<Expr>),
    Tuple(Vec<Expr>),
    Range {
        start: Box<Expr>,
        step: Option<Box<Expr>>,
        end: Box<Expr>,
    },
    Unary(Unary, Box<Expr>),
    Binary(BinOp, Box<Expr>, Box<Expr>),
    Filter {
        vars: Box<Expr>,
        set: Box<Expr>,
        when: Box<Expr>,
    },
    Map {
        expr: Box<Expr>,
        vars: Box<Expr>,
        set: Box<Expr>,
        when: Option<Box<Expr>>,
    },
}

#[derive(Clone, Copy, Debug)]
enum Unary {
    Not,
    Neg,
    Pos,
    CastBool,
    CastReal,
    CastString,
    CastTuple,
    CastArray,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Pow,
    Eq,
    Ne,
    Lt,
    Gt,
    Le,
    Ge,
    And,
    Or,
    In,
    Union,
    Inter,
}

#[derive(Clone, Copy, Debug)]
enum Builtin {
    Length,
    IsEmpty,
    IsBoolean,
    IsReal,
    IsString,
    IsTuple,
    IsArray,
    Exp,
    Ln,
    Log10,
    Sin,
    Cos,
    Tan,
    Atan,
    Acos,
    Sqrt,
    Cbrt,
    Sign,
    Floor,
    Ceil,
    Trunc,
    Sum,
    Erf,
    Erfc,
    Gamma,
    Lgamma,
    Round,
    Normpdf,
    Normcdf,
    Max,
    Min,
    Mod,
    Normpdf3,
    Normcdf3,
}

fn expr_depth(expr: &Expr) -> u16 {
    let mut stack = vec![(expr, 1u16)];
    let mut max_depth = 1u16;
    while let Some((expr, depth)) = stack.pop() {
        if depth > max_depth {
            max_depth = depth;
        }
        if max_depth >= TREE_DEPTH_CAP {
            return TREE_DEPTH_CAP;
        }
        let next = depth.saturating_add(1);
        match expr {
            Expr::Unary(_, inner) => stack.push((inner, next)),
            Expr::Binary(_, left, right) => {
                stack.push((left, next));
                stack.push((right, next));
            }
            Expr::Range { start, step, end } => {
                stack.push((start, next));
                if let Some(step) = step {
                    stack.push((step, next));
                }
                stack.push((end, next));
            }
            Expr::Filter { vars, set, when } => {
                stack.push((vars, next));
                stack.push((set, next));
                stack.push((when, next));
            }
            Expr::Map {
                expr,
                vars,
                set,
                when,
            } => {
                stack.push((expr, next));
                stack.push((vars, next));
                stack.push((set, next));
                if let Some(when) = when {
                    stack.push((when, next));
                }
            }
            Expr::Array(items) | Expr::Tuple(items) | Expr::Builtin(_, items) => {
                for item in items {
                    stack.push((item, next));
                }
            }
            Expr::Call { args, .. } | Expr::Index { indexes: args, .. } => {
                for arg in args {
                    stack.push((arg, next));
                }
            }
            Expr::Bool(_) | Expr::Real(_) | Expr::Text(_) | Expr::Var(_) | Expr::Defined(_) => {}
        }
    }
    max_depth
}

fn fit1(expr: &Expr) -> Result<(), MacroEvalError> {
    if expr_depth(expr) >= TREE_DEPTH_CAP {
        Err(MacroEvalError::Limit("expression depth"))
    } else {
        Ok(())
    }
}

fn fit2(left: &Expr, right: &Expr) -> Result<(), MacroEvalError> {
    fit1(left)?;
    fit1(right)
}

fn fit_all(exprs: &[Expr]) -> Result<(), MacroEvalError> {
    for expr in exprs {
        fit1(expr)?;
    }
    Ok(())
}

fn box_expr(expr: Expr) -> Result<Box<Expr>, MacroEvalError> {
    fit1(&expr)?;
    Ok(Box::new(expr))
}

fn parse_expr(source: &str) -> Result<Expr, MacroEvalError> {
    parse_expr_context(source, ExprContext::Directive)
}

fn variable_name(expr: &Expr) -> Option<&str> {
    match expr {
        Expr::Var(name) | Expr::Index { name, .. } => Some(name),
        _ => None,
    }
}

#[derive(Clone, Copy)]
enum ExprContext {
    Directive,
    Interpolation,
}

fn parse_expr_context(source: &str, context: ExprContext) -> Result<Expr, MacroEvalError> {
    let mut lexer = Lexer::new(source, context);
    if lexer.peek == Tok::End {
        return Err(lexer.or_fail(MacroEvalError::SyntaxEol));
    }
    let expr = lexer.parse_bp_with_colon(0)?;
    lexer.take_fail()?;
    if lexer.peek != Tok::End {
        return Err(MacroEvalError::SyntaxUnexpected(lexer.peek.name()));
    }
    Ok(expr)
}

struct Lexer<'a> {
    src: &'a str,
    context: ExprContext,
    pos: usize,
    peek: Tok,
    depth: u16,
    fail: Option<MacroEvalError>,
}

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    End,
    Num(f64),
    Str(String),
    Ident(String),
    Word(&'static str, &'static str),
}

impl Tok {
    fn name(&self) -> &'static str {
        match self {
            Tok::End => "EOL",
            Tok::Num(_) => "NUMBER",
            Tok::Str(_) => "QUOTED_STRING",
            Tok::Ident(_) => "IDENTIFIER",
            Tok::Word(_, name) => name,
        }
    }
}

impl<'a> Lexer<'a> {
    fn new(src: &'a str, context: ExprContext) -> Self {
        let mut lexer = Self {
            src,
            context,
            pos: 0,
            peek: Tok::End,
            depth: 0,
            fail: None,
        };
        lexer.bump();
        lexer
    }

    fn take_fail(&mut self) -> Result<(), MacroEvalError> {
        match self.fail.take() {
            Some(err) => Err(err),
            None => Ok(()),
        }
    }

    fn or_fail(&mut self, err: MacroEvalError) -> MacroEvalError {
        self.fail.take().unwrap_or(err)
    }

    fn enter(&mut self) -> Result<(), MacroEvalError> {
        self.take_fail()?;
        if self.depth >= PARSE_DEPTH_CAP {
            return Err(MacroEvalError::Limit("expression depth"));
        }
        self.depth += 1;
        Ok(())
    }

    fn bump(&mut self) {
        self.skip();
        if self.pos >= self.src.len() {
            self.peek = Tok::End;
            return;
        }
        let rest = &self.src[self.pos..];
        let bytes = rest.as_bytes();
        if bytes[0] == b'"' {
            if let Some(end) = rest[1..].find('"') {
                if end > STRING_CAP {
                    self.fail = Some(MacroEvalError::Limit("string size"));
                    self.peek = Tok::End;
                    return;
                }
                self.peek = Tok::Str(rest[1..1 + end].to_string());
                self.pos += end + 2;
            } else {
                self.peek = Tok::Word("text", "TEXT");
                self.pos += 1;
            }
            return;
        }
        if let Some(word) = keyword(rest) {
            self.peek = Tok::Word(word.0, word.1);
            self.pos += word.0.len();
            return;
        }
        if is_number_start(rest) {
            let (value, len) = scan_number(rest);
            self.peek = Tok::Num(value);
            self.pos += len;
            return;
        }
        if bytes[0].is_ascii_alphabetic() || bytes[0] == b'_' {
            let len = ident_len(rest);
            self.peek = Tok::Ident(rest[..len].to_string());
            self.pos += len;
            return;
        }
        let ch = rest.chars().next().unwrap();
        self.peek = Tok::Word("text", "TEXT");
        self.pos += ch.len_utf8();
    }

    fn skip(&mut self) {
        loop {
            let rest = &self.src[self.pos..];
            if rest.is_empty() {
                return;
            }
            if matches!(self.context, ExprContext::Directive) && rest.starts_with("//") {
                if let Some(end) = rest.find(['\n', '\r']) {
                    self.pos += end;
                } else {
                    self.pos = self.src.len();
                }
                continue;
            }
            if rest.starts_with([' ', '\t']) {
                self.pos += 1;
                continue;
            }
            if matches!(self.context, ExprContext::Interpolation) {
                if rest.starts_with("\r\n") {
                    self.pos += 2;
                    continue;
                }
                if rest.starts_with('\n') {
                    self.pos += 1;
                    continue;
                }
            }
            if matches!(self.context, ExprContext::Directive) && rest == "\n" {
                self.pos += 1;
                continue;
            }
            if matches!(self.context, ExprContext::Directive) && rest == "\r\n" {
                self.pos += 2;
                continue;
            }
            return;
        }
    }

    fn eat(&mut self, spelling: &str) -> bool {
        match &self.peek {
            Tok::Word(text, _) if *text == spelling => {
                self.bump();
                true
            }
            _ => false,
        }
    }

    fn parse_prefix(&mut self) -> Result<Expr, MacroEvalError> {
        self.take_fail()?;
        if self.eat("!") {
            let inner = self.parse_bp_with_colon(111)?;
            return Ok(Expr::Unary(Unary::Not, box_expr(inner)?));
        }
        if self.eat("+") {
            let inner = self.parse_bp_with_colon(111)?;
            return Ok(Expr::Unary(Unary::Pos, box_expr(inner)?));
        }
        if self.eat("-") {
            let inner = self.parse_bp_with_colon(111)?;
            return Ok(Expr::Unary(Unary::Neg, box_expr(inner)?));
        }
        let mut left = self.parse_primary()?;
        if self.eat("^") {
            if self.peek == Tok::End {
                return Err(MacroEvalError::SyntaxUnexpected("EOL"));
            }
            let right = self.parse_tight()?;
            if matches!(&self.peek, Tok::Word("^", _)) {
                return Err(MacroEvalError::SyntaxUnexpected("POWER"));
            }
            fit2(&left, &right)?;
            left = Expr::Binary(BinOp::Pow, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_tight(&mut self) -> Result<Expr, MacroEvalError> {
        self.enter()?;
        let result = self.parse_tight_at();
        self.depth -= 1;
        result
    }

    fn parse_tight_at(&mut self) -> Result<Expr, MacroEvalError> {
        if self.eat("!") {
            let inner = self.parse_tight()?;
            return Ok(Expr::Unary(Unary::Not, box_expr(inner)?));
        }
        if self.eat("+") {
            let inner = self.parse_tight()?;
            return Ok(Expr::Unary(Unary::Pos, box_expr(inner)?));
        }
        if self.eat("-") {
            let inner = self.parse_tight()?;
            return Ok(Expr::Unary(Unary::Neg, box_expr(inner)?));
        }
        self.parse_primary()
    }

    fn parse_primary(&mut self) -> Result<Expr, MacroEvalError> {
        match self.peek.clone() {
            Tok::Word("(", "LPAREN") => self.parse_paren(),
            Tok::Word("[", "LBRACKET") => self.parse_bracket(),
            Tok::Word("true", "TRUE") => {
                self.bump();
                Ok(Expr::Bool(true))
            }
            Tok::Word("false", "FALSE") => {
                self.bump();
                Ok(Expr::Bool(false))
            }
            Tok::Num(n) => {
                self.bump();
                Ok(Expr::Real(n))
            }
            Tok::Str(s) => {
                self.bump();
                Ok(Expr::Text(s))
            }
            Tok::Ident(name) => {
                self.bump();
                if self.eat("(") {
                    let args = self.parse_commas(")", true)?;
                    fit_all(&args)?;
                    return Ok(Expr::Call { name, args });
                }
                if self.eat("[") {
                    let indexes = self.parse_commas("]", true)?;
                    fit_all(&indexes)?;
                    return Ok(Expr::Index { name, indexes });
                }
                Ok(Expr::Var(name))
            }
            Tok::Word("defined", _) => {
                self.bump();
                if !self.eat("(") {
                    return Err(self.or_fail(MacroEvalError::SyntaxUnexpected(self.peek.name())));
                }
                let Tok::Ident(name) = self.peek.clone() else {
                    return Err(self.or_fail(MacroEvalError::SyntaxUnexpected(self.peek.name())));
                };
                self.bump();
                if !self.eat(")") {
                    return Err(self.or_fail(MacroEvalError::SyntaxUnexpected(self.peek.name())));
                }
                Ok(Expr::Defined(name))
            }
            Tok::Word(_, _) if self.builtin_ahead() => self.parse_builtin(),
            Tok::End => Err(self.or_fail(MacroEvalError::SyntaxEol)),
            other => Err(MacroEvalError::SyntaxUnexpected(other.name())),
        }
    }

    fn builtin_ahead(&self) -> bool {
        matches!(
            &self.peek,
            Tok::Word(
                "length"
                    | "isempty"
                    | "isboolean"
                    | "isreal"
                    | "isstring"
                    | "istuple"
                    | "isarray"
                    | "exp"
                    | "log"
                    | "ln"
                    | "log10"
                    | "sin"
                    | "cos"
                    | "tan"
                    | "asin"
                    | "acos"
                    | "atan"
                    | "sqrt"
                    | "cbrt"
                    | "sign"
                    | "floor"
                    | "ceil"
                    | "trunc"
                    | "sum"
                    | "erf"
                    | "erfc"
                    | "gamma"
                    | "lgamma"
                    | "round"
                    | "normpdf"
                    | "normcdf"
                    | "max"
                    | "min"
                    | "mod",
                _
            )
        )
    }

    fn parse_builtin(&mut self) -> Result<Expr, MacroEvalError> {
        let spelling = match &self.peek {
            Tok::Word(text, _) => *text,
            _ => return Err(self.or_fail(MacroEvalError::SyntaxUnexpected(self.peek.name()))),
        };
        self.bump();
        if !self.eat("(") {
            return Err(self.or_fail(MacroEvalError::SyntaxUnexpected(self.peek.name())));
        }
        let args = self.parse_commas(")", false)?;
        let builtin = match (spelling, args.len()) {
            ("length", 1) => Builtin::Length,
            ("isempty", 1) => Builtin::IsEmpty,
            ("isboolean", 1) => Builtin::IsBoolean,
            ("isreal", 1) => Builtin::IsReal,
            ("isstring", 1) => Builtin::IsString,
            ("istuple", 1) => Builtin::IsTuple,
            ("isarray", 1) => Builtin::IsArray,
            ("exp", 1) => Builtin::Exp,
            ("log" | "ln", 1) => Builtin::Ln,
            ("log10", 1) => Builtin::Log10,
            ("sin", 1) => Builtin::Sin,
            ("cos", 1) => Builtin::Cos,
            ("tan", 1) => Builtin::Tan,
            ("asin" | "atan", 1) => Builtin::Atan,
            ("acos", 1) => Builtin::Acos,
            ("sqrt", 1) => Builtin::Sqrt,
            ("cbrt", 1) => Builtin::Cbrt,
            ("sign", 1) => Builtin::Sign,
            ("floor", 1) => Builtin::Floor,
            ("ceil", 1) => Builtin::Ceil,
            ("trunc", 1) => Builtin::Trunc,
            ("sum", 1) => Builtin::Sum,
            ("erf", 1) => Builtin::Erf,
            ("erfc", 1) => Builtin::Erfc,
            ("gamma", 1) => Builtin::Gamma,
            ("lgamma", 1) => Builtin::Lgamma,
            ("round", 1) => Builtin::Round,
            ("normpdf", 1) => Builtin::Normpdf,
            ("normcdf", 1) => Builtin::Normcdf,
            ("max", 2) => Builtin::Max,
            ("min", 2) => Builtin::Min,
            ("mod", 2) => Builtin::Mod,
            ("normpdf", 3) => Builtin::Normpdf3,
            ("normcdf", 3) => Builtin::Normcdf3,
            _ => {
                return Err(MacroEvalError::SyntaxUnexpected(if args.is_empty() {
                    "RPAREN"
                } else {
                    "COMMA"
                }));
            }
        };
        fit_all(&args)?;
        Ok(Expr::Builtin(builtin, args))
    }

    fn parse_paren(&mut self) -> Result<Expr, MacroEvalError> {
        self.bump();
        if let Some(cast) = cast_word(&self.peek) {
            let saved = self.pos;
            let saved_peek = self.peek.clone();
            self.bump();
            if self.eat(")") {
                let operand = self.parse_bp_with_colon(121)?;
                return Ok(Expr::Unary(cast, box_expr(operand)?));
            }
            self.pos = saved;
            self.peek = saved_peek;
        }
        if self.eat(")") {
            return Ok(Expr::Tuple(Vec::new()));
        }
        let leading_comma = self.eat(",");
        let first = self.parse_bp_with_colon(0)?;
        if !leading_comma && self.eat(")") {
            return Ok(first);
        }
        if leading_comma && self.eat(")") {
            return Ok(Expr::Tuple(vec![first]));
        }
        if !self.eat(",") {
            return Err(self.or_fail(MacroEvalError::SyntaxUnexpected(self.peek.name())));
        }
        let mut items = vec![first];
        if !self.eat(")") {
            // `expr COMMA` is a singleton tuple production; its recursive
            // `tuple_comma_expr COMMA expr` permits `(1,,2)`.
            if !leading_comma {
                self.eat(",");
            }
            loop {
                if self.peek == Tok::End {
                    return Err(self.or_fail(MacroEvalError::SyntaxEol));
                }
                items.push(self.parse_bp_with_colon(0)?);
                if items.len() > RANGE_CAP {
                    return Err(MacroEvalError::Limit("collection size"));
                }
                if self.eat(")") {
                    break;
                }
                if !self.eat(",") {
                    return Err(self.or_fail(MacroEvalError::SyntaxUnexpected(self.peek.name())));
                }
            }
        }
        fit_all(&items)?;
        Ok(Expr::Tuple(items))
    }

    fn parse_bracket(&mut self) -> Result<Expr, MacroEvalError> {
        self.bump();
        if self.eat("]") {
            return Ok(Expr::Array(Vec::new()));
        }
        // `comma_expr` can start with its empty production before a comma.
        if self.eat(",") {
            if matches!(self.peek, Tok::Word("]", _)) {
                return Err(MacroEvalError::SyntaxUnexpected("RBRACKET"));
            }
            let items = self.parse_commas("]", false)?;
            fit_all(&items)?;
            return Ok(Expr::Array(items));
        }
        let first = self.parse_bp_with_colon(0)?;
        if self.eat("for") {
            // The loop name stops before `in`. A looser parse would consume
            // `i in [1,2]` as the name.
            let vars = self.parse_bp_with_colon(51)?;
            if !self.eat("in") {
                return Err(self.or_fail(MacroEvalError::SyntaxUnexpected(self.peek.name())));
            }
            let set = self.parse_bp_with_colon(0)?;
            let when = if self.eat("when") {
                Some(self.parse_bp_with_colon(0)?)
            } else {
                None
            };
            if !self.eat("]") {
                return Err(self.or_fail(MacroEvalError::SyntaxUnexpected(self.peek.name())));
            }
            fit1(&first)?;
            fit1(&vars)?;
            fit1(&set)?;
            if let Some(when) = &when {
                fit1(when)?;
            }
            return Ok(Expr::Map {
                expr: Box::new(first),
                vars: Box::new(vars),
                set: Box::new(set),
                when: when.map(Box::new),
            });
        }
        if let Expr::Binary(BinOp::In, vars, set) = first {
            if self.eat("when") {
                let when = self.parse_bp_with_colon(0)?;
                if !self.eat("]") {
                    return Err(self.or_fail(MacroEvalError::SyntaxUnexpected(self.peek.name())));
                }
                fit1(&when)?;
                return Ok(Expr::Filter {
                    vars,
                    set,
                    when: Box::new(when),
                });
            }
            let mut items = vec![Expr::Binary(BinOp::In, vars, set)];
            return self.finish_array(items.swap_remove(0), &mut items);
        }
        let mut items = Vec::new();
        self.finish_array(first, &mut items)
    }

    fn finish_array(&mut self, first: Expr, items: &mut Vec<Expr>) -> Result<Expr, MacroEvalError> {
        items.push(first);
        while self.eat(",") {
            if self.peek == Tok::End {
                return Err(self.or_fail(MacroEvalError::SyntaxEol));
            }
            items.push(self.parse_bp_with_colon(0)?);
            if items.len() > RANGE_CAP {
                return Err(MacroEvalError::Limit("collection size"));
            }
        }
        if !self.eat("]") {
            return Err(self.or_fail(MacroEvalError::SyntaxUnexpected(self.peek.name())));
        }
        let items = std::mem::take(items);
        fit_all(&items)?;
        Ok(Expr::Array(items))
    }

    fn parse_commas(
        &mut self,
        end: &str,
        leading_comma: bool,
    ) -> Result<Vec<Expr>, MacroEvalError> {
        let mut args = Vec::new();
        if self.eat(end) {
            return Ok(args);
        }
        if leading_comma {
            self.eat(",");
        }
        loop {
            if self.peek == Tok::End {
                return Err(self.or_fail(MacroEvalError::SyntaxEol));
            }
            args.push(self.parse_bp_with_colon(0)?);
            if args.len() > RANGE_CAP {
                return Err(MacroEvalError::Limit("collection size"));
            }
            if self.eat(end) {
                return Ok(args);
            }
            if !self.eat(",") {
                return Err(self.or_fail(MacroEvalError::SyntaxUnexpected(self.peek.name())));
            }
        }
    }
}

fn infix_binding(tok: &Tok) -> Option<(BinOp, u8, bool)> {
    let Tok::Word(text, _) = tok else {
        return None;
    };
    Some(match *text {
        "||" => (BinOp::Or, 10, false),
        "&&" => (BinOp::And, 20, false),
        "==" => (BinOp::Eq, 30, false),
        "!=" => (BinOp::Ne, 30, false),
        "<" => (BinOp::Lt, 40, false),
        ">" => (BinOp::Gt, 40, false),
        "<=" => (BinOp::Le, 40, false),
        ">=" => (BinOp::Ge, 40, false),
        "in" => (BinOp::In, 50, true),
        ":" => return colon_as_binary(),
        "|" => (BinOp::Union, 70, false),
        "&" => (BinOp::Inter, 80, false),
        "+" => (BinOp::Add, 90, false),
        "-" => (BinOp::Sub, 90, false),
        "*" => (BinOp::Mul, 100, false),
        "/" => (BinOp::Div, 100, false),
        _ => return None,
    })
}

fn colon_as_binary() -> Option<(BinOp, u8, bool)> {
    // Colon is parsed in `parse_bp` by a dedicated path. Returning None here
    // would drop it, so the binding is installed by `parse_colon_layer`.
    None
}

fn keyword(rest: &str) -> Option<(&'static str, &'static str)> {
    const WORDS: &[(&str, &str)] = &[
        ("||", "OR"),
        ("&&", "AND"),
        ("==", "EQUAL_EQUAL"),
        ("!=", "NOT_EQUAL"),
        ("=", "EQUAL"),
        ("<=", "LESS_EQUAL"),
        (">=", "GREATER_EQUAL"),
        ("defined", "DEFINED"),
        ("isempty", "ISEMPTY"),
        ("isboolean", "ISBOOLEAN"),
        ("isstring", "ISSTRING"),
        ("istuple", "ISTUPLE"),
        ("isarray", "ISARRAY"),
        ("isreal", "ISREAL"),
        ("log10", "LOG10"),
        ("normpdf", "NORMPDF"),
        ("normcdf", "NORMCDF"),
        ("length", "LENGTH"),
        ("lgamma", "LGAMMA"),
        ("false", "FALSE"),
        ("floor", "FLOOR"),
        ("trunc", "TRUNC"),
        ("round", "ROUND"),
        ("gamma", "GAMMA"),
        ("string", "STRING"),
        ("tuple", "TUPLE"),
        ("array", "ARRAY"),
        ("when", "WHEN"),
        ("true", "TRUE"),
        ("asin", "ATAN"),
        ("acos", "ACOS"),
        ("atan", "ATAN"),
        ("sqrt", "SQRT"),
        ("cbrt", "CBRT"),
        ("sign", "SIGN"),
        ("ceil", "CEIL"),
        ("log", "LOG"),
        ("exp", "EXP"),
        ("sin", "SIN"),
        ("cos", "COS"),
        ("tan", "TAN"),
        ("max", "MAX"),
        ("min", "MIN"),
        ("mod", "MOD"),
        ("sum", "SUM"),
        ("erf", "ERF"),
        ("erfc", "ERFC"),
        ("for", "FOR"),
        ("bool", "BOOL"),
        ("real", "REAL"),
        ("ln", "LN"),
        ("in", "IN"),
        ("(", "LPAREN"),
        (")", "RPAREN"),
        ("[", "LBRACKET"),
        ("]", "RBRACKET"),
        (",", "COMMA"),
        (":", "COLON"),
        ("+", "PLUS"),
        ("-", "MINUS"),
        ("*", "TIMES"),
        ("/", "DIVIDE"),
        ("^", "POWER"),
        ("<", "LESS"),
        (">", "GREATER"),
        ("!", "NOT"),
        ("|", "UNION"),
        ("&", "INTERSECTION"),
    ];
    for (spelling, name) in WORDS {
        if rest
            .get(..spelling.len())
            .is_some_and(|word| word.eq_ignore_ascii_case(spelling))
            && boundary(rest, spelling.len())
        {
            return Some((*spelling, *name));
        }
    }
    None
}

fn boundary(rest: &str, len: usize) -> bool {
    let spelling = &rest[..len];
    if spelling
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
    {
        match rest[len..].chars().next() {
            Some(ch) if ch.is_ascii_alphanumeric() || ch == '_' => return false,
            _ => {}
        }
    }
    true
}

fn is_number_start(rest: &str) -> bool {
    let b = rest.as_bytes();
    if b[0].is_ascii_digit() {
        return true;
    }
    b[0] == b'.' && b.get(1).is_some_and(|c| c.is_ascii_digit())
        || ((rest
            .get(..3)
            .is_some_and(|word| word.eq_ignore_ascii_case("nan"))
            || rest
                .get(..3)
                .is_some_and(|word| word.eq_ignore_ascii_case("inf")))
            && boundary(rest, 3))
}

fn scan_number(rest: &str) -> (f64, usize) {
    if rest
        .get(..3)
        .is_some_and(|word| word.eq_ignore_ascii_case("nan"))
        && boundary(rest, 3)
    {
        return (f64::NAN, 3);
    }
    if rest
        .get(..3)
        .is_some_and(|word| word.eq_ignore_ascii_case("inf"))
        && boundary(rest, 3)
    {
        return (f64::INFINITY, 3);
    }
    let bytes = rest.as_bytes();
    let mut i = 0;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    let digits = i;
    if i < bytes.len() && bytes[i] == b'.' {
        i += 1;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
    }
    if i == 0 || (digits == 0 && i == 1) {
        return (0.0, 1);
    }
    if i < bytes.len() && matches!(bytes[i], b'e' | b'E' | b'd' | b'D') {
        let mut j = i + 1;
        if j < bytes.len() && (bytes[j] == b'+' || bytes[j] == b'-') {
            j += 1;
        }
        if j < bytes.len() && bytes[j].is_ascii_digit() {
            while j < bytes.len() && bytes[j].is_ascii_digit() {
                j += 1;
            }
            i = j;
        }
    }
    // `strtod` accepts `e` exponents and stops at `d`, while the token still
    // consumes a `d` exponent. `1d2` is therefore the number 1.
    let value = strtod_prefix(&rest[..i]);
    (value, i)
}

fn strtod_prefix(text: &str) -> f64 {
    let bytes = text.as_bytes();
    let mut i = 0;
    if bytes.first().is_some_and(|ch| *ch == b'+' || *ch == b'-') {
        i = 1;
    }
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    if i < bytes.len() && bytes[i] == b'.' {
        i += 1;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
    }
    if i < bytes.len() && (bytes[i] == b'e' || bytes[i] == b'E') {
        let mut j = i + 1;
        if j < bytes.len() && (bytes[j] == b'+' || bytes[j] == b'-') {
            j += 1;
        }
        if j < bytes.len() && bytes[j].is_ascii_digit() {
            while j < bytes.len() && bytes[j].is_ascii_digit() {
                j += 1;
            }
            if j > i + 1 && !(j == i + 2 && !bytes[i + 1].is_ascii_digit()) {
                i = j;
            }
        }
    }
    text[..i].parse::<f64>().unwrap_or(0.0)
}

fn ident_len(rest: &str) -> usize {
    rest.chars()
        .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '_')
        .map(|ch| ch.len_utf8())
        .sum()
}

fn cast_word(tok: &Tok) -> Option<Unary> {
    let Tok::Word(text, _) = tok else {
        return None;
    };
    Some(match *text {
        "bool" => Unary::CastBool,
        "real" => Unary::CastReal,
        "string" => Unary::CastString,
        "tuple" => Unary::CastTuple,
        "array" => Unary::CastArray,
        _ => return None,
    })
}

fn eval_expr(
    expr: &Expr,
    global: &mut HashMap<String, MacroVal>,
    mut local: Option<&mut HashMap<String, MacroVal>>,
    call_depth: usize,
    expr_depth: usize,
    budget: &mut MacroBudget,
) -> Result<MacroVal, MacroEvalError> {
    if expr_depth >= EXPR_DEPTH_CAP || call_depth > CALL_DEPTH_CAP {
        return Err(MacroEvalError::Limit("expression depth"));
    }
    budget.spend_work(1)?;
    let child = expr_depth + 1;
    match expr {
        Expr::Bool(v) => Ok(MacroVal::Bool(*v)),
        Expr::Real(v) => Ok(MacroVal::Real(*v)),
        Expr::Text(v) => {
            budget.reserve_string(v.len())?;
            Ok(MacroVal::Text(v.clone()))
        }
        Expr::Var(name) => lookup_var(global, local.as_deref(), name, budget),
        Expr::Defined(name) => Ok(MacroVal::Bool(symbol_defined(
            global,
            local.as_deref(),
            name,
        ))),
        Expr::Array(items) => {
            if items.len() > RANGE_CAP {
                return Err(MacroEvalError::Limit("collection size"));
            }
            budget.spend_work(items.len())?;
            let mut values = Vec::with_capacity(items.len());
            for item in items {
                values.push(eval_expr(
                    item,
                    global,
                    local.as_deref_mut(),
                    call_depth,
                    child,
                    budget,
                )?);
            }
            seal_value(MacroVal::Array(values))
        }
        Expr::Tuple(items) => {
            if items.len() > RANGE_CAP {
                return Err(MacroEvalError::Limit("collection size"));
            }
            budget.spend_work(items.len())?;
            let mut values = Vec::with_capacity(items.len());
            for item in items {
                values.push(eval_expr(
                    item,
                    global,
                    local.as_deref_mut(),
                    call_depth,
                    child,
                    budget,
                )?);
            }
            seal_value(MacroVal::Tuple(values))
        }
        Expr::Range { start, step, end } => {
            let start = real_of(&eval_expr(
                start,
                global,
                local.as_deref_mut(),
                call_depth,
                child,
                budget,
            )?)?;
            let step = match step {
                Some(step) => real_of(&eval_expr(
                    step,
                    global,
                    local.as_deref_mut(),
                    call_depth,
                    child,
                    budget,
                )?)?,
                None => 1.0,
            };
            let end = real_of(&eval_expr(
                end,
                global,
                local.as_deref_mut(),
                call_depth,
                child,
                budget,
            )?)?;
            materialize_range(start, step, end, budget)
        }
        Expr::Unary(op, expr) => {
            let value = eval_expr(
                expr,
                global,
                local.as_deref_mut(),
                call_depth,
                child,
                budget,
            )?;
            eval_unary(*op, value, global, local.as_deref_mut(), budget)
        }
        Expr::Binary(op, left, right) => eval_binary(
            *op,
            left,
            right,
            global,
            local.as_deref_mut(),
            call_depth,
            child,
            budget,
        ),
        Expr::Index { name, indexes } => {
            let mut flat = Vec::new();
            for index in indexes {
                let value = eval_expr(
                    index,
                    global,
                    local.as_deref_mut(),
                    call_depth,
                    child,
                    budget,
                )?;
                flatten_index(value, &mut flat)?;
            }
            let target = lookup_var(global, local.as_deref(), name, budget)?;
            if indexes.is_empty() {
                return Ok(target);
            }
            apply_index(target, &flat, budget)
        }
        Expr::Call { name, args } => eval_call(name, args, global, call_depth, child, budget),
        Expr::Builtin(op, args) => {
            let mut values = Vec::with_capacity(args.len());
            for arg in args {
                values.push(eval_expr(
                    arg,
                    global,
                    local.as_deref_mut(),
                    call_depth,
                    child,
                    budget,
                )?);
            }
            eval_builtin(*op, &values)
        }
        Expr::Filter { vars, set, when } => eval_comprehension(
            None,
            vars,
            set,
            Some(when),
            global,
            local.as_deref_mut(),
            call_depth,
            child,
            budget,
        ),
        Expr::Map {
            expr,
            vars,
            set,
            when,
        } => eval_comprehension(
            Some(expr),
            vars,
            set,
            when.as_deref(),
            global,
            local,
            call_depth,
            child,
            budget,
        ),
    }
}

fn eval_call(
    name: &str,
    args: &[Expr],
    global: &mut HashMap<String, MacroVal>,
    call_depth: usize,
    expr_depth: usize,
    budget: &mut MacroBudget,
) -> Result<MacroVal, MacroEvalError> {
    if call_depth >= CALL_DEPTH_CAP {
        return Err(MacroEvalError::Limit("expression depth"));
    }
    budget.spend_work(1)?;
    let (params, body) = match global.get(name) {
        Some(MacroVal::Function { params, body }) => {
            charge_source(budget, body)?;
            for param in params {
                budget.spend_work(param.len().max(1))?;
            }
            (params.clone(), body.clone())
        }
        Some(MacroVal::Unresolved) => return Err(MacroEvalError::PriorFailure),
        _ => return Err(MacroEvalError::UnknownFunction(name.to_string())),
    };
    if params.len() != args.len() {
        return Err(official(
            "E285",
            format!(
                "The number of arguments used to call {name} does not match the number used in its definition"
            ),
        ));
    }
    let parsed = parse_expr(&unfold_continuations(&body))?;
    let mut callee = HashMap::new();
    for (param, arg) in params.iter().zip(args) {
        let value = eval_expr(
            arg,
            global,
            Some(&mut callee),
            call_depth + 1,
            expr_depth,
            budget,
        )?;
        callee.insert(param.clone(), value);
    }
    eval_expr(
        &parsed,
        global,
        Some(&mut callee),
        call_depth + 1,
        expr_depth,
        budget,
    )
}

#[allow(clippy::too_many_arguments)]
fn eval_comprehension(
    map_expr: Option<&Expr>,
    vars: &Expr,
    set: &Expr,
    when: Option<&Expr>,
    global: &mut HashMap<String, MacroVal>,
    mut local: Option<&mut HashMap<String, MacroVal>>,
    call_depth: usize,
    expr_depth: usize,
    budget: &mut MacroBudget,
) -> Result<MacroVal, MacroEvalError> {
    let input = eval_expr(
        set,
        global,
        local.as_deref_mut(),
        call_depth,
        expr_depth,
        budget,
    )?;
    let MacroVal::Array(items) = input else {
        return Err(official("E285", "The input set must evaluate to an array"));
    };
    if items.len() > RANGE_CAP {
        return Err(MacroEvalError::Limit("collection size"));
    }
    if variable_name(vars).is_none() && !matches!(vars, Expr::Tuple(_)) {
        return Err(official(
            "E285",
            "the loop variables must be either a tuple or a variable",
        ));
    }
    let mut values = Vec::new();
    for item in items {
        budget.spend_work(1)?;
        bind_comprehension(vars, &item, global, local.as_deref_mut(), budget)?;
        if let Some(when) = when {
            let cond = eval_expr(
                when,
                global,
                local.as_deref_mut(),
                call_depth,
                expr_depth,
                budget,
            )?;
            if !truth(&cond, "The condition must evaluate to a boolean or a real")? {
                continue;
            }
        }
        let value = if let Some(map_expr) = map_expr {
            eval_expr(
                map_expr,
                global,
                local.as_deref_mut(),
                call_depth,
                expr_depth,
                budget,
            )?
        } else {
            item
        };
        values.push(value);
        if values.len() > RANGE_CAP {
            return Err(MacroEvalError::Limit("collection size"));
        }
    }
    seal_value(MacroVal::Array(values))
}

fn bind_comprehension(
    vars: &Expr,
    item: &MacroVal,
    global: &mut HashMap<String, MacroVal>,
    mut local: Option<&mut HashMap<String, MacroVal>>,
    budget: &mut MacroBudget,
) -> Result<(), MacroEvalError> {
    match vars {
        Expr::Var(name) | Expr::Index { name, .. } => {
            let owned = clone_macro_val(item, budget)?;
            bind_name(global, local, name.clone(), owned)?;
            Ok(())
        }
        Expr::Tuple(names) => {
            let MacroVal::Tuple(values) = item else {
                return Err(official(
                    "E285",
                    "assigning to tuple in output expression but input expression does not contain tuples",
                ));
            };
            if names.len() != values.len() {
                return Err(official(
                    "E284",
                    "The number of elements in the input  set tuple are not the same as the number of elements in the output expression tuple",
                ));
            }
            for (name, value) in names.iter().zip(values) {
                let Some(name) = variable_name(name) else {
                    return Err(official(
                        "E285",
                        "Output expression tuple must be comprised of variable names",
                    ));
                };
                let owned = clone_macro_val(value, budget)?;
                bind_name(global, local.as_deref_mut(), name.to_owned(), owned)?;
            }
            Ok(())
        }
        _ => Err(official(
            "E285",
            "the loop variables must be either a tuple or a variable",
        )),
    }
}

fn bind_name(
    global: &mut HashMap<String, MacroVal>,
    local: Option<&mut HashMap<String, MacroVal>>,
    name: String,
    value: MacroVal,
) -> Result<(), MacroEvalError> {
    if let Some(local) = local {
        local.insert(name, value);
    } else {
        if matches!(global.get(&name), Some(MacroVal::Function { .. })) {
            return Err(official(
                "E285",
                format!("Variable {name} was previously defined as a function"),
            ));
        }
        global.insert(name, value);
    }
    Ok(())
}

fn lookup_var(
    global: &HashMap<String, MacroVal>,
    local: Option<&HashMap<String, MacroVal>>,
    name: &str,
    budget: &mut MacroBudget,
) -> Result<MacroVal, MacroEvalError> {
    let found = local
        .and_then(|map| map.get(name))
        .or_else(|| global.get(name));
    match found {
        Some(MacroVal::Unresolved) => Err(MacroEvalError::PriorFailure),
        Some(MacroVal::Function { .. }) => Err(MacroEvalError::UnknownVariable(name.to_string())),
        Some(value) => clone_macro_val(value, budget),
        None => Err(MacroEvalError::UnknownVariable(name.to_string())),
    }
}

fn symbol_defined(
    global: &HashMap<String, MacroVal>,
    local: Option<&HashMap<String, MacroVal>>,
    name: &str,
) -> bool {
    let present = |map: &HashMap<String, MacroVal>| {
        map.get(name)
            .is_some_and(|value| !matches!(value, MacroVal::Unresolved))
    };
    local.is_some_and(present) || present(global)
}

#[allow(clippy::too_many_arguments)]
fn eval_binary(
    op: BinOp,
    left: &Expr,
    right: &Expr,
    global: &mut HashMap<String, MacroVal>,
    mut local: Option<&mut HashMap<String, MacroVal>>,
    call_depth: usize,
    expr_depth: usize,
    budget: &mut MacroBudget,
) -> Result<MacroVal, MacroEvalError> {
    if matches!(op, BinOp::And | BinOp::Or) {
        let lhs = eval_expr(
            left,
            global,
            local.as_deref_mut(),
            call_depth,
            expr_depth,
            budget,
        )?;
        return eval_logic(
            op, lhs, right, global, local, call_depth, expr_depth, budget,
        );
    }
    if op == BinOp::In {
        let rhs = eval_expr(
            right,
            global,
            local.as_deref_mut(),
            call_depth,
            expr_depth,
            budget,
        )?;
        let lhs = eval_expr(left, global, local, call_depth, expr_depth, budget)?;
        return eval_values(op, lhs, rhs, budget);
    }
    let lhs = eval_expr(
        left,
        global,
        local.as_deref_mut(),
        call_depth,
        expr_depth,
        budget,
    )?;
    let rhs = eval_expr(right, global, local, call_depth, expr_depth, budget)?;
    eval_values(op, lhs, rhs, budget)
}

#[allow(clippy::too_many_arguments)]
fn eval_logic(
    op: BinOp,
    left: MacroVal,
    right: &Expr,
    global: &mut HashMap<String, MacroVal>,
    local: Option<&mut HashMap<String, MacroVal>>,
    call_depth: usize,
    expr_depth: usize,
    budget: &mut MacroBudget,
) -> Result<MacroVal, MacroEvalError> {
    let symbol = if op == BinOp::And { "&&" } else { "||" };
    let truth = match &left {
        MacroVal::Bool(v) => *v,
        MacroVal::Real(v) => *v != 0.0,
        MacroVal::Int(v) => *v != 0,
        _ => {
            return Err(official(
                "E285",
                format!("Operator {symbol} does not exist for this type"),
            ))
        }
    };
    let take_right = if op == BinOp::And { truth } else { !truth };
    if !take_right {
        return Ok(MacroVal::Bool(op == BinOp::Or));
    }
    let rhs = eval_expr(right, global, local, call_depth, expr_depth, budget)?;
    let right_truth = match &rhs {
        MacroVal::Bool(v) => *v,
        MacroVal::Real(_) | MacroVal::Int(_) => numeric(&rhs).is_some_and(|n| n != 0.0),
        _ => {
            return Err(official(
                "E285",
                format!("Type mismatch for operands of {symbol} operator"),
            ))
        }
    };
    Ok(MacroVal::Bool(right_truth))
}

fn numeric(value: &MacroVal) -> Option<f64> {
    match value {
        MacroVal::Real(v) => Some(*v),
        MacroVal::Int(v) => Some(*v as f64),
        _ => None,
    }
}

fn eval_values(
    op: BinOp,
    left: MacroVal,
    right: MacroVal,
    budget: &mut MacroBudget,
) -> Result<MacroVal, MacroEvalError> {
    match op {
        BinOp::In => {
            let items = match right {
                MacroVal::Array(values) | MacroVal::Tuple(values) => values,
                _ => return Err(MacroEvalError::InOperandType),
            };
            if items.len() > RANGE_CAP {
                return Err(MacroEvalError::Limit("collection size"));
            }
            charge_equality_scans(&left, &items, budget)?;
            Ok(MacroVal::Bool(
                items.iter().any(|item| values_equal(&left, item)),
            ))
        }
        BinOp::Add => eval_add(left, right, budget),
        BinOp::Sub => eval_sub(left, right, budget),
        BinOp::Mul => eval_mul(left, right, budget),
        BinOp::Div => eval_div(left, right),
        BinOp::Pow => eval_pow(left, right, budget),
        BinOp::Eq => {
            charge_one_equality(&left, &right, budget)?;
            Ok(MacroVal::Bool(values_equal(&left, &right)))
        }
        BinOp::Ne => {
            charge_one_equality(&left, &right, budget)?;
            Ok(MacroVal::Bool(!values_equal(&left, &right)))
        }
        BinOp::Lt => eval_cmp(left, right, "<", |a, b| a < b, |a, b| a < b),
        BinOp::Gt => eval_cmp(left, right, ">", |a, b| a > b, |a, b| a > b),
        BinOp::Le => eval_cmp(left, right, "<=", |a, b| a <= b, |a, b| a <= b),
        BinOp::Ge => eval_cmp(left, right, ">=", |a, b| a >= b, |a, b| a >= b),
        BinOp::Union => eval_union(left, right, budget),
        BinOp::Inter => eval_inter(left, right, budget),
        BinOp::And | BinOp::Or => unreachable!("logic is short-circuit"),
    }
}

fn eval_add(
    left: MacroVal,
    right: MacroVal,
    budget: &mut MacroBudget,
) -> Result<MacroVal, MacroEvalError> {
    match (&left, &right) {
        (MacroVal::Real(_) | MacroVal::Int(_), _) => match numeric(&right) {
            Some(b) => Ok(MacroVal::Real(numeric(&left).unwrap() + b)),
            None => Err(mismatch("+")),
        },
        (MacroVal::Text(a), MacroVal::Text(b)) => {
            let bytes = a.len().saturating_add(b.len());
            budget.reserve_string(bytes)?;
            Ok(MacroVal::Text(a.clone() + b))
        }
        (MacroVal::Text(_) | MacroVal::Bytes(_), MacroVal::Text(_) | MacroVal::Bytes(_)) => {
            let left_bytes = match &left {
                MacroVal::Text(text) => text.as_bytes(),
                MacroVal::Bytes(text) => text.as_slice(),
                _ => unreachable!("string arm"),
            };
            let right_bytes = match &right {
                MacroVal::Text(text) => text.as_bytes(),
                MacroVal::Bytes(text) => text.as_slice(),
                _ => unreachable!("string arm"),
            };
            let mut bytes = Vec::with_capacity(left_bytes.len().saturating_add(right_bytes.len()));
            bytes.extend_from_slice(left_bytes);
            bytes.extend_from_slice(right_bytes);
            budget.reserve_string(bytes.len())?;
            Ok(text_from_bytes(bytes))
        }
        (MacroVal::Array(a), MacroVal::Array(b)) => {
            if a.len().saturating_add(b.len()) > RANGE_CAP {
                return Err(MacroEvalError::Limit("collection size"));
            }
            budget.spend_work(a.len().saturating_add(b.len()).max(1))?;
            let mut out = Vec::with_capacity(a.len() + b.len());
            for item in a {
                out.push(clone_macro_val(item, budget)?);
            }
            for item in b {
                out.push(clone_macro_val(item, budget)?);
            }
            seal_value(MacroVal::Array(out))
        }
        (MacroVal::Text(_) | MacroVal::Bytes(_), _) | (MacroVal::Array(_), _) => Err(mismatch("+")),
        _ => Err(missing_op("+")),
    }
}

fn eval_sub(
    left: MacroVal,
    right: MacroVal,
    budget: &mut MacroBudget,
) -> Result<MacroVal, MacroEvalError> {
    match (&left, &right) {
        (MacroVal::Real(_) | MacroVal::Int(_), MacroVal::Real(_) | MacroVal::Int(_)) => Ok(
            MacroVal::Real(numeric(&left).unwrap() - numeric(&right).unwrap()),
        ),
        (MacroVal::Array(a), MacroVal::Array(b)) => {
            charge_set_scans(a, b, false, budget)?;
            let mut out = Vec::new();
            for item in a {
                if !b.iter().any(|other| values_equal(item, other)) {
                    out.push(clone_macro_val(item, budget)?);
                }
            }
            seal_value(MacroVal::Array(out))
        }
        (MacroVal::Real(_) | MacroVal::Int(_) | MacroVal::Array(_), _) => Err(mismatch("-")),
        _ => Err(missing_op("-")),
    }
}

fn eval_mul(
    left: MacroVal,
    right: MacroVal,
    budget: &mut MacroBudget,
) -> Result<MacroVal, MacroEvalError> {
    match (&left, &right) {
        (MacroVal::Real(_) | MacroVal::Int(_), MacroVal::Real(_) | MacroVal::Int(_)) => Ok(
            MacroVal::Real(numeric(&left).unwrap() * numeric(&right).unwrap()),
        ),
        (MacroVal::Array(a), MacroVal::Array(b)) => {
            seal_value(MacroVal::Array(cartesian(a, b, budget)?))
        }
        (MacroVal::Array(_), _) => Err(mismatch("*")),
        _ => Err(missing_op("*")),
    }
}

fn eval_div(left: MacroVal, right: MacroVal) -> Result<MacroVal, MacroEvalError> {
    match (&left, &right) {
        (MacroVal::Real(_) | MacroVal::Int(_), MacroVal::Real(_) | MacroVal::Int(_)) => Ok(
            MacroVal::Real(numeric(&left).unwrap() / numeric(&right).unwrap()),
        ),
        (MacroVal::Real(_) | MacroVal::Int(_), _) => Err(mismatch("/")),
        _ => Err(missing_op("/")),
    }
}

fn eval_pow(
    left: MacroVal,
    right: MacroVal,
    budget: &mut MacroBudget,
) -> Result<MacroVal, MacroEvalError> {
    match left {
        MacroVal::Real(_) | MacroVal::Int(_) => {
            let a = numeric(&left).unwrap();
            match numeric(&right) {
                Some(b) => Ok(MacroVal::Real(a.powf(b))),
                None => Err(mismatch("^")),
            }
        }
        MacroVal::Array(items) => {
            let Some(exp) = numeric(&right) else {
                return Err(official(
                    "E285",
                    "The second argument of the power operator (^) must be an integer",
                ));
            };
            if !is_integer(exp) {
                return Err(official(
                    "E285",
                    "The second argument of the power operator (^) must be an integer",
                ));
            }
            if exp < 1.0 {
                return seal_value(MacroVal::Array(items));
            }
            let times = exp as u32;
            if times > 32 {
                return Err(MacroEvalError::Limit("collection size"));
            }
            let mut acc = Vec::with_capacity(items.len());
            for item in &items {
                acc.push(clone_macro_val(item, budget)?);
            }
            for _ in 1..times {
                acc = cartesian(&acc, &items, budget)?;
            }
            seal_value(MacroVal::Array(acc))
        }
        _ => Err(missing_op("^")),
    }
}

fn cartesian(
    left: &[MacroVal],
    right: &[MacroVal],
    budget: &mut MacroBudget,
) -> Result<Vec<MacroVal>, MacroEvalError> {
    let count = left
        .len()
        .checked_mul(right.len())
        .ok_or(MacroEvalError::Limit("collection size"))?;
    if count > RANGE_CAP {
        return Err(MacroEvalError::Limit("collection size"));
    }
    budget.spend_work(count.max(1))?;
    let mut out = Vec::with_capacity(count);
    for lhs in left {
        for rhs in right {
            out.push(seal_value(MacroVal::Tuple(concat_tuple(
                lhs, rhs, budget,
            )?))?);
        }
    }
    Ok(out)
}

fn concat_tuple(
    left: &MacroVal,
    right: &MacroVal,
    budget: &mut MacroBudget,
) -> Result<Vec<MacroVal>, MacroEvalError> {
    let mut row = Vec::new();
    push_factor(&mut row, left, true, budget)?;
    push_factor(&mut row, right, false, budget)?;
    Ok(row)
}

fn push_factor(
    row: &mut Vec<MacroVal>,
    value: &MacroVal,
    left: bool,
    budget: &mut MacroBudget,
) -> Result<(), MacroEvalError> {
    match value {
        MacroVal::Real(_) | MacroVal::Int(_) | MacroVal::Text(_) => {
            row.push(clone_macro_val(value, budget)?);
        }
        MacroVal::Tuple(items) => {
            for item in items {
                row.push(clone_macro_val(item, budget)?);
            }
        }
        _ => {
            return Err(official(
                "E285",
                if left {
                    "Array::times: unsupported type on lhs"
                } else {
                    "Array::times: unsupported type on rhs"
                },
            ));
        }
    }
    Ok(())
}

fn eval_union(
    left: MacroVal,
    right: MacroVal,
    budget: &mut MacroBudget,
) -> Result<MacroVal, MacroEvalError> {
    let MacroVal::Array(left) = left else {
        return Err(missing_op("|"));
    };
    let MacroVal::Array(right) = right else {
        return Err(official(
            "E285",
            "Arguments of the union operator (|) must be sets",
        ));
    };
    charge_set_scans(&left, &right, true, budget)?;
    let mut out = left;
    for item in right {
        if !out.iter().any(|have| values_equal(have, &item)) {
            out.push(item);
            if out.len() > RANGE_CAP {
                return Err(MacroEvalError::Limit("collection size"));
            }
        }
    }
    seal_value(MacroVal::Array(out))
}

fn eval_inter(
    left: MacroVal,
    right: MacroVal,
    budget: &mut MacroBudget,
) -> Result<MacroVal, MacroEvalError> {
    let MacroVal::Array(left) = left else {
        return Err(missing_op("&"));
    };
    let MacroVal::Array(right) = right else {
        return Err(official(
            "E285",
            "Arguments of the intersection operator (|) must be sets",
        ));
    };
    charge_set_scans(&left, &right, false, budget)?;
    let mut out = Vec::new();
    for item in right {
        if left.iter().any(|have| values_equal(have, &item)) {
            out.push(item);
        }
    }
    seal_value(MacroVal::Array(out))
}

fn eval_cmp(
    left: MacroVal,
    right: MacroVal,
    op: &'static str,
    reals: fn(f64, f64) -> bool,
    texts: fn(&str, &str) -> bool,
) -> Result<MacroVal, MacroEvalError> {
    match (&left, &right) {
        (MacroVal::Real(_) | MacroVal::Int(_), MacroVal::Real(_) | MacroVal::Int(_)) => Ok(
            MacroVal::Bool(reals(numeric(&left).unwrap(), numeric(&right).unwrap())),
        ),
        (MacroVal::Text(a), MacroVal::Text(b)) => Ok(MacroVal::Bool(texts(a, b))),
        (MacroVal::Text(_) | MacroVal::Bytes(_), MacroVal::Text(_) | MacroVal::Bytes(_)) => {
            let left_bytes = match &left {
                MacroVal::Text(text) => text.as_bytes(),
                MacroVal::Bytes(text) => text.as_slice(),
                _ => unreachable!("string arm"),
            };
            let right_bytes = match &right {
                MacroVal::Text(text) => text.as_bytes(),
                MacroVal::Bytes(text) => text.as_slice(),
                _ => unreachable!("string arm"),
            };
            Ok(MacroVal::Bool(match op {
                "<" => left_bytes < right_bytes,
                ">" => left_bytes > right_bytes,
                "<=" => left_bytes <= right_bytes,
                ">=" => left_bytes >= right_bytes,
                _ => return Err(missing_op(op)),
            }))
        }
        (MacroVal::Real(_) | MacroVal::Int(_) | MacroVal::Text(_) | MacroVal::Bytes(_), _) => {
            Err(mismatch(op))
        }
        _ => Err(missing_op(op)),
    }
}

fn eval_unary(
    op: Unary,
    value: MacroVal,
    global: &mut HashMap<String, MacroVal>,
    local: Option<&mut HashMap<String, MacroVal>>,
    budget: &mut MacroBudget,
) -> Result<MacroVal, MacroEvalError> {
    match op {
        Unary::Not => match &value {
            MacroVal::Bool(v) => Ok(MacroVal::Bool(!v)),
            MacroVal::Real(v) => Ok(MacroVal::Bool(*v == 0.0)),
            MacroVal::Int(v) => Ok(MacroVal::Bool(*v == 0)),
            _ => Err(missing_op("!")),
        },
        Unary::Neg => match numeric(&value) {
            Some(v) => Ok(MacroVal::Real(-v)),
            None => Err(missing_op_unary("-")),
        },
        Unary::Pos => match numeric(&value) {
            Some(v) => Ok(MacroVal::Real(v)),
            None => Err(missing_op_unary("+")),
        },
        Unary::CastBool => cast_bool(value, global, local),
        Unary::CastReal => cast_real(value, global, local),
        Unary::CastString => Ok(MacroVal::Text(render_stored(&value, budget)?)),
        Unary::CastTuple => seal_value(MacroVal::Tuple(singleton_or_items(&value, true, budget)?)),
        Unary::CastArray => seal_value(MacroVal::Array(singleton_or_items(&value, false, budget)?)),
    }
}

fn singleton_or_items(
    value: &MacroVal,
    tuple_identity: bool,
    budget: &mut MacroBudget,
) -> Result<Vec<MacroVal>, MacroEvalError> {
    let items = match value {
        MacroVal::Tuple(items) if tuple_identity => items,
        MacroVal::Array(items) if !tuple_identity => items,
        MacroVal::Array(items) if tuple_identity => items,
        MacroVal::Tuple(items) if !tuple_identity => items,
        other => return Ok(vec![clone_macro_val(other, budget)?]),
    };
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        out.push(clone_macro_val(item, budget)?);
    }
    Ok(out)
}

fn cast_bool(
    value: MacroVal,
    global: &mut HashMap<String, MacroVal>,
    local: Option<&mut HashMap<String, MacroVal>>,
) -> Result<MacroVal, MacroEvalError> {
    match value {
        MacroVal::Bool(v) => Ok(MacroVal::Bool(v)),
        MacroVal::Real(v) => Ok(MacroVal::Bool(v != 0.0)),
        MacroVal::Int(v) => Ok(MacroVal::Bool(v != 0)),
        MacroVal::Text(s) => Ok(MacroVal::Bool(string_to_bool(&s)?)),
        MacroVal::Array(items) => cast_singleton(items, "Array", true, global, local),
        MacroVal::Tuple(items) => cast_singleton(items, "Tuple", true, global, local),
        _ => Err(official("E285", "This type cannot be cast to a boolean")),
    }
}

fn cast_real(
    value: MacroVal,
    global: &mut HashMap<String, MacroVal>,
    local: Option<&mut HashMap<String, MacroVal>>,
) -> Result<MacroVal, MacroEvalError> {
    match value {
        MacroVal::Real(v) => Ok(MacroVal::Real(v)),
        MacroVal::Int(v) => Ok(MacroVal::Real(v as f64)),
        MacroVal::Bool(v) => Ok(MacroVal::Real(if v { 1.0 } else { 0.0 })),
        MacroVal::Text(s) => Ok(MacroVal::Real(string_to_real(&s)?)),
        MacroVal::Array(items) => cast_singleton(items, "Array", false, global, local),
        MacroVal::Tuple(items) => cast_singleton(items, "Tuple", false, global, local),
        _ => Err(official("E285", "This type cannot be cast to a real")),
    }
}

fn cast_singleton(
    items: Vec<MacroVal>,
    kind: &str,
    boolean: bool,
    global: &mut HashMap<String, MacroVal>,
    local: Option<&mut HashMap<String, MacroVal>>,
) -> Result<MacroVal, MacroEvalError> {
    if items.len() != 1 {
        let target = if boolean { "boolean" } else { "real" };
        return Err(official(
            "E285",
            format!("{kind} must be of size 1 to be cast to a {target}"),
        ));
    }
    let item = items.into_iter().next().unwrap();
    if boolean {
        cast_bool(item, global, local)
    } else {
        cast_real(item, global, local)
    }
}

fn string_to_bool(value: &str) -> Result<bool, MacroEvalError> {
    if value.eq_ignore_ascii_case("true") {
        return Ok(true);
    }
    if value.eq_ignore_ascii_case("false") {
        return Ok(false);
    }
    match string_to_real(value) {
        Ok(number) => Ok(number != 0.0),
        Err(_) => Err(official(
            "E285",
            format!("\"{value}\" cannot be converted to a boolean"),
        )),
    }
}

fn string_to_real(value: &str) -> Result<f64, MacroEvalError> {
    lexical_stod(value)
        .ok_or_else(|| official("E285", format!("\"{value}\" cannot be converted to a real")))
}

/// Pinned `std::stod`: leading C whitespace, a full subject sequence, and no
/// `ERANGE` result. Trailing whitespace is not consumed.
fn lexical_stod(value: &str) -> Option<f64> {
    let bytes = value.as_bytes();
    let mut i = 0;
    while i < bytes.len() && is_c_space(bytes[i]) {
        i += 1;
    }
    if i == bytes.len() {
        return None;
    }
    let rest = &value[i..];
    let (sign, body) = match rest.as_bytes().first() {
        Some(b'+') => (1.0, &rest[1..]),
        Some(b'-') => (-1.0, &rest[1..]),
        _ => (1.0, rest),
    };
    if body.is_empty() {
        return None;
    }
    if let Some(special) = parse_special(body) {
        return Some(apply_sign(sign, special));
    }
    if looks_hex(body) {
        let number = parse_hex_float(body)?;
        return accept_stod_range(apply_sign(sign, number), hex_significand_is_zero(body));
    }
    let consumed = decimal_subject_len(body);
    if consumed == 0 || consumed != body.len() {
        return None;
    }
    let number = body.parse::<f64>().ok()?;
    accept_stod_range(apply_sign(sign, number), decimal_significand_is_zero(body))
}

fn apply_sign(sign: f64, number: f64) -> f64 {
    if sign < 0.0 {
        -number
    } else {
        number
    }
}

fn is_c_space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c)
}

fn parse_special(body: &str) -> Option<f64> {
    let lower = body.to_ascii_lowercase();
    if lower == "inf" || lower == "infinity" {
        return Some(f64::INFINITY);
    }
    if lower == "nan" {
        return Some(f64::NAN);
    }
    let bytes = lower.as_bytes();
    if bytes.len() >= 5 && bytes.starts_with(b"nan(") && bytes.ends_with(b")") {
        let inside = &bytes[4..bytes.len() - 1];
        if inside.contains(&b')') {
            return None;
        }
        return Some(f64::NAN);
    }
    None
}

fn looks_hex(body: &str) -> bool {
    let bytes = body.as_bytes();
    bytes.len() >= 2 && bytes[0] == b'0' && (bytes[1] == b'x' || bytes[1] == b'X')
}

fn hex_significand_is_zero(body: &str) -> bool {
    body.bytes()
        .skip(2)
        .take_while(|byte| byte.is_ascii_hexdigit() || *byte == b'.')
        .filter(|byte| byte.is_ascii_hexdigit())
        .all(|byte| byte == b'0')
}

fn decimal_significand_is_zero(body: &str) -> bool {
    body.bytes()
        .take_while(|byte| byte.is_ascii_digit() || *byte == b'.')
        .filter(|byte| byte.is_ascii_digit())
        .all(|byte| byte == b'0')
}

fn decimal_subject_len(body: &str) -> usize {
    let bytes = body.as_bytes();
    let mut i = 0;
    let start = i;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    if i < bytes.len() && bytes[i] == b'.' {
        i += 1;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
    }
    if i == start || (i == start + 1 && bytes.get(start) == Some(&b'.')) {
        return 0;
    }
    if i < bytes.len() && (bytes[i] == b'e' || bytes[i] == b'E') {
        let mut j = i + 1;
        if j < bytes.len() && (bytes[j] == b'+' || bytes[j] == b'-') {
            j += 1;
        }
        let exp_digits = j;
        while j < bytes.len() && bytes[j].is_ascii_digit() {
            j += 1;
        }
        if j == exp_digits {
            return 0;
        }
        i = j;
    }
    i
}

fn parse_hex_float(body: &str) -> Option<f64> {
    let bytes = body.as_bytes();
    let mut i = 2;
    let mut value = 0.0f64;
    let mut scale = 0.0625f64;
    let mut saw_digit = false;
    let mut dot = false;
    while i < bytes.len() {
        if bytes[i] == b'.' {
            if dot {
                return None;
            }
            dot = true;
            i += 1;
            continue;
        }
        if !bytes[i].is_ascii_hexdigit() {
            break;
        }
        saw_digit = true;
        let digit = hex_digit(bytes[i])?;
        if dot {
            value += digit as f64 * scale;
            scale /= 16.0;
        } else {
            value = value * 16.0 + digit as f64;
        }
        i += 1;
    }
    if !saw_digit {
        return None;
    }
    let mut exp = 0i32;
    if i < bytes.len() && (bytes[i] == b'p' || bytes[i] == b'P') {
        i += 1;
        let mut exp_sign = 1i32;
        if i < bytes.len() && (bytes[i] == b'+' || bytes[i] == b'-') {
            if bytes[i] == b'-' {
                exp_sign = -1;
            }
            i += 1;
        }
        if i >= bytes.len() || !bytes[i].is_ascii_digit() {
            return None;
        }
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            exp = exp
                .saturating_mul(10)
                .saturating_add((bytes[i] - b'0') as i32);
            i += 1;
        }
        exp *= exp_sign;
    }
    if i != bytes.len() {
        return None;
    }
    if value == 0.0 {
        return Some(0.0);
    }
    Some(value * 2f64.powi(exp))
}

fn hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn accept_stod_range(number: f64, exact_zero: bool) -> Option<f64> {
    if number.is_nan() {
        return Some(number);
    }
    if number.is_infinite() {
        return None;
    }
    if number == 0.0 {
        return exact_zero.then_some(number);
    }
    number.is_normal().then_some(number)
}

fn eval_builtin(op: Builtin, args: &[MacroVal]) -> Result<MacroVal, MacroEvalError> {
    match op {
        Builtin::Length => length_of(&args[0]),
        Builtin::IsEmpty => isempty_of(&args[0]),
        Builtin::IsBoolean => Ok(MacroVal::Bool(matches!(args[0], MacroVal::Bool(_)))),
        Builtin::IsReal => Ok(MacroVal::Bool(matches!(
            args[0],
            MacroVal::Real(_) | MacroVal::Int(_)
        ))),
        Builtin::IsString => Ok(MacroVal::Bool(matches!(
            args[0],
            MacroVal::Text(_) | MacroVal::Bytes(_)
        ))),
        Builtin::IsTuple => Ok(MacroVal::Bool(matches!(args[0], MacroVal::Tuple(_)))),
        Builtin::IsArray => Ok(MacroVal::Bool(matches!(args[0], MacroVal::Array(_)))),
        Builtin::Sum => sum_of(&args[0]),
        Builtin::Max => real_binary(&args[0], &args[1], "max", |a, b| if a < b { b } else { a }),
        Builtin::Min => real_binary(&args[0], &args[1], "min", |a, b| if b < a { b } else { a }),
        Builtin::Mod => real_binary(&args[0], &args[1], "mod", |a, b| a % b),
        Builtin::Normpdf3 => normpdf3(&args[0], &args[1], &args[2]),
        Builtin::Normcdf3 => normcdf3(&args[0], &args[1], &args[2]),
        other => {
            let value = numeric(&args[0]).ok_or_else(|| missing_builtin(other))?;
            let result = match other {
                Builtin::Exp => value.exp(),
                Builtin::Ln => value.ln(),
                Builtin::Log10 => value.log10(),
                Builtin::Sin => value.sin(),
                Builtin::Cos => value.cos(),
                Builtin::Tan => value.tan(),
                Builtin::Atan => value.atan(),
                Builtin::Acos => value.acos(),
                Builtin::Sqrt => value.sqrt(),
                Builtin::Cbrt => value.cbrt(),
                Builtin::Sign => {
                    if value > 0.0 {
                        1.0
                    } else if value < 0.0 {
                        -1.0
                    } else {
                        0.0
                    }
                }
                Builtin::Floor => value.floor(),
                Builtin::Ceil => value.ceil(),
                Builtin::Trunc => value.trunc(),
                Builtin::Erf => erf_real(value),
                Builtin::Erfc => erfc_real(value),
                Builtin::Gamma => gamma_real(value),
                Builtin::Lgamma => lgamma_real(value),
                Builtin::Round => value.round(),
                Builtin::Normpdf => normpdf_standard(value),
                Builtin::Normcdf => normcdf_standard(value),
                _ => return Err(missing_builtin(other)),
            };
            Ok(MacroVal::Real(result))
        }
    }
}

fn length_of(value: &MacroVal) -> Result<MacroVal, MacroEvalError> {
    let len = match value {
        MacroVal::Array(v) | MacroVal::Tuple(v) => v.len(),
        MacroVal::Text(v) => v.len(),
        MacroVal::Bytes(v) => v.len(),
        _ => return Err(missing_named("length")),
    };
    Ok(MacroVal::Real(len as f64))
}

fn isempty_of(value: &MacroVal) -> Result<MacroVal, MacroEvalError> {
    let empty = match value {
        MacroVal::Array(v) | MacroVal::Tuple(v) => v.is_empty(),
        MacroVal::Text(v) => v.is_empty(),
        MacroVal::Bytes(v) => v.is_empty(),
        _ => return Err(missing_named("isempty")),
    };
    Ok(MacroVal::Bool(empty))
}

fn sum_of(value: &MacroVal) -> Result<MacroVal, MacroEvalError> {
    let MacroVal::Array(items) = value else {
        return Err(missing_named("sum"));
    };
    let mut total = 0.0;
    for item in items {
        let Some(number) = numeric(item) else {
            return Err(official(
                "E285",
                "Type mismatch for operands of in operator",
            ));
        };
        total += number;
    }
    Ok(MacroVal::Real(total))
}

fn real_binary(
    left: &MacroVal,
    right: &MacroVal,
    name: &str,
    op: fn(f64, f64) -> f64,
) -> Result<MacroVal, MacroEvalError> {
    match (numeric(left), numeric(right)) {
        (Some(a), Some(b)) => Ok(MacroVal::Real(op(a, b))),
        (None, _) => Err(missing_named(name)),
        _ => Err(official(
            "E285",
            format!("Type mismatch for operands of `{name}` operator"),
        )),
    }
}

fn normpdf_standard(value: f64) -> f64 {
    let scale = (2.0 * std::f64::consts::PI).sqrt();
    1.0 / (scale * (value * value / 2.0).exp())
}

fn normcdf_standard(value: f64) -> f64 {
    0.5 * (1.0 + libm::erf(value / std::f64::consts::SQRT_2))
}

fn erf_real(x: f64) -> f64 {
    libm::erf(x)
}

fn erfc_real(x: f64) -> f64 {
    if x < 0.0 {
        // `erfc(-x) = 1 + erf(x)`. Musl `erfc` of a negative argument is one
        // ulp below the UCRT value, so `erfc(-1)` prints `...71` instead of
        // the pin's `...72`. `erf` already matches that pin.
        1.0 + libm::erf(-x)
    } else {
        libm::erfc(x)
    }
}

fn gamma_real(z: f64) -> f64 {
    libm::tgamma(z)
}

fn lgamma_real(z: f64) -> f64 {
    libm::lgamma_r(z).0
}

fn normpdf3(x: &MacroVal, mean: &MacroVal, sigma: &MacroVal) -> Result<MacroVal, MacroEvalError> {
    if numeric(x).is_none() {
        return Err(missing_named("normpdf"));
    }
    let (Some(x), Some(mean), Some(sigma)) = (numeric(x), numeric(mean), numeric(sigma)) else {
        return Err(official(
            "E285",
            "Type mismatch for operands of `normpdf` operator",
        ));
    };
    let z = (x - mean) / sigma;
    Ok(MacroVal::Real(
        1.0 / (sigma * (2.0 * std::f64::consts::PI).sqrt() * (z * z / 2.0).exp()),
    ))
}

fn normcdf3(x: &MacroVal, mean: &MacroVal, sigma: &MacroVal) -> Result<MacroVal, MacroEvalError> {
    if numeric(x).is_none() {
        return Err(missing_named("normcdf"));
    }
    let (Some(x), Some(mean), Some(sigma)) = (numeric(x), numeric(mean), numeric(sigma)) else {
        return Err(official(
            "E285",
            "Type mismatch for operands of `normpdf` operator",
        ));
    };
    Ok(MacroVal::Real(normcdf_standard((x - mean) / sigma)))
}

fn materialize_range(
    start: f64,
    step: f64,
    end: f64,
    budget: &mut MacroBudget,
) -> Result<MacroVal, MacroEvalError> {
    let mut out = Vec::new();
    let mut i = start;
    if step > 0.0 {
        while i <= end {
            if out.len() >= RANGE_CAP {
                return Err(MacroEvalError::Limit("range size"));
            }
            budget.spend_work(1)?;
            out.push(MacroVal::Real(i));
            let next = i + step;
            if next == i {
                return Err(MacroEvalError::Limit("range size"));
            }
            i = next;
        }
    } else if step < 0.0 {
        while i >= end {
            if out.len() >= RANGE_CAP {
                return Err(MacroEvalError::Limit("range size"));
            }
            budget.spend_work(1)?;
            out.push(MacroVal::Real(i));
            let next = i + step;
            if next == i {
                return Err(MacroEvalError::Limit("range size"));
            }
            i = next;
        }
    }
    Ok(MacroVal::Array(out))
}

fn real_of(value: &MacroVal) -> Result<f64, MacroEvalError> {
    numeric(value).ok_or_else(|| {
        official(
            "E285",
            "To create an array from a range using the colon operator, the arguments must evaluate to reals",
        )
    })
}

fn flatten_index(value: MacroVal, out: &mut Vec<i64>) -> Result<(), MacroEvalError> {
    match value {
        MacroVal::Real(_) | MacroVal::Int(_) => {
            out.push(index_int(&value)?);
            Ok(())
        }
        MacroVal::Array(items) => {
            for item in items {
                match item {
                    MacroVal::Real(_) | MacroVal::Int(_) => out.push(index_int(&item)?),
                    MacroVal::Array(_) => {
                        return Err(official(
                            "E285",
                            "You cannot index a variable with a nested array",
                        ))
                    }
                    _ => {
                        return Err(official(
                            "E285",
                            "You cannot index a variable with a nested array",
                        ))
                    }
                }
            }
            Ok(())
        }
        _ => Err(official(
            "E285",
            "You can only index a variable with an int or an int array",
        )),
    }
}

fn index_int(value: &MacroVal) -> Result<i64, MacroEvalError> {
    let number = numeric(value).unwrap();
    if !is_integer(number) {
        return Err(official(
            "E285",
            "When indexing a variable you must pass an int or an int array",
        ));
    }
    if number < i64::MIN as f64 || number > i64::MAX as f64 {
        return Err(official("E285", "Index out of range"));
    }
    Ok(number as i64)
}

fn apply_index(
    value: MacroVal,
    indexes: &[i64],
    budget: &mut MacroBudget,
) -> Result<MacroVal, MacroEvalError> {
    match value {
        MacroVal::Bool(_) => Err(official("E285", "You cannot index a boolean")),
        MacroVal::Real(_) | MacroVal::Int(_) => Err(official("E285", "You cannot index a real")),
        MacroVal::Tuple(_) => Err(official("E285", "You cannot index a tuple")),
        MacroVal::Text(text) => index_text(text.as_bytes(), indexes, budget),
        MacroVal::Bytes(text) => index_text(&text, indexes, budget),
        MacroVal::Array(items) => {
            if indexes.len() > RANGE_CAP {
                return Err(MacroEvalError::Limit("collection size"));
            }
            budget.spend_work(indexes.len().max(1))?;
            let mut picked = Vec::new();
            for index in indexes {
                let pos = index_pos(*index, items.len())?;
                if pos == items.len() {
                    return Err(official("E285", "Index out of range"));
                }
                picked.push(clone_macro_val(&items[pos], budget)?);
            }
            if picked.len() == 1 {
                Ok(picked.pop().unwrap())
            } else {
                seal_value(MacroVal::Array(picked))
            }
        }
        other => Err(official(
            "E285",
            format!("You cannot index a {}", type_name(&other)),
        )),
    }
}

/// One-based index. `pos == len` is the pinned string `substr` boundary.
fn index_pos(index: i64, len: usize) -> Result<usize, MacroEvalError> {
    let pos = index
        .checked_sub(1)
        .filter(|pos| *pos >= 0)
        .ok_or_else(|| official("E285", "Index out of range"))?;
    let pos = pos as usize;
    if pos > len {
        return Err(official("E285", "Index out of range"));
    }
    Ok(pos)
}

fn index_text(
    bytes: &[u8],
    indexes: &[i64],
    budget: &mut MacroBudget,
) -> Result<MacroVal, MacroEvalError> {
    if indexes.len() > STRING_CAP {
        return Err(MacroEvalError::Limit("string size"));
    }
    budget.reserve_string(indexes.len())?;
    let mut out = Vec::with_capacity(indexes.len());
    for index in indexes {
        let pos = index_pos(*index, bytes.len())?;
        if pos == bytes.len() {
            continue;
        }
        out.push(bytes[pos]);
    }
    Ok(text_from_bytes(out))
}

fn type_name(value: &MacroVal) -> &'static str {
    match value {
        MacroVal::Bool(_) => "boolean",
        MacroVal::Real(_) | MacroVal::Int(_) => "real",
        MacroVal::Text(_) | MacroVal::Bytes(_) => "string",
        MacroVal::Tuple(_) => "tuple",
        MacroVal::Array(_) => "array",
        MacroVal::Function { .. } => "function",
        MacroVal::Unresolved => "value",
    }
}

fn values_equal(left: &MacroVal, right: &MacroVal) -> bool {
    match (left, right) {
        (MacroVal::Bool(a), MacroVal::Bool(b)) => a == b,
        (MacroVal::Text(a), MacroVal::Text(b)) => a == b,
        (MacroVal::Text(_) | MacroVal::Bytes(_), MacroVal::Text(_) | MacroVal::Bytes(_)) => {
            let left_bytes = match left {
                MacroVal::Text(text) => text.as_bytes(),
                MacroVal::Bytes(text) => text.as_slice(),
                _ => return false,
            };
            let right_bytes = match right {
                MacroVal::Text(text) => text.as_bytes(),
                MacroVal::Bytes(text) => text.as_slice(),
                _ => return false,
            };
            left_bytes == right_bytes
        }
        (MacroVal::Int(a), MacroVal::Int(b)) => a == b,
        (MacroVal::Real(a), MacroVal::Real(b)) => a == b,
        (MacroVal::Int(a), MacroVal::Real(b)) => *a as f64 == *b,
        (MacroVal::Real(a), MacroVal::Int(b)) => *a == *b as f64,
        (MacroVal::Array(a), MacroVal::Array(b)) | (MacroVal::Tuple(a), MacroVal::Tuple(b)) => {
            a.len() == b.len() && a.iter().zip(b.iter()).all(|(x, y)| values_equal(x, y))
        }
        _ => false,
    }
}

fn is_integer(value: f64) -> bool {
    value.is_finite() && value.fract() == 0.0
}

fn truth(value: &MacroVal, message: &str) -> Result<bool, MacroEvalError> {
    match value {
        MacroVal::Bool(v) => Ok(*v),
        MacroVal::Real(v) => Ok(*v != 0.0),
        MacroVal::Int(v) => Ok(*v != 0),
        _ => Err(official("E283", message)),
    }
}

fn charge_one_equality(
    left: &MacroVal,
    right: &MacroVal,
    budget: &mut MacroBudget,
) -> Result<(), MacroEvalError> {
    let left_cost = measure_val(left)?;
    let right_cost = measure_val(right)?;
    let units = left_cost
        .nodes
        .saturating_add(right_cost.nodes)
        .saturating_add(left_cost.bytes)
        .saturating_add(right_cost.bytes);
    budget.spend_work(units.max(1))
}

fn charge_equality_scans(
    needle: &MacroVal,
    items: &[MacroVal],
    budget: &mut MacroBudget,
) -> Result<(), MacroEvalError> {
    let needle_cost = measure_val(needle)?;
    let pair = needle_cost
        .nodes
        .saturating_add(needle_cost.bytes)
        .saturating_add(max_child_nodes(items)?)
        .saturating_add(max_child_bytes(items)?)
        .max(1);
    let work = items
        .len()
        .checked_mul(pair)
        .ok_or(MacroEvalError::Limit("iteration work"))?;
    budget.spend_work(work.max(1))
}

fn max_child_nodes(items: &[MacroVal]) -> Result<usize, MacroEvalError> {
    let mut max_nodes = 1usize;
    for item in items {
        max_nodes = max_nodes.max(measure_val(item)?.nodes);
    }
    Ok(max_nodes)
}

fn max_child_bytes(items: &[MacroVal]) -> Result<usize, MacroEvalError> {
    let mut max_bytes = 0usize;
    for item in items {
        max_bytes = max_bytes.max(measure_val(item)?.bytes);
    }
    Ok(max_bytes)
}

fn union_scan_count(left: usize, right: usize) -> Result<usize, MacroEvalError> {
    let cross = left
        .checked_mul(right)
        .ok_or(MacroEvalError::Limit("iteration work"))?;
    let triangular = right
        .checked_mul(right.saturating_sub(1))
        .ok_or(MacroEvalError::Limit("iteration work"))?
        / 2;
    cross
        .checked_add(triangular)
        .ok_or(MacroEvalError::Limit("iteration work"))
}

fn charge_set_scans(
    left: &[MacroVal],
    right: &[MacroVal],
    growing: bool,
    budget: &mut MacroBudget,
) -> Result<(), MacroEvalError> {
    check_quad(left.len(), right.len())?;
    let scans = if growing {
        union_scan_count(left.len(), right.len())?
    } else {
        left.len()
            .checked_mul(right.len())
            .ok_or(MacroEvalError::Limit("iteration work"))?
    };
    let pair = max_child_nodes(left)?
        .saturating_add(max_child_nodes(right)?)
        .saturating_add(max_child_bytes(left)?)
        .saturating_add(max_child_bytes(right)?)
        .max(1);
    let work = scans
        .checked_mul(pair)
        .ok_or(MacroEvalError::Limit("iteration work"))?;
    if work == 0 {
        return Ok(());
    }
    budget.spend_work(work)
}

fn check_quad(a: usize, b: usize) -> Result<(), MacroEvalError> {
    if a > RANGE_CAP || b > RANGE_CAP || a.saturating_mul(b) > SET_WORK_CAP {
        Err(MacroEvalError::Limit("collection size"))
    } else {
        Ok(())
    }
}

fn official(code: &'static str, message: impl Into<String>) -> MacroEvalError {
    MacroEvalError::Official {
        code,
        message: message.into(),
    }
}

fn mismatch(op: &'static str) -> MacroEvalError {
    official(
        "E285",
        format!("Type mismatch for operands of {op} operator"),
    )
}

fn missing_op(op: &'static str) -> MacroEvalError {
    official(
        "E285",
        format!("Operator {op} does not exist for this type"),
    )
}

fn missing_op_unary(op: &'static str) -> MacroEvalError {
    official(
        "E285",
        format!("Unary operator {op} does not exist for this type"),
    )
}

fn missing_named(name: &str) -> MacroEvalError {
    official(
        "E285",
        format!("Operator `{name}` does not exist for this type"),
    )
}

fn missing_builtin(op: Builtin) -> MacroEvalError {
    let name = match op {
        Builtin::Exp => "exp",
        Builtin::Ln => "ln",
        Builtin::Log10 => "log10",
        Builtin::Sin => "sin",
        Builtin::Cos => "cos",
        Builtin::Tan => "tan",
        Builtin::Atan => "atan",
        Builtin::Acos => "acos",
        Builtin::Sqrt => "sqrt",
        Builtin::Cbrt => "cbrt",
        Builtin::Sign => "sign",
        Builtin::Floor => "floor",
        Builtin::Ceil => "ceil",
        Builtin::Trunc => "trunc",
        Builtin::Erf => "erf",
        Builtin::Erfc => "erfc",
        Builtin::Gamma => "gamma",
        Builtin::Lgamma => "lgamma",
        Builtin::Round => "round",
        Builtin::Normpdf | Builtin::Normpdf3 => "normpdf",
        Builtin::Normcdf | Builtin::Normcdf3 => "normcdf",
        _ => "operator",
    };
    missing_named(name)
}

enum PrintItem<'a> {
    Expr(&'a Expr),
    Static(&'static str),
    Slice(&'a str),
    Owned(String),
}

fn print_walk(expr: &Expr, sink: &mut dyn FnMut(&str)) {
    let mut stack = vec![PrintItem::Expr(expr)];
    while let Some(item) = stack.pop() {
        match item {
            PrintItem::Static(text) => sink(text),
            PrintItem::Slice(text) => sink(text),
            PrintItem::Owned(text) => sink(&text),
            PrintItem::Expr(expr) => push_print(&mut stack, expr),
        }
    }
}

fn push_print<'a>(stack: &mut Vec<PrintItem<'a>>, expr: &'a Expr) {
    match expr {
        Expr::Bool(true) => stack.push(PrintItem::Static("true")),
        Expr::Bool(false) => stack.push(PrintItem::Static("false")),
        Expr::Real(value) => stack.push(PrintItem::Owned(format_real(*value))),
        Expr::Text(value) => {
            stack.push(PrintItem::Static("\""));
            stack.push(PrintItem::Slice(value));
            stack.push(PrintItem::Static("\""));
        }
        Expr::Var(name) => stack.push(PrintItem::Slice(name)),
        Expr::Defined(name) => {
            stack.push(PrintItem::Static(")"));
            stack.push(PrintItem::Slice(name));
            stack.push(PrintItem::Static("defined("));
        }
        Expr::Unary(op, inner) => {
            stack.push(PrintItem::Expr(inner));
            stack.push(PrintItem::Static(match op {
                Unary::Not => "!",
                Unary::Neg => "-",
                Unary::Pos => "+",
                Unary::CastBool => "(bool)",
                Unary::CastReal => "(real)",
                Unary::CastString => "(string)",
                Unary::CastTuple => "(tuple)",
                Unary::CastArray => "(array)",
            }));
        }
        Expr::Binary(op, left, right) => match op {
            BinOp::Union => {
                stack.push(PrintItem::Static("))"));
                stack.push(PrintItem::Expr(right));
                stack.push(PrintItem::Static(", "));
                stack.push(PrintItem::Expr(left));
                stack.push(PrintItem::Static("union(("));
            }
            BinOp::Inter => {
                stack.push(PrintItem::Static("))"));
                stack.push(PrintItem::Expr(right));
                stack.push(PrintItem::Static(", "));
                stack.push(PrintItem::Expr(left));
                stack.push(PrintItem::Static("intersection(("));
            }
            other => {
                stack.push(PrintItem::Static(")"));
                stack.push(PrintItem::Expr(right));
                stack.push(PrintItem::Owned(format!(" {} ", binary_spelling(*other))));
                stack.push(PrintItem::Expr(left));
                stack.push(PrintItem::Static("("));
            }
        },
        Expr::Call { name, args } => {
            stack.push(PrintItem::Static(")"));
            push_print_list(stack, args);
            stack.push(PrintItem::Owned(format!("{name}(")));
        }
        Expr::Builtin(op, args) => {
            stack.push(PrintItem::Static(")"));
            push_print_list(stack, args);
            stack.push(PrintItem::Owned(format!("{}(", builtin_name(*op))));
        }
        Expr::Index { name, indexes } => {
            stack.push(PrintItem::Static("]"));
            push_print_list(stack, indexes);
            stack.push(PrintItem::Owned(format!("{name}[")));
        }
        Expr::Array(items) => {
            stack.push(PrintItem::Static("]"));
            push_print_list(stack, items);
            stack.push(PrintItem::Static("["));
        }
        Expr::Tuple(items) => {
            stack.push(PrintItem::Static(")"));
            push_print_list(stack, items);
            stack.push(PrintItem::Static("("));
        }
        Expr::Range { start, step, end } => {
            stack.push(PrintItem::Expr(end));
            if let Some(step) = step {
                stack.push(PrintItem::Static(":"));
                stack.push(PrintItem::Expr(step));
            }
            stack.push(PrintItem::Static(":"));
            stack.push(PrintItem::Expr(start));
        }
        Expr::Filter { vars, set, when } => {
            stack.push(PrintItem::Static("]"));
            stack.push(PrintItem::Expr(when));
            stack.push(PrintItem::Static(" when "));
            stack.push(PrintItem::Expr(set));
            stack.push(PrintItem::Static(" in "));
            stack.push(PrintItem::Expr(vars));
            stack.push(PrintItem::Static("["));
        }
        Expr::Map {
            expr,
            vars,
            set,
            when,
        } => {
            stack.push(PrintItem::Static("]"));
            if let Some(when) = when {
                stack.push(PrintItem::Expr(when));
                stack.push(PrintItem::Static(" when "));
            }
            stack.push(PrintItem::Expr(set));
            stack.push(PrintItem::Static(" in "));
            stack.push(PrintItem::Expr(vars));
            stack.push(PrintItem::Static(" for "));
            stack.push(PrintItem::Expr(expr));
            stack.push(PrintItem::Static("["));
        }
    }
}

fn push_print_list<'a>(stack: &mut Vec<PrintItem<'a>>, items: &'a [Expr]) {
    if let Some((last, rest)) = items.split_last() {
        stack.push(PrintItem::Expr(last));
        for item in rest.iter().rev() {
            stack.push(PrintItem::Static(", "));
            stack.push(PrintItem::Expr(item));
        }
    }
}

fn binary_spelling(op: BinOp) -> &'static str {
    match op {
        BinOp::Add => "+",
        BinOp::Sub => "-",
        BinOp::Mul => "*",
        BinOp::Div => "/",
        BinOp::Pow => "^",
        BinOp::Eq => "==",
        BinOp::Ne => "!=",
        BinOp::Lt => "<",
        BinOp::Gt => ">",
        BinOp::Le => "<=",
        BinOp::Ge => ">=",
        BinOp::And => "&&",
        BinOp::Or => "||",
        BinOp::In => "in",
        BinOp::Union => "|",
        BinOp::Inter => "&",
    }
}

fn builtin_name(op: Builtin) -> &'static str {
    match op {
        Builtin::Length => "length",
        Builtin::IsEmpty => "isempty",
        Builtin::IsBoolean => "isboolean",
        Builtin::IsReal => "isreal",
        Builtin::IsString => "isstring",
        Builtin::IsTuple => "istuple",
        Builtin::IsArray => "isarray",
        Builtin::Exp => "exp",
        Builtin::Ln => "ln",
        Builtin::Log10 => "log10",
        Builtin::Sin => "sin",
        Builtin::Cos => "cos",
        Builtin::Tan => "tan",
        Builtin::Atan => "atan",
        Builtin::Acos => "acos",
        Builtin::Sqrt => "sqrt",
        Builtin::Cbrt => "cbrt",
        Builtin::Sign => "sign",
        Builtin::Floor => "floor",
        Builtin::Ceil => "ceil",
        Builtin::Trunc => "trunc",
        Builtin::Sum => "sum",
        Builtin::Erf => "erf",
        Builtin::Erfc => "erfc",
        Builtin::Gamma => "gamma",
        Builtin::Lgamma => "lgamma",
        Builtin::Round => "round",
        Builtin::Normpdf | Builtin::Normpdf3 => "normpdf",
        Builtin::Normcdf | Builtin::Normcdf3 => "normcdf",
        Builtin::Max => "max",
        Builtin::Min => "min",
        Builtin::Mod => "mod",
    }
}

// Colon is not an ordinary infix operator: `A:B` and `A:INC:B` stop at
// operands tighter than `|`. The parser above currently returns None for `:`,
// so install it by rewriting parse_bp. The function below is the real layer
// and is called from a patched parse_bp at the bottom of this module.

fn parse_colon(lexer: &mut Lexer<'_>, left: Expr) -> Result<Expr, MacroEvalError> {
    lexer.bump();
    let second = lexer.parse_bp_with_colon(70)?;
    if lexer.eat(":") {
        let third = lexer.parse_bp_with_colon(70)?;
        if matches!(&lexer.peek, Tok::Word(":", _)) {
            return Err(MacroEvalError::SyntaxUnexpected("COLON"));
        }
        fit1(&left)?;
        fit1(&second)?;
        fit1(&third)?;
        return Ok(Expr::Range {
            start: Box::new(left),
            step: Some(Box::new(second)),
            end: Box::new(third),
        });
    }
    fit1(&left)?;
    fit1(&second)?;
    Ok(Expr::Range {
        start: Box::new(left),
        step: None,
        end: Box::new(second),
    })
}

impl<'a> Lexer<'a> {
    fn parse_bp_with_colon(&mut self, min: u8) -> Result<Expr, MacroEvalError> {
        self.enter()?;
        let result = self.parse_bp_limited(min);
        self.depth -= 1;
        result
    }

    fn parse_bp_limited(&mut self, min: u8) -> Result<Expr, MacroEvalError> {
        let mut left = self.parse_prefix()?;
        loop {
            if min <= 60 {
                if let Tok::Word(":", _) = &self.peek {
                    if 60 < min {
                        break;
                    }
                    left = parse_colon(self, left)?;
                    continue;
                }
            }
            let Some((op, lbp, nonassoc)) = infix_binding(&self.peek) else {
                break;
            };
            if lbp < min {
                break;
            }
            if nonassoc && lbp == min {
                return Err(self.or_fail(MacroEvalError::SyntaxUnexpected(self.peek.name())));
            }
            self.bump();
            if self.peek == Tok::End {
                return Err(self.or_fail(MacroEvalError::SyntaxUnexpected(self.peek.name())));
            }
            let right_min = if nonassoc { lbp } else { lbp + 1 };
            let right = self.parse_bp_with_colon(right_min)?;
            fit2(&left, &right)?;
            left = Expr::Binary(op, Box::new(left), Box::new(right));
        }
        Ok(left)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn replay_round_trip(value: &MacroVal) -> MacroVal {
        let encoded = encode_replay_value(value, &mut MacroBudget::new()).unwrap();
        decode_replay_value(&encoded, &mut MacroBudget::new()).unwrap()
    }

    #[test]
    fn private_replay_preserves_value_bits_and_collection_shape() {
        let number = 1.0_f64 / 3.0;
        let MacroVal::Real(round_trip) = replay_round_trip(&MacroVal::Real(number)) else {
            panic!("real type");
        };
        assert_eq!(round_trip.to_bits(), number.to_bits());
        // Public display is deliberately lossy; replay is not parsed from it.
        assert_ne!(
            format_real(number).parse::<f64>().unwrap().to_bits(),
            number.to_bits()
        );
        for value in [
            0.0_f64,
            -0.0,
            f64::INFINITY,
            f64::from_bits(0x7ff8_0000_0000_0042),
        ] {
            let MacroVal::Real(round_trip) = replay_round_trip(&MacroVal::Real(value)) else {
                panic!("real type");
            };
            assert_eq!(round_trip.to_bits(), value.to_bits());
        }
        let tuple = MacroVal::Tuple(vec![MacroVal::Real(1.0)]);
        assert!(matches!(replay_round_trip(&tuple), MacroVal::Tuple(items) if items.len()==1));
        let input = MacroVal::Tuple(vec![MacroVal::Real(1.0), MacroVal::Real(2.0)]);
        assert!(values_equal(&replay_round_trip(&input), &input));
        let bytes = MacroVal::Bytes(vec![0xc3]);
        let decoded = replay_round_trip(&bytes);
        assert!(matches!(&decoded, MacroVal::Bytes(value) if value == &[0xc3]));
        assert_eq!(interpolate(&length_of(&decoded).unwrap()), "1");
        let text = MacroVal::Text("quoted \"text\"\\\n\t\u{0}".into());
        assert!(values_equal(&replay_round_trip(&text), &text));
        let array = MacroVal::Array(vec![
            MacroVal::Array(Vec::new()),
            tuple,
            input,
            MacroVal::Bool(true),
            MacroVal::Int(-9),
        ]);
        assert!(values_equal(&replay_round_trip(&array), &array));
    }

    #[test]
    fn flat_replay_handles_supported_depth_without_json_recursion() {
        let mut value = MacroVal::Real(1.0);
        for _ in 1..VALUE_DEPTH_CAP {
            value = MacroVal::Tuple(vec![value]);
        }
        let decoded = replay_round_trip(&value);
        assert!(values_equal(&decoded, &value));
        let mut too_deep = Vec::new();
        for _ in 0..VALUE_DEPTH_CAP {
            too_deep.push(ReplayAtom::Tuple(1));
        }
        too_deep.push(ReplayAtom::RealBits(1.0_f64.to_bits()));
        let encoded = serde_json::to_string(&too_deep).unwrap();
        assert!(matches!(
            decode_replay_value(&encoded, &mut MacroBudget::new()),
            Err(MacroEvalError::Limit("value depth"))
        ));
    }

    #[test]
    fn replay_checks_work_and_counts_before_value_allocation() {
        let mut budget = MacroBudget::new();
        budget.spend_work(MACRO_WORK_CAP - 1).unwrap();
        assert!(matches!(
            encode_replay_value(&MacroVal::Text("text".into()), &mut budget),
            Err(MacroEvalError::Limit("iteration work"))
        ));
        let mut budget = MacroBudget::new();
        budget.spend_work(MACRO_WORK_CAP - 1).unwrap();
        assert!(matches!(
            decode_replay_value("[{\"Bool\":true}]", &mut budget),
            Err(MacroEvalError::Limit("iteration work"))
        ));
        let invalid = "{invalid}";
        let mut budget = MacroBudget::new();
        budget
            .spend_work(MACRO_WORK_CAP - invalid.len() - 1)
            .unwrap();
        // Input-byte charging alone would reach serde's syntax error. The
        // allocation guard must stop before serde examines malformed input.
        assert!(matches!(
            decode_replay_value(invalid, &mut budget),
            Err(MacroEvalError::Limit("iteration work"))
        ));
        for malformed in [
            "[]",
            "[{\"Tuple\":10001}]",
            "[{\"Array\":1}]",
            "[{\"Bool\":true},{\"Bool\":false}]",
        ] {
            assert!(
                matches!(
                    decode_replay_value(malformed, &mut MacroBudget::new()),
                    Err(MacroEvalError::Limit("source mapping"))
                ),
                "{malformed}"
            );
        }
    }

    #[test]
    fn expression_context_preserves_pinned_tokens() {
        assert_eq!(printed("1 // trailing directive comment"), "1");
        assert_eq!(printed("1 + \\\\ // continuation comment\n2"), "3");
        assert_eq!(printed("\"a\\\\\nb\""), "a\\\\\nb");
        let mut defines = HashMap::new();
        let mut budget = MacroBudget::new();
        let value =
            eval_macro_expr_interpolation_budget("1 +\r\n2", &mut defines, &mut budget).unwrap();
        assert_eq!(interpolate(&value), "3");
        for source in ["1 //2", "1 + \\\\ \n2"] {
            assert!(
                matches!(
                    eval_macro_expr_interpolation_budget(
                        source,
                        &mut defines,
                        &mut MacroBudget::new()
                    ),
                    Err(MacroEvalError::SyntaxUnexpected(_))
                ),
                "{source}"
            );
        }
        for source in ["1\u{a0}+2", "\u{a0}1", "1\u{a0}"] {
            assert!(
                matches!(
                    eval_macro_expr(source, &mut defines),
                    Err(MacroEvalError::SyntaxUnexpected("TEXT"))
                ),
                "{source}"
            );
            assert!(
                check_macro_syntax_interpolation_budget(source, &mut MacroBudget::new()).is_err(),
                "{source}"
            );
        }
        assert!(matches!(
            check_macro_syntax("length(,1)"),
            Err(MacroEvalError::SyntaxUnexpected("COMMA"))
        ));
    }

    #[test]
    fn directive_validation_uses_expression_ast_without_evaluation() {
        for source in [
            "flag",
            "x=missing",
            "f()=missing",
            "f(x,y)=x+y",
            "f(x)=x+\\\\\n1",
        ] {
            check_macro_definition_budget(source, &mut MacroBudget::new()).unwrap();
        }
        for source in ["true", "x=1+", "f(,x)=1", "f(x)", "x[1]=2"] {
            assert!(
                check_macro_definition_budget(source, &mut MacroBudget::new()).is_err(),
                "{source}"
            );
        }
        assert_eq!(
            check_macro_for_header_budget("(i,j) in missing when unknown", &mut MacroBudget::new())
                .unwrap(),
            ["i", "j"]
        );
        assert_eq!(
            check_macro_for_header_budget("a[missing] in []", &mut MacroBudget::new()).unwrap(),
            ["a"]
        );
        assert!(
            check_macro_for_header_budget("() in [()]", &mut MacroBudget::new())
                .unwrap()
                .is_empty()
        );
        assert!(check_macro_for_header_budget("(a,1) in []", &mut MacroBudget::new()).is_err());
        assert_eq!(
            macro_condition_variable_budget("a[missing]", &mut MacroBudget::new()).unwrap(),
            Some("a".into())
        );
        assert_eq!(
            macro_condition_variable_budget("1+1", &mut MacroBudget::new()).unwrap(),
            None
        );
    }

    fn val(source: &str) -> MacroVal {
        eval_macro_expr(source, &mut HashMap::new()).unwrap_or_else(|err| {
            panic!("{source}: {err:?}");
        })
    }

    fn printed(source: &str) -> String {
        interpolate(&val(source))
    }

    fn refused(source: &str) -> String {
        match eval_macro_expr(source, &mut HashMap::new()) {
            Ok(value) => panic!("{source} produced {}", interpolate(&value)),
            Err(MacroEvalError::Official { code, message }) => format!("{code}: {message}"),
            Err(MacroEvalError::Limit(name)) => format!("limit: {name}"),
            Err(MacroEvalError::SyntaxUnexpected(token)) => format!("syntax: {token}"),
            Err(err) => panic!("{source}: {err:?}"),
        }
    }

    fn defined(
        source: &str,
        defines: &[(&str, MacroVal)],
    ) -> (MacroVal, HashMap<String, MacroVal>) {
        let mut map = HashMap::new();
        for (name, value) in defines {
            map.insert((*name).to_string(), value.clone());
        }
        let value = eval_macro_expr(source, &mut map).unwrap_or_else(|err| {
            panic!("{source}: {err:?}");
        });
        (value, map)
    }

    #[test]
    fn format_real_keeps_magnitude_and_fifteen_digits() {
        assert_eq!(format_real(0.9999999999999999), "1");
        assert_eq!(format_real(9.999999999999998), "10");
        assert_eq!(format_real(0.0), "0");
        assert_eq!(format_real(-0.0), "-0");
        assert_eq!(format_real(f64::INFINITY), "inf");
        assert_eq!(format_real(f64::NEG_INFINITY), "-inf");
        assert_eq!(format_real(f64::NAN), "nan");
        assert_eq!(printed("1e-4"), "0.0001");
        assert_eq!(printed("1e-5"), "1e-05");
        assert_eq!(printed("1e14"), "100000000000000");
        assert_eq!(printed("1e15"), "1e+15");
        assert_eq!(printed("-1e-5"), "-1e-05");
        assert_eq!(printed("0.1+0.2"), "0.3");
        assert_eq!(printed("1/3"), "0.333333333333333");
        assert_eq!(printed("2/3"), "0.666666666666667");
        assert_eq!(printed("(string)(1/3)"), "0.333333333333333");
        assert_eq!(printed("1e309"), "inf");
        assert_eq!(printed("1e-308"), "1e-308");
    }

    #[test]
    fn casts_match_stod_and_predicates() {
        let accept = [
            (r#"(real)"inf""#, "inf"),
            (r#"(real)"INF""#, "inf"),
            (r#"(real)"+inf""#, "inf"),
            (r#"(real)"-inf""#, "-inf"),
            (r#"(real)"infinity""#, "inf"),
            (r#"(real)"InFiNiTy""#, "inf"),
            (r#"(real)"nan""#, "nan"),
            (r#"(real)"NaN""#, "nan"),
            (r#"(real)"nan()""#, "nan"),
            (r#"(real)"nan(ind)""#, "nan"),
            (r#"(real)"0x10""#, "16"),
            (r#"(real)"0x1.8p1""#, "3"),
            (r#"(real)"0x1p+1""#, "2"),
            (r#"(real)"0X1P-1""#, "0.5"),
            (r#"(real)"-0x10""#, "-16"),
            ("(real)\"\t2\"", "2"),
            (r#"(real)"+2""#, "2"),
            (r#"(real)"-2""#, "-2"),
            (r#"(real)".5""#, "0.5"),
            (r#"(real)"1.""#, "1"),
            (r#"(real)"-0""#, "-0"),
            (r#"(real)"  1.5""#, "1.5"),
            (r#"(real)"1e308""#, "1e+308"),
            (r#"(real)"1e-307""#, "1e-307"),
            (r#"(real)"2.23e-308""#, "2.23e-308"),
            (r#"(real)"0e999""#, "0"),
            (r#"(real)"0e-999""#, "0"),
            ("(real)true", "1"),
            ("(real)[3]", "3"),
            ("(bool)2", "true"),
            ("(bool)0", "false"),
            ("(bool)(-0)", "false"),
            (r#"(bool)"inf""#, "true"),
            (r#"(bool)"0""#, "false"),
            (r#"(bool)"TRUE""#, "true"),
            (r#"(bool)"False""#, "false"),
            (r#"(bool)"0x0""#, "false"),
            (r#"(bool)"  0x10""#, "true"),
            ("(string)[]", "[]"),
            ("(tuple)1", "(1)"),
            ("(tuple)[1,2]", "(1, 2)"),
            ("(array)(1,2)", "[1, 2]"),
            ("(array)1", "[1]"),
            ("isboolean(true)", "true"),
            ("isboolean(1)", "false"),
            ("isreal(1)", "true"),
            ("isreal(true)", "false"),
            (r#"isstring("a")"#, "true"),
            ("isstring(1)", "false"),
            ("istuple((1,2))", "true"),
            ("istuple((tuple)1)", "true"),
            ("istuple((1))", "false"),
            ("istuple([1])", "false"),
            ("isarray([1])", "true"),
            ("isarray((1))", "false"),
            ("isempty([])", "true"),
            ("isempty([1])", "false"),
            (r#"isempty("")"#, "true"),
            ("length([1,2,3])", "3"),
            ("sum([1,2,3])", "6"),
        ];
        for (source, expect) in accept {
            assert_eq!(printed(source), expect, "{source}");
        }
        let reject = [
            (r#"(real)"""#, r#"E285: "" cannot be converted to a real"#),
            (
                r#"(real)"abc""#,
                r#"E285: "abc" cannot be converted to a real"#,
            ),
            (
                r#"(real)"infx""#,
                r#"E285: "infx" cannot be converted to a real"#,
            ),
            (
                r#"(real)"1e""#,
                r#"E285: "1e" cannot be converted to a real"#,
            ),
            (
                r#"(real)"1e+""#,
                r#"E285: "1e+" cannot be converted to a real"#,
            ),
            (
                r#"(real)"1.2.3""#,
                r#"E285: "1.2.3" cannot be converted to a real"#,
            ),
            (
                r#"(real)"++1""#,
                r#"E285: "++1" cannot be converted to a real"#,
            ),
            (
                r#"(real)"e10""#,
                r#"E285: "e10" cannot be converted to a real"#,
            ),
            (r#"(real)".""#, r#"E285: "." cannot be converted to a real"#),
            (
                r#"(real)"0x""#,
                r#"E285: "0x" cannot be converted to a real"#,
            ),
            (
                r#"(real)"0x10g""#,
                r#"E285: "0x10g" cannot be converted to a real"#,
            ),
            (
                r#"(real)"nan(ind)x""#,
                r#"E285: "nan(ind)x" cannot be converted to a real"#,
            ),
            (
                r#"(real)"1.5 ""#,
                r#"E285: "1.5 " cannot be converted to a real"#,
            ),
            (
                r#"(real)"1e309""#,
                r#"E285: "1e309" cannot be converted to a real"#,
            ),
            (
                r#"(real)"1.8e308""#,
                r#"E285: "1.8e308" cannot be converted to a real"#,
            ),
            (
                r#"(real)"1e-400""#,
                r#"E285: "1e-400" cannot be converted to a real"#,
            ),
            (
                r#"(real)"1e-308""#,
                r#"E285: "1e-308" cannot be converted to a real"#,
            ),
            (
                r#"(bool)"abc""#,
                r#"E285: "abc" cannot be converted to a boolean"#,
            ),
            (
                r#"(bool)" 0 ""#,
                r#"E285: " 0 " cannot be converted to a boolean"#,
            ),
            (
                "(real)[1,2]",
                "E285: Array must be of size 1 to be cast to a real",
            ),
            (
                "sum([\"a\"])",
                "E285: Type mismatch for operands of in operator",
            ),
            ("1|2", "E285: Operator | does not exist for this type"),
            (
                "[1]|2",
                "E285: Arguments of the union operator (|) must be sets",
            ),
            ("1&2", "E285: Operator & does not exist for this type"),
            (
                "gamma(\"a\")",
                "E285: Operator `gamma` does not exist for this type",
            ),
            (
                "asin(\"a\")",
                "E285: Operator `atan` does not exist for this type",
            ),
            ("gamma()", "syntax: RPAREN"),
            ("gamma(1,2)", "syntax: COMMA"),
            (
                "normcdf(1,2,\"a\")",
                "E285: Type mismatch for operands of `normpdf` operator",
            ),
        ];
        for (source, expect) in reject {
            assert_eq!(refused(source), expect, "{source}");
        }
        let mut defines = HashMap::new();
        defines.insert("q".into(), MacroVal::Real(1.0));
        assert_eq!(
            interpolate(&eval_macro_expr("defined(q)", &mut defines).unwrap()),
            "true"
        );
        assert_eq!(
            interpolate(&eval_macro_expr("defined(missing)", &mut defines).unwrap()),
            "false"
        );
    }

    #[test]
    fn math_builtins_match_pinned_prints() {
        let cases = [
            ("asin(1)", "0.785398163397448"),
            ("atan(1)", "0.785398163397448"),
            ("acos(1)", "0"),
            ("sin(0)", "0"),
            ("cos(0)", "1"),
            ("tan(0)", "0"),
            ("exp(0)", "1"),
            ("ln(1)", "0"),
            ("log(1)", "0"),
            ("log10(1)", "0"),
            ("log10(0)", "-inf"),
            ("ln(0)", "-inf"),
            ("ln(-1)", "nan"),
            ("sqrt(-1)", "nan"),
            ("sqrt(4)", "2"),
            ("cbrt(8)", "2"),
            ("(-8)^(1/3)", "nan"),
            ("sign(-0)", "0"),
            ("sign(-2)", "-1"),
            ("sign(2)", "1"),
            ("round(1.5)", "2"),
            ("round(-1.5)", "-2"),
            ("round(2.5)", "3"),
            ("floor(-1.2)", "-2"),
            ("ceil(-1.2)", "-1"),
            ("trunc(-1.2)", "-1"),
            ("max(1,2)", "2"),
            ("min(1,2)", "1"),
            ("mod(5,2)", "1"),
            ("normpdf(0)", "0.398942280401433"),
            ("normcdf(-8)", "6.10622663543836e-16"),
            ("normcdf(-20)", "0"),
            ("normcdf(8)", "0.999999999999999"),
            ("normcdf(-8) > 0", "true"),
            ("normcdf(8) < 1", "true"),
            ("normcdf(-20) > 0", "false"),
            ("normpdf(0,0,1)", "0.398942280401433"),
            ("normcdf(0,0,1)", "0.5"),
            ("erf(1.5)", "0.966105146475311"),
            ("erf(-1.5)", "-0.966105146475311"),
            ("erf(2)", "0.995322265018953"),
            ("erf(0.5)", "0.520499877813047"),
            ("erfc(1.5)", "0.0338948535246893"),
            ("erfc(-1)", "1.84270079294972"),
            ("erfc(-1.5)", "1.96610514647531"),
            ("erfc(-2)", "1.99532226501895"),
            ("erfc(-0.5)", "1.52049987781305"),
            ("erfc(0.5)", "0.479500122186953"),
            ("erfc(3)", "2.20904969985854e-05"),
            ("erf(1)", "0.842700792949715"),
            ("sin(1)", "0.841470984807897"),
            ("cos(1)", "0.54030230586814"),
            ("tan(1)", "1.5574077246549"),
            ("exp(1)", "2.71828182845905"),
            ("acos(0.5)", "1.0471975511966"),
            ("sqrt(2)", "1.4142135623731"),
            ("cbrt(2)", "1.25992104989487"),
            ("atan(2)", "1.10714871779409"),
            ("normcdf(1)", "0.841344746068543"),
            ("normpdf(1)", "0.241970724519143"),
            ("gamma(-0)", "-inf"),
            ("gamma(0.1)", "9.51350769866873"),
            ("lgamma(0.1)", "2.25271265173421"),
            ("erfc(0)", "1"),
            ("gamma(0)", "inf"),
            ("gamma(0.5)", "1.77245385090552"),
            ("gamma(-0.5)", "-3.54490770181103"),
            ("gamma(-1.5)", "2.36327180120735"),
            ("gamma(5)", "24"),
            ("gamma(143)", "2.69536413788816e+245"),
            ("gamma(171)", "7.257415615308e+306"),
            ("gamma(172)", "inf"),
            ("gamma(-1)", "nan"),
            ("gamma(-2)", "nan"),
            ("lgamma(143)", "565.124881094874"),
            ("lgamma(-1)", "inf"),
            ("lgamma(0.5)", "0.5723649429247"),
            ("lgamma(-0.5)", "1.26551212348465"),
            ("lgamma(0)", "inf"),
        ];
        for (source, expect) in cases {
            assert_eq!(printed(source), expect, "{source}");
        }
    }

    #[test]
    fn indexes_follow_substr_and_keep_valid_utf8() {
        let text = [
            ("s".into(), MacroVal::Text("abc".into())),
            ("bytes".into(), MacroVal::Text("€x".into())),
            ("cafe".into(), MacroVal::Text("café".into())),
            (
                "a".into(),
                MacroVal::Array(vec![
                    MacroVal::Real(1.0),
                    MacroVal::Real(2.0),
                    MacroVal::Real(3.0),
                ]),
            ),
            ("flag".into(), MacroVal::Bool(true)),
        ];
        let mut defines = HashMap::new();
        for (name, value) in text {
            defines.insert(name, value);
        }
        let show = |source: &str, defines: &mut HashMap<String, MacroVal>| {
            interpolate(&eval_macro_expr(source, defines).unwrap())
        };
        assert_eq!(show("s[1]", &mut defines), "a");
        assert_eq!(show("s[3]", &mut defines), "c");
        assert_eq!(show("s[4]", &mut defines), "");
        assert_eq!(show("s[1,4]", &mut defines), "a");
        assert_eq!(show("s[4,4]", &mut defines), "");
        assert_eq!(show("s[]", &mut defines), "abc");
        assert_eq!(show("s[2:4]", &mut defines), "bc");
        assert_eq!(show("length(bytes)", &mut defines), "4");
        assert_eq!(show("length(bytes[1:3])", &mut defines), "3");
        assert_eq!(show("bytes[4]", &mut defines), "x");
        assert_eq!(show("length(bytes[4])", &mut defines), "1");
        assert_eq!(show("cafe[4:5]", &mut defines), "é");
        assert_eq!(show("a[2]", &mut defines), "2");
        assert_eq!(show("a[2,2]", &mut defines), "[2, 2]");
        for source in ["s[5]", "s[0]", "s[-1]", "a[4]", "a[0]"] {
            match eval_macro_expr(source, &mut defines) {
                Err(MacroEvalError::Official { message, .. }) => {
                    assert_eq!(message, "Index out of range", "{source}");
                }
                other => panic!("{source}: {other:?}"),
            }
        }
        match eval_macro_expr("s[1.5]", &mut defines) {
            Err(MacroEvalError::Official { message, .. }) => assert_eq!(
                message,
                "When indexing a variable you must pass an int or an int array"
            ),
            other => panic!("{other:?}"),
        }
        match eval_macro_expr("flag[1]", &mut defines) {
            Err(MacroEvalError::Official { message, .. }) => {
                assert_eq!(message, "You cannot index a boolean");
            }
            other => panic!("{other:?}"),
        }
        let indexed = eval_macro_expr("bytes[1]", &mut defines).expect("byte slice");
        assert!(matches!(indexed, MacroVal::Bytes(ref bytes) if bytes.len() == 1));
        match render_interpolation_budget(&indexed, &mut MacroBudget::new()) {
            Err(MacroEvalError::Limit(name)) => assert_eq!(name, "non-UTF-8 byte slice"),
            other => panic!("{other:?}"),
        }
        assert_eq!(show("length(bytes[1])", &mut defines), "1");
    }

    #[test]
    fn collections_and_comprehensions_keep_environment() {
        assert_eq!(printed("[1,2,2] | [2,3,4]"), "[1, 2, 2, 3, 4]");
        assert_eq!(printed("[1,2,3] & [3,2,4]"), "[3, 2]");
        assert_eq!(printed("[1,2,3] - [2]"), "[1, 3]");
        assert_eq!(printed("[1,2] * [3,4]"), "[(1, 3), (1, 4), (2, 3), (2, 4)]");
        assert_eq!(printed("[1]^2"), "[(1, 1)]");
        assert_eq!(printed("[1,2]^0"), "[1, 2]");
        assert_eq!(printed("[1,2]^-1"), "[1, 2]");
        assert_eq!(printed("[]^0"), "[]");
        assert_eq!(printed("1:3"), "[1, 2, 3]");
        assert_eq!(printed("1 in 1:3"), "true");
        assert_eq!(printed("1 in [1:3]"), "false");
        assert_eq!(printed("3:1"), "[]");
        assert_eq!(printed("1:0:2"), "[]");
        assert_eq!(printed("0 && missing"), "false");
        assert_eq!(printed("1 || missing"), "true");
        match eval_macro_expr("1 && missing", &mut HashMap::new()) {
            Err(MacroEvalError::UnknownVariable(name)) => assert_eq!(name, "missing"),
            other => panic!("{other:?}"),
        }
        let (value, map) = defined(
            "[a+b for (a,b) in [(1,2),(3,4)]]",
            &[("z", MacroVal::Real(9.0))],
        );
        assert_eq!(interpolate(&value), "[3, 7]");
        assert_eq!(interpolate(map.get("a").unwrap()), "3");
        assert_eq!(interpolate(map.get("b").unwrap()), "4");
        assert_eq!(interpolate(map.get("z").unwrap()), "9");
        let (filtered, map) = defined("[a for a in [1,2,3] when a < 3]", &[]);
        assert_eq!(interpolate(&filtered), "[1, 2]");
        assert_eq!(interpolate(map.get("a").unwrap()), "3");
        let (tuples, _) = defined("[(a,b) in [(1,2),(3,4)] when 1]", &[]);
        assert_eq!(interpolate(&tuples), "[(1, 2), (3, 4)]");
        let (mapped, _) = defined("[gamma(a) for a in [5, \"no\"] when isreal(a)]", &[]);
        assert_eq!(interpolate(&mapped), "[24]");
        let (nested, _) = defined("[[b for b in [a, a+1]] for a in [1,2]]", &[]);
        assert_eq!(interpolate(&nested), "[[1, 2], [2, 3]]");
        let (called, _) = defined(
            "[f(a) for a in [1,2]]",
            &[(
                "f",
                MacroVal::Function {
                    params: vec!["x".into()],
                    body: "x+1".into(),
                },
            )],
        );
        assert_eq!(interpolate(&called), "[2, 3]");
        assert_eq!(
            refused("[a for a in 1]"),
            "E285: The input set must evaluate to an array"
        );
        assert_eq!(
            refused("[a in [1] when \"a\"]"),
            "E283: The condition must evaluate to a boolean or a real"
        );
        assert_eq!(
            refused("[a for (a,b) in [1]]"),
            "E285: assigning to tuple in output expression but input expression does not contain tuples"
        );
        assert_eq!(
            refused("[(a,b) for (a,b) in [(1,2,3)]]"),
            "E284: The number of elements in the input  set tuple are not the same as the number of elements in the output expression tuple"
        );
        match eval_macro_expr("[1] & 2", &mut HashMap::new()) {
            Err(MacroEvalError::Official { message, .. }) => {
                assert_eq!(
                    message,
                    "Arguments of the intersection operator (|) must be sets"
                );
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn limits_stop_before_unbounded_work() {
        let mut budget = MacroBudget::new();
        assert!(budget.spend_work(0).is_ok());
        assert!(budget.spend_output(0).is_ok());
        assert!(matches!(
            budget.spend_output(MACRO_OUTPUT_CAP + 1),
            Err(MacroEvalError::Limit("output size"))
        ));
        let mut budget = MacroBudget::new();
        budget.spend_output(MACRO_OUTPUT_CAP).unwrap();
        assert!(matches!(
            budget.spend_output(1),
            Err(MacroEvalError::Limit("output size"))
        ));
        let mut budget = MacroBudget::new();
        budget.spend_work(MACRO_WORK_CAP).unwrap();
        assert!(matches!(
            eval_macro_expr_budget("1", &mut HashMap::new(), &mut budget),
            Err(MacroEvalError::Limit("iteration work"))
        ));
        let deep = format!("{}1{}", "(".repeat(140), ")".repeat(140));
        assert!(matches!(
            check_macro_syntax(&deep),
            Err(MacroEvalError::Limit("expression depth"))
        ));
        let wide = vec!["1"; 140].join("+");
        assert!(matches!(
            check_macro_syntax(&wide),
            Err(MacroEvalError::Limit("expression depth"))
        ));
        assert!(matches!(
            eval_macro_expr(&wide, &mut HashMap::new()),
            Err(MacroEvalError::Limit("expression depth"))
        ));
        let mut defines = HashMap::new();
        defines.insert(
            "f".into(),
            MacroVal::Function {
                params: Vec::new(),
                body: "f()".into(),
            },
        );
        assert!(matches!(
            eval_macro_expr("f()", &mut defines),
            Err(MacroEvalError::Limit("expression depth"))
        ));
        let chunk = "a".repeat(600_000);
        let mut defines = HashMap::new();
        defines.insert("a".into(), MacroVal::Text(chunk.clone()));
        defines.insert("b".into(), MacroVal::Text(chunk));
        assert!(matches!(
            eval_macro_expr("a+b", &mut defines),
            Err(MacroEvalError::Limit("iteration work"))
        ));
        let mut defines = HashMap::new();
        defines.insert(
            "f".into(),
            MacroVal::Function {
                params: Vec::new(),
                body: format!("\"{}\"", "a".repeat(STRING_CAP + 1)),
            },
        );
        assert!(matches!(
            eval_macro_expr("f()", &mut defines),
            Err(MacroEvalError::Limit("string size"))
        ));
        let row = vec![MacroVal::Real(1.0); 200];
        let mut defines = HashMap::new();
        defines.insert("p".into(), MacroVal::Array(row.clone()));
        defines.insert("q".into(), MacroVal::Array(row));
        let mut budget = MacroBudget::new();
        budget.spend_work(MACRO_WORK_CAP - 3).unwrap();
        assert!(matches!(
            eval_macro_expr_budget("p & q", &mut defines, &mut budget),
            Err(MacroEvalError::Limit("iteration work"))
        ));
        let fat = vec![MacroVal::Real(1.0); 300];
        let mut defines = HashMap::new();
        defines.insert("p".into(), MacroVal::Array(fat.clone()));
        defines.insert("q".into(), MacroVal::Array(fat));
        assert!(matches!(
            eval_macro_expr("p & q", &mut defines),
            Err(MacroEvalError::Limit("collection size"))
        ));
        assert!(matches!(
            eval_macro_expr("1:10001", &mut HashMap::new()),
            Err(MacroEvalError::Limit("range size"))
        ));
    }

    #[test]
    fn resource_bounds_stop_before_copy_and_scan() {
        let short = vec!["1"; 40].join("+");
        assert_eq!(printed(&short), "40");
        assert_eq!(print_expression("1+1").unwrap(), "(1 + 1)");
        assert_eq!(print_expression("1|2").unwrap(), "union((1, 2))");
        assert_eq!(print_expression("a[1,2]").unwrap(), "a[1, 2]");
        assert_eq!(
            print_expression("[1 for a in [2]]").unwrap(),
            "[1 for a in [2]]"
        );
        let long = vec!["1"; 160].join("+");
        for result in [
            check_macro_syntax(&long),
            eval_macro_expr(&long, &mut HashMap::new()).map(|_| ()),
            print_expression(&long).map(|_| ()),
        ] {
            assert!(matches!(
                result,
                Err(MacroEvalError::Limit("expression depth"))
            ));
        }
        assert_eq!(printed(&format!("{}1", "(real)".repeat(8))), "1");
        assert!(matches!(
            check_macro_syntax(&format!("{}1", "(real)".repeat(160))),
            Err(MacroEvalError::Limit("expression depth"))
        ));
        assert!(check_macro_syntax("f(f(1))").is_ok());
        let calls = format!("{}1{}", "f(".repeat(160), ")".repeat(160));
        assert!(matches!(
            check_macro_syntax(&calls),
            Err(MacroEvalError::Limit("expression depth"))
        ));
        assert!(check_macro_syntax("a[a[1]]").is_ok());
        let indexes = format!("{}1{}", "a[".repeat(160), "]".repeat(160));
        assert!(matches!(
            check_macro_syntax(&indexes),
            Err(MacroEvalError::Limit("expression depth"))
        ));
        assert_eq!(printed("1:2:4"), "[1, 3]");
        assert_eq!(printed("[1 for a in [2]]"), "[1]");
        let mut nested = "1".to_string();
        for _ in 0..160 {
            nested = format!("[1 for a in [{nested}]]");
        }
        assert!(matches!(
            check_macro_syntax(&nested),
            Err(MacroEvalError::Limit("expression depth"))
        ));

        assert_eq!(printed("[[1,2],[3]]"), "[[1, 2], [3]]");
        let mut defines = HashMap::new();
        defines.insert("n".into(), MacroVal::Real(1.0));
        let mut budget = MacroBudget::new();
        let mut wraps = 0usize;
        loop {
            wraps += 1;
            assert!(wraps < VALUE_DEPTH_CAP + 4, "value depth was not enforced");
            match eval_macro_expr_budget("[n]", &mut defines, &mut budget) {
                Ok(value) => {
                    defines.insert("n".into(), value);
                }
                Err(MacroEvalError::Limit("value depth")) => break,
                Err(error) => panic!("{error:?}"),
            }
        }
        assert!(wraps > 20, "small nesting was refused");
        let stored = measure_val(defines.get("n").unwrap()).unwrap();
        assert!(stored.nodes <= VALUE_DEPTH_CAP);

        let mut defines = HashMap::new();
        defines.insert("n".into(), MacroVal::Array(vec![MacroVal::Real(1.0)]));
        let mut budget = MacroBudget::new();
        budget.spend_work(MACRO_WORK_CAP - 200).unwrap();
        let mut rounds = 0usize;
        loop {
            rounds += 1;
            assert!(rounds < 12, "exponential copy was not stopped");
            match eval_macro_expr_budget("[n, n]", &mut defines, &mut budget) {
                Ok(value) => {
                    assert!(interpolate(&value).len() < 5_000);
                    defines.insert("n".into(), value);
                }
                Err(MacroEvalError::Limit("iteration work")) => break,
                Err(error) => panic!("{error:?}"),
            }
        }
        assert!(rounds > 1, "a small pair copy should succeed");
        let stored = measure_val(defines.get("n").unwrap()).unwrap();
        assert!(stored.nodes < 300);

        let mut defines = HashMap::new();
        defines.insert("a".into(), MacroVal::Text("a".repeat(STRING_CAP + 1)));
        assert!(matches!(
            eval_macro_expr("a", &mut defines),
            Err(MacroEvalError::Limit("string size"))
        ));

        assert_eq!(printed("[] | [1,2,3]"), "[1, 2, 3]");
        assert_eq!(printed("[1,1] | [1,2]"), "[1, 1, 2]");
        assert_eq!(printed("[[1]] | [[1],[2]]"), "[[1], [2]]");
        assert_eq!(printed("[] | (1:5)"), "[1, 2, 3, 4, 5]");
        assert_eq!(union_scan_count(0, 10_000).unwrap(), 49_995_000);
        let mut budget = MacroBudget::new();
        assert!(matches!(
            budget.spend_work(49_995_000),
            Err(MacroEvalError::Limit("iteration work"))
        ));
        // `|` binds tighter than `:`, so the range is grouped. 200 covers the
        // 20-element materialization. The triangular scan does not.
        let mut budget = MacroBudget::new();
        budget.spend_work(MACRO_WORK_CAP - 200).unwrap();
        assert!(matches!(
            eval_macro_expr_budget("[] | (1:20)", &mut HashMap::new(), &mut budget),
            Err(MacroEvalError::Limit("iteration work"))
        ));

        let body = format!("0 && \"{}\"", "a".repeat(400));
        let mut defines = HashMap::new();
        defines.insert(
            "f".into(),
            MacroVal::Function {
                params: Vec::new(),
                body: body.clone(),
            },
        );
        let mut budget = MacroBudget::new();
        budget
            .spend_work(MACRO_WORK_CAP - body.len() * 2 - 80)
            .unwrap();
        let mut oks = 0usize;
        for _ in 0..6 {
            match eval_macro_expr_budget("f()", &mut defines, &mut budget) {
                Ok(MacroVal::Bool(false)) => oks += 1,
                Err(MacroEvalError::Limit("iteration work")) => break,
                other => panic!("{other:?}"),
            }
        }
        assert!((1..=3).contains(&oks), "oks={oks}");
        let mut budget = MacroBudget::new();
        budget.spend_work(MACRO_WORK_CAP - body.len() - 5).unwrap();
        assert!(check_macro_syntax_budget(&body, &mut budget).is_ok());
        assert!(matches!(
            check_macro_syntax_budget(&body, &mut budget),
            Err(MacroEvalError::Limit("iteration work"))
        ));
        assert!(print_expression_budget(&body, &mut MacroBudget::new())
            .unwrap()
            .contains('a'));

        let nested = val("[[1], [2]]");
        let mut budget = MacroBudget::new();
        assert_eq!(
            render_macro_val_budget(&nested, false, &mut budget).unwrap(),
            "[[1], [2]]"
        );
        assert_eq!(
            render_macro_val_budget(&nested, true, &mut MacroBudget::new()).unwrap(),
            "{{1}, {2}}"
        );
        let mut budget = MacroBudget::new();
        budget.spend_output(MACRO_OUTPUT_CAP - 2).unwrap();
        assert!(matches!(
            render_macro_val_budget(&val("\"abcd\""), false, &mut budget),
            Err(MacroEvalError::Limit("output size"))
        ));
        let mut deep_value = MacroVal::Real(1.0);
        for _ in 0..VALUE_DEPTH_CAP {
            deep_value = MacroVal::Array(vec![deep_value]);
        }
        assert!(matches!(
            plan_macro_render(&deep_value, false),
            Err(MacroEvalError::Limit("value depth"))
        ));
    }

    #[test]
    fn dynare_7_2_value_honesty_when_present() {
        let binary =
            std::path::PathBuf::from(r"C:\dynare\7.2\preprocessor\dynare-preprocessor.exe");
        if !binary.is_file() {
            eprintln!("SKIP value honesty: Dynare 7.2 is absent");
            return;
        }
        let expressions = [
            "0.9999999999999999",
            "9.999999999999998",
            "-0",
            "1e-5",
            "1e15",
            "0.1+0.2",
            "1/3",
            "(real)\"inf\"",
            "(real)\"0x1.8p1\"",
            "(real)\"1.5 \"",
            "gamma(143)",
            "gamma(-1)",
            "gamma(0.5)",
            "lgamma(-0.5)",
            "erf(1.5)",
            "erfc(-1)",
            "erfc(0.5)",
            "sin(1)",
            "cos(1)",
            "gamma(0.1)",
            "normcdf(-8)",
            "asin(1)",
            "atan(1)",
        ];
        let mut source = String::new();
        let mut expect = Vec::new();
        for expression in expressions {
            match eval_macro_expr(expression, &mut HashMap::new()) {
                Ok(value) => {
                    source.push_str(&format!("@#echo {expression}\n"));
                    expect.push(interpolate(&value));
                }
                Err(_) => {
                    let _echo = dynare_echo(&binary, &format!("@#echo {expression}\n"));
                    assert!(
                        _echo.is_none(),
                        "{expression} refuses here but Dynare printed {_echo:?}"
                    );
                }
            }
        }
        let echoed = dynare_echo(&binary, &source).expect("Dynare success batch");
        assert_eq!(echoed, expect);
    }

    fn dynare_echo(binary: &std::path::Path, source: &str) -> Option<Vec<String>> {
        let dir = std::env::temp_dir().join("dyg-value-gaps");
        std::fs::create_dir_all(&dir).ok()?;
        let path = dir.join("batch.mod");
        std::fs::write(&path, source).ok()?;
        let mut child = std::process::Command::new(binary)
            .arg(&path)
            .arg("onlymacro")
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .ok()?;
        let started = std::time::Instant::now();
        loop {
            if started.elapsed() > std::time::Duration::from_secs(8) {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            match child.try_wait() {
                Ok(Some(status)) => {
                    let output = child.wait_with_output().ok()?;
                    if !status.success() {
                        return None;
                    }
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    return Some(
                        stdout
                            .lines()
                            .filter_map(|line| {
                                line.split_once("): ").map(|(_, value)| value.to_string())
                            })
                            .collect(),
                    );
                }
                Ok(None) => std::thread::sleep(std::time::Duration::from_millis(20)),
                Err(_) => return None,
            }
        }
    }
}
