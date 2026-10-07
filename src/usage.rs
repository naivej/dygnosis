//! Private Check usage tree. The written arena and its references stay intact.
//!
//! Dynare 7.2 DataTree constructors simplify while parsing, before collection.
//! Intern this small parallel graph once; then visit each live node once. Local
//! definitions are collection edges, not replacements of written local symbols.

use std::collections::{HashMap, HashSet};

use crate::expr::{BinOp, ExprId, ExprKind, UnOp};
use crate::intern::Name;
use crate::model::{
    Equation, Model, PacTargetComponentRow, PacTargetInfoRow, SemiStructuralKind,
    SemiStructuralValue,
};

type NodeId = usize;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum Node {
    Number(String),
    Ident(Name, i32, usize),
    Neg(NodeId),
    Binary(u8, NodeId, NodeId),
    Builtin(&'static str, Vec<NodeId>),
    Call(Name, Vec<NodeId>),
    SteadyState(NodeId),
    Expectation(i32, NodeId),
    Opaque(ExprId),
}

pub(crate) struct Usage<'a> {
    model: &'a Model,
    nodes: Vec<Node>,
    values: Vec<Option<f64>>,
    intern: HashMap<Node, NodeId>,
    normalized: HashMap<ExprId, NodeId>,
    locals: HashMap<(usize, Name), ExprId>,
    zero: NodeId,
    one: NodeId,
    minus_one: NodeId,
}

impl<'a> Usage<'a> {
    pub(crate) fn new(model: &'a Model) -> Self {
        // AddLocalVariable belongs to one DataTree. A heterogeneous body may
        // define the same local name as the aggregate model with another RHS.
        let mut locals = HashMap::new();
        let mut contexts = HashMap::new();
        let trees = std::iter::once(model.equations.as_slice()).chain(
            model
                .heterogeneous_models
                .iter()
                .map(|block| block.equations.as_slice()),
        );
        for (context, equations) in trees.enumerate() {
            for eq in equations {
                if eq.is_local {
                    if let (Some(lhs), Some(rhs)) = (eq.lhs_expr, eq.rhs_expr) {
                        if let ExprKind::Ident { name, .. } = model.exprs.get(lhs).kind {
                            locals.insert((context, name), rhs);
                        }
                    }
                }
                let mut pending: Vec<_> =
                    [eq.lhs_expr, eq.rhs_expr].into_iter().flatten().collect();
                while let Some(id) = pending.pop() {
                    if contexts.insert(id, context).is_some() {
                        continue;
                    }
                    match &model.exprs.get(id).kind {
                        ExprKind::Unary { arg, .. }
                        | ExprKind::SteadyState { arg }
                        | ExprKind::Expectation { arg, .. } => pending.push(*arg),
                        ExprKind::Binary { lhs, rhs, .. } => pending.extend([*lhs, *rhs]),
                        ExprKind::Call { args, .. } => pending.extend(args),
                        _ => {}
                    }
                }
            }
        }
        let mut usage = Self {
            model,
            nodes: Vec::new(),
            values: Vec::new(),
            intern: HashMap::new(),
            normalized: HashMap::new(),
            locals,
            zero: 0,
            one: 0,
            minus_one: 0,
        };
        usage.zero = usage.intern(Node::Number("0".into()), Some(0.0));
        usage.one = usage.intern(Node::Number("1".into()), Some(1.0));
        usage.minus_one = usage.intern(Node::Neg(usage.one), Some(-1.0));
        // The arena allocates children first. No recursive AST walk is needed,
        // and normalization work is bounded by the written arena size.
        for (id, expr) in model.exprs.iter() {
            let normalized = match &expr.kind {
                ExprKind::Number => {
                    let text = model
                        .numeric_literal_texts
                        .get(&id)
                        .cloned()
                        .unwrap_or_else(|| {
                            model.source[expr.span.start as usize..expr.span.end as usize]
                                .to_string()
                        });
                    let value = model
                        .numeric_literals
                        .get(&id)
                        .copied()
                        .or_else(|| text.parse().ok());
                    usage.intern(Node::Number(text), value)
                }
                ExprKind::Ident { name, timing, .. } => {
                    let context = contexts.get(&id).copied().unwrap_or(0);
                    let value = usage
                        .locals
                        .get(&(context, *name))
                        .and_then(|rhs| usage.normalized.get(rhs))
                        .and_then(|n| usage.values[*n]);
                    usage.intern(Node::Ident(*name, *timing, context), value)
                }
                ExprKind::Unary { op, arg } => {
                    let arg = usage.normalized[arg];
                    match op {
                        UnOp::Pos => arg,
                        UnOp::Neg => usage.neg(arg),
                    }
                }
                ExprKind::Binary { op, lhs, rhs } => {
                    usage.binary(*op, usage.normalized[lhs], usage.normalized[rhs])
                }
                ExprKind::Call { callee, args } => {
                    let args: Vec<_> = args.iter().map(|id| usage.normalized[id]).collect();
                    if let Some(builtin) = builtin(model.name(*callee)) {
                        usage.builtin(builtin, args)
                    } else {
                        usage.intern(Node::Call(*callee, args), None)
                    }
                }
                ExprKind::SteadyState { arg } => {
                    let arg = usage.normalized[arg];
                    if let Some(value) = usage.values[arg] {
                        usage.constant(value)
                    } else {
                        usage.intern(Node::SteadyState(arg), None)
                    }
                }
                ExprKind::Expectation { shift, arg } => {
                    usage.intern(Node::Expectation(*shift, usage.normalized[arg]), None)
                }
                ExprKind::String | ExprKind::Error => usage.intern(Node::Opaque(id), None),
            };
            usage.normalized.insert(id, normalized);
        }
        usage
    }

    fn intern(&mut self, node: Node, value: Option<f64>) -> NodeId {
        if let Some(&id) = self.intern.get(&node) {
            if self.values[id].is_none() {
                self.values[id] = value;
            }
            return id;
        }
        let id = self.nodes.len();
        self.nodes.push(node.clone());
        self.values.push(value);
        self.intern.insert(node, id);
        id
    }

    fn constant(&mut self, value: f64) -> NodeId {
        if value < 0.0 {
            let positive = self.constant(-value);
            return self.intern(Node::Neg(positive), Some(value));
        }
        // AddPossiblyNegativeConstant uses sixteen significant digits. Literal
        // constants keep their spelling: `0.0` is not DataTree::Zero.
        let text = constant_text(value);
        let rounded = text.parse().ok().unwrap_or(value);
        self.intern(Node::Number(text), Some(rounded))
    }

    fn neg(&mut self, arg: NodeId) -> NodeId {
        if arg == self.zero {
            return self.zero;
        }
        if let Node::Neg(inner) = self.nodes[arg] {
            return inner;
        }
        // AddUnaryOp skips evaluation for a negative numeric literal.
        if !matches!(self.nodes[arg], Node::Number(_)) {
            if let Some(value) = self.values[arg] {
                return self.constant(-value);
            }
        }
        self.intern(Node::Neg(arg), self.values[arg].map(|v| -v))
    }

    fn pair(&self, node: NodeId, op: BinOp) -> Option<(NodeId, NodeId)> {
        match self.nodes[node] {
            Node::Binary(code, lhs, rhs) if code == op as u8 => Some((lhs, rhs)),
            _ => None,
        }
    }

    fn binary(&mut self, op: BinOp, mut lhs: NodeId, mut rhs: NodeId) -> NodeId {
        match op {
            BinOp::Add => {
                if rhs == self.zero {
                    return lhs;
                }
                if lhs == self.zero {
                    return rhs;
                }
                if let Node::Neg(arg) = self.nodes[rhs] {
                    return self.binary(BinOp::Sub, lhs, arg);
                }
                if let Node::Neg(arg) = self.nodes[lhs] {
                    return self.binary(BinOp::Sub, rhs, arg);
                }
                if let Some((x, y)) = self.pair(lhs, BinOp::Sub) {
                    if y == rhs {
                        return x;
                    }
                }
                if let Some((x, y)) = self.pair(rhs, BinOp::Sub) {
                    if y == lhs {
                        return x;
                    }
                }
            }
            BinOp::Sub => {
                if rhs == self.zero {
                    return lhs;
                }
                if lhs == self.zero {
                    return self.neg(rhs);
                }
                if lhs == rhs {
                    return self.zero;
                }
                if let Node::Neg(arg) = self.nodes[rhs] {
                    return self.binary(BinOp::Add, lhs, arg);
                }
                if let Some((x, y)) = self.pair(lhs, BinOp::Add) {
                    if y == rhs {
                        return x;
                    }
                    if x == rhs {
                        return y;
                    }
                }
            }
            BinOp::Mul => {
                if lhs == self.zero || rhs == self.zero {
                    return self.zero;
                }
                if lhs == self.one {
                    return rhs;
                }
                if rhs == self.one {
                    return lhs;
                }
                if lhs == self.minus_one {
                    return self.neg(rhs);
                }
                if rhs == self.minus_one {
                    return self.neg(lhs);
                }
                if let Some((x, y)) = self.pair(lhs, BinOp::Div) {
                    if y == rhs {
                        return x;
                    }
                }
                if let Some((x, y)) = self.pair(rhs, BinOp::Div) {
                    if y == lhs {
                        return x;
                    }
                }
            }
            BinOp::Div => {
                if rhs == self.one {
                    return lhs;
                }
                // A refused denominator must never be hidden by 0/x or x/x.
                if rhs == self.zero {
                    return self.intern(Node::Binary(op as u8, lhs, rhs), None);
                }
                if lhs == self.zero {
                    return self.zero;
                }
                if lhs == rhs {
                    return self.one;
                }
                if let Some((x, y)) = self.pair(rhs, BinOp::Div) {
                    if x == self.one {
                        return self.binary(BinOp::Mul, lhs, y);
                    }
                }
                if let Some((x, y)) = self.pair(lhs, BinOp::Mul) {
                    if y == rhs {
                        return x;
                    }
                    if x == rhs {
                        return y;
                    }
                }
            }
            BinOp::Pow => {
                if rhs == self.zero {
                    return self.one;
                }
                if lhs == self.zero {
                    return self.zero;
                }
                if lhs == self.one {
                    return self.one;
                }
                if rhs == self.one {
                    return lhs;
                }
            }
            _ => {}
        }
        if matches!(op, BinOp::Add | BinOp::Mul) && lhs > rhs {
            std::mem::swap(&mut lhs, &mut rhs);
        }
        if let (Some(a), Some(b)) = (self.values[lhs], self.values[rhs]) {
            let value = match op {
                BinOp::Add => a + b,
                BinOp::Sub => a - b,
                BinOp::Mul => a * b,
                BinOp::Div => a / b,
                BinOp::Pow => a.powf(b),
                BinOp::Lt => (a < b) as u8 as f64,
                BinOp::Gt => (a > b) as u8 as f64,
                BinOp::Le => (a <= b) as u8 as f64,
                BinOp::Ge => (a >= b) as u8 as f64,
                BinOp::EqEq => (a == b) as u8 as f64,
                BinOp::Ne => (a != b) as u8 as f64,
            };
            return self.constant(value);
        }
        self.intern(Node::Binary(op as u8, lhs, rhs), None)
    }

    fn builtin(&mut self, callee: &'static str, mut args: Vec<NodeId>) -> NodeId {
        // ParsingDriver fills the one-argument normal distribution forms with
        // the canonical Zero and One nodes, not written decimal constants.
        if matches!(callee, "normcdf" | "normpdf") && args.len() == 1 {
            args.extend([self.zero, self.one]);
        }
        let mut negative = false;
        if matches!(callee, "log" | "log10") && args.len() == 1 {
            // AddLog/AddLog10 turn log(1/x) into -log(x). Each step consumes
            // one normalized division node, so deeply nested reciprocals do
            // not add a recursive collector walk.
            while let Some((numerator, denominator)) = self.pair(args[0], BinOp::Div) {
                if numerator != self.one {
                    break;
                }
                args[0] = denominator;
                negative = !negative;
            }
        }
        let node = if let Some(value) = self.call_value(callee, &args) {
            self.constant(value)
        } else {
            self.intern(Node::Builtin(callee, args), None)
        };
        if negative {
            self.neg(node)
        } else {
            node
        }
    }

    fn call_value(&self, callee: &str, args: &[NodeId]) -> Option<f64> {
        let value = self.values[*args.first()?]?;
        if matches!(callee, "normcdf" | "normpdf") && args.len() == 3 {
            let (mean, deviation) = (self.values[args[1]]?, self.values[args[2]]?);
            let standardized = (value - mean) / deviation;
            return Some(if callee == "normcdf" {
                0.5 * (1.0 + libm::erf(standardized / std::f64::consts::SQRT_2))
            } else {
                1.0 / (deviation
                    * (2.0 * std::f64::consts::PI).sqrt()
                    * (standardized.powi(2) / 2.0).exp())
            });
        }
        if args.len() == 2 {
            let other = self.values[args[1]]?;
            return match callee {
                "max" => Some(if value < other { other } else { value }),
                "min" => Some(if value > other { other } else { value }),
                _ => None,
            };
        }
        if args.len() != 1 {
            return None;
        }
        Some(match callee {
            "exp" => value.exp(),
            "log" => value.ln(),
            "log10" => value.log10(),
            "cos" => value.cos(),
            "sin" => value.sin(),
            "tan" => value.tan(),
            "acos" => value.acos(),
            "asin" => value.asin(),
            "atan" => value.atan(),
            "cosh" => value.cosh(),
            "sinh" => value.sinh(),
            "tanh" => value.tanh(),
            "acosh" => value.acosh(),
            "asinh" => value.asinh(),
            "atanh" => value.atanh(),
            "sqrt" => value.sqrt(),
            "cbrt" => value.cbrt(),
            "abs" => value.abs(),
            "erf" => libm::erf(value),
            "erfc" => libm::erfc(value),
            "sign" => {
                if value > 0.0 {
                    1.0
                } else if value < 0.0 {
                    -1.0
                } else {
                    0.0
                }
            }
            _ => return None,
        })
    }

    pub(crate) fn model_names(&self) -> HashSet<Name> {
        self.names(model_roots(self.model))
    }

    pub(crate) fn exogenous_exemptions(&self) -> HashSet<Name> {
        let command_growth = self
            .model
            .semi_structural_commands
            .iter()
            .filter(|command| command.kind == SemiStructuralKind::PacModel)
            .flat_map(|command| &command.options)
            .filter_map(|option| match &option.value {
                SemiStructuralValue::Expression(expression)
                    if option.name.eq_ignore_ascii_case("growth") =>
                {
                    expression.expr
                }
                _ => None,
            });
        let component_growth = self
            .model
            .pac_target_info
            .iter()
            .flat_map(|block| &block.rows)
            .filter_map(|row| match row {
                PacTargetInfoRow::Component(component) => Some(component),
                _ => None,
            })
            .flat_map(|component| &component.rows)
            .filter_map(|row| match row {
                PacTargetComponentRow::Growth(expression) => expression.expr,
                _ => None,
            });
        let mut names = self.names(command_growth.chain(component_growth));
        names.extend(self.model.varexobs.iter().map(|observed| observed.name));
        names
    }

    pub(crate) fn parameter_names(&self) -> HashSet<Name> {
        let mut names = self.names(
            model_roots(self.model).chain(
                self.model
                    .steady_state_equations
                    .iter()
                    .filter_map(|eq| eq.rhs_expr),
            ),
        );
        for equation in &self.model.steady_state_equations {
            names.extend(
                equation
                    .steady_state_targets
                    .iter()
                    .map(|target| target.name),
            );
        }
        names
    }

    pub(crate) fn names(&self, roots: impl IntoIterator<Item = ExprId>) -> HashSet<Name> {
        let mut names = HashSet::new();
        let mut seen = HashSet::new();
        let mut pending: Vec<_> = roots
            .into_iter()
            .filter_map(|id| self.normalized.get(&id).copied())
            .collect();
        while let Some(id) = pending.pop() {
            if !seen.insert(id) {
                continue;
            }
            match &self.nodes[id] {
                Node::Ident(name, _, context) => {
                    names.insert(*name);
                    if let Some(definition) = self
                        .locals
                        .get(&(*context, *name))
                        .and_then(|expr| self.normalized.get(expr))
                    {
                        pending.push(*definition);
                    }
                }
                Node::Neg(arg) | Node::SteadyState(arg) | Node::Expectation(_, arg) => {
                    pending.push(*arg)
                }
                Node::Binary(_, lhs, rhs) => pending.extend([*lhs, *rhs]),
                Node::Builtin(_, args) | Node::Call(_, args) => pending.extend(args),
                Node::Number(_) | Node::Opaque(_) => {}
            }
        }
        names
    }
}

/// DynareFlex recognizes these tokens without case, while an identifier or an
/// external function keeps its exact spelling. LN has the LOG semantic action.
fn builtin(name: &str) -> Option<&'static str> {
    if name.eq_ignore_ascii_case("ln") {
        return Some("log");
    }
    const BUILTINS: &[&str] = &[
        "exp", "log", "log10", "sin", "cos", "tan", "asin", "acos", "atan", "sinh", "cosh", "tanh",
        "asinh", "acosh", "atanh", "sqrt", "cbrt", "max", "min", "abs", "sign", "normcdf",
        "normpdf", "erf", "erfc", "diff", "sum",
    ];
    BUILTINS
        .iter()
        .find(|builtin| name.eq_ignore_ascii_case(builtin))
        .copied()
}

fn model_roots(model: &Model) -> impl Iterator<Item = ExprId> + '_ {
    model
        .equations
        .iter()
        .chain(model.heterogeneous_models.iter().flat_map(|b| &b.equations))
        .filter(|eq| !eq.is_local)
        .flat_map(|eq: &Equation| [eq.lhs_expr, eq.rhs_expr].into_iter().flatten())
}

fn constant_text(value: f64) -> String {
    if value.is_nan() {
        return "NaN".into();
    }
    if value.is_infinite() {
        return "Inf".into();
    }
    if value == 0.0 {
        return "0".into();
    }
    let scientific = format!("{value:.15e}");
    let (mantissa, exponent) = scientific.split_once('e').unwrap();
    let exponent: i32 = exponent.parse().unwrap();
    let digits = mantissa.replace('.', "").trim_end_matches('0').to_string();
    if !(-4..16).contains(&exponent) {
        let mantissa = mantissa.trim_end_matches('0').trim_end_matches('.');
        return format!("{mantissa}e{exponent:+03}");
    }
    let point = exponent + 1;
    if point <= 0 {
        return format!("0.{}{digits}", "0".repeat((-point) as usize));
    }
    let point = point as usize;
    if point >= digits.len() {
        return format!("{digits}{}", "0".repeat(point - digits.len()));
    }
    format!("{}.{}", &digits[..point], &digits[point..])
}
