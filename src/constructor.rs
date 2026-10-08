//! Private pinned Parse constructor identities, evaluation, and display.
//! Callers own execution order, diagnostic ranges, and collection roots.

use crate::expr::{BinOp, ExprId, ExprKind, UnOp};
use crate::intern::Name;
use std::collections::{HashMap, HashSet};

pub(crate) type NodeId = usize;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum DataTreeScope {
    General,
    ShockPaths,
    Dynamic,
    SteadyState,
    Planner(usize),
    Epilogue,
    Occbin(usize),
    Heterogeneous(Name),
}

/// The caller's current data tree and already-known local evaluation value.
pub(crate) struct ConstructorContext {
    pub(crate) scope: DataTreeScope,
    pub(crate) ident_value: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Node {
    Number(String),
    Ident(Name, i32, DataTreeScope),
    PathNamespace(PathNamespace),
    Neg(NodeId),
    Binary(u8, NodeId, NodeId),
    Builtin(&'static str, Vec<NodeId>),
    Call(Name, Vec<NodeId>),
    SteadyState(NodeId),
    Expectation(i32, NodeId),
    Opaque(ExprId),
}

/// Namespace identities use the constructed lag and the pin's namespace kind.
/// Written aliases and lag expressions remain in the expression arena.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct PathNamespace {
    kind: PathNamespaceKind,
    symbol: Name,
    lag: i32,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum PathNamespaceKind {
    Initval,
    SelfValue,
    Prev,
    LearntIn(PathLearningPeriod),
    Database(String),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum PathLearningPeriod {
    Integer(i32),
    Date(String),
}

#[derive(Clone, Debug)]
pub(crate) struct PathValueFacts {
    pub(crate) max_lag: i64,
    pub(crate) self_variables: Vec<(Name, i32)>,
}

struct PathValueReach {
    symbol_lags: Vec<(Name, i64)>,
    has_unlagged_leaf: bool,
    self_variables: Vec<(Name, i32)>,
}

pub(crate) struct ConstructorTree {
    nodes: Vec<Node>,
    values: Vec<Option<f64>>,
    path_value_reach: HashMap<NodeId, PathValueReach>,
    intern: HashMap<(DataTreeScope, Node), NodeId>,
    scopes: HashMap<DataTreeScope, (NodeId, NodeId, NodeId)>,
    scope: DataTreeScope,
    zero: NodeId,
    one: NodeId,
    minus_one: NodeId,
}

impl Default for ConstructorTree {
    fn default() -> Self {
        let mut tree = Self {
            nodes: Vec::new(),
            values: Vec::new(),
            path_value_reach: HashMap::new(),
            intern: HashMap::new(),
            scopes: HashMap::new(),
            scope: DataTreeScope::Dynamic,
            zero: 0,
            one: 0,
            minus_one: 0,
        };
        tree.select_scope(DataTreeScope::Dynamic);
        tree
    }
}

impl ConstructorTree {
    fn select_scope(&mut self, scope: DataTreeScope) {
        self.scope = scope;
        if let Some(&(zero, one, minus_one)) = self.scopes.get(&scope) {
            (self.zero, self.one, self.minus_one) = (zero, one, minus_one);
            return;
        }
        // Match initConstants order, including nodes used only for ordering.
        self.zero = self.intern(Node::Number("0".into()), Some(0.0));
        self.one = self.intern(Node::Number("1".into()), Some(1.0));
        self.intern(Node::Number("2".into()), Some(2.0));
        self.intern(Node::Number("3".into()), Some(3.0));
        self.minus_one = self.intern(Node::Neg(self.one), Some(-1.0));
        self.intern(Node::Number("NaN".into()), Some(f64::NAN));
        let infinity = self.intern(Node::Number("Inf".into()), Some(f64::INFINITY));
        self.intern(Node::Neg(infinity), Some(f64::NEG_INFINITY));
        self.intern(
            Node::Number("3.141592653589793".into()),
            Some(std::f64::consts::PI),
        );
        self.scopes
            .insert(scope, (self.zero, self.one, self.minus_one));
    }
    pub(crate) fn node(&self, id: NodeId) -> &Node {
        &self.nodes[id]
    }
    pub(crate) fn value(&self, id: NodeId) -> Option<f64> {
        self.values[id]
    }
    pub(crate) fn opaque(&mut self, id: ExprId) -> NodeId {
        self.intern(Node::Opaque(id), None)
    }

    /// Children are already complete. Each written allocation does constant
    /// work plus its argument list; neither caller rebuilds an existing tree.
    pub(crate) fn construct(
        &mut self,
        id: ExprId,
        kind: &ExprKind,
        children: &HashMap<ExprId, NodeId>,
        literal: Option<(&str, Option<f64>)>,
        context: ConstructorContext,
        name: impl Fn(Name) -> String,
    ) -> NodeId {
        self.select_scope(context.scope);
        match kind {
            ExprKind::Number => {
                let (text, value) = literal.expect("numeric constructor spelling");
                self.intern(Node::Number(text.into()), value)
            }
            ExprKind::Ident { name, timing, .. } => self.intern(
                Node::Ident(*name, *timing, context.scope),
                context.ident_value,
            ),
            ExprKind::PathNamespace { reference, lag } => {
                let lag = lag
                    .map(|id| self.match_integer(children[&id]))
                    .unwrap_or(Some(0));
                let Some(lag) = lag else {
                    return self.opaque(id);
                };
                let kind = match reference.namespace.as_deref() {
                    Some("init" | "initval") => PathNamespaceKind::Initval,
                    Some("self") => PathNamespaceKind::SelfValue,
                    Some("prev") => PathNamespaceKind::Prev,
                    Some("learnt_in") => {
                        let period = match reference.learnt_in.as_ref() {
                            Some(crate::model::PeriodPoint::Integer(period)) => {
                                PathLearningPeriod::Integer(*period)
                            }
                            Some(crate::model::PeriodPoint::Date(date)) => {
                                PathLearningPeriod::Date(path_learning_date(&date.constructor_text))
                            }
                            _ => return self.opaque(id),
                        };
                        PathNamespaceKind::LearntIn(period)
                    }
                    Some(database) => PathNamespaceKind::Database(database.into()),
                    None => return self.opaque(id),
                };
                self.intern(
                    Node::PathNamespace(PathNamespace {
                        kind,
                        symbol: reference.name,
                        lag,
                    }),
                    None,
                )
            }
            ExprKind::Unary { op, arg } => match op {
                UnOp::Pos => children[arg],
                UnOp::Neg => self.neg(children[arg]),
            },
            ExprKind::Binary { op, lhs, rhs } => self.binary(*op, children[lhs], children[rhs]),
            ExprKind::Call { callee, args } => {
                let args = args.iter().map(|id| children[id]).collect();
                if let Some(builtin) = builtin(&name(*callee)) {
                    self.builtin(builtin, args)
                } else {
                    self.intern(Node::Call(*callee, args), None)
                }
            }
            ExprKind::SteadyState { arg } => {
                let arg = children[arg];
                if let Some(value) = self.values[arg] {
                    self.constant(value)
                } else {
                    self.intern(Node::SteadyState(arg), None)
                }
            }
            ExprKind::Expectation { shift, arg } => {
                self.intern(Node::Expectation(*shift, children[arg]), None)
            }
            ExprKind::String | ExprKind::Error => self.opaque(id),
        }
    }

    /// A canonical zero denominator is refused before 0/x or x/x identities.
    /// Refusal facts have no source or diagnostic policy: Parse owns those.
    pub(crate) fn division_numerator(&self, id: NodeId) -> Option<NodeId> {
        self.pair(id, BinOp::Div)
            .and_then(|(lhs, rhs)| (rhs == self.zero).then_some(lhs))
    }

    /// NumConstNode uses stoi, which reads a decimal integer prefix. It thus
    /// accepts written `1.0` and `1e2` as 1. Unary minus must wrap a number.
    pub(crate) fn match_integer(&self, id: NodeId) -> Option<i32> {
        match &self.nodes[id] {
            Node::Number(text) => integer_prefix(text),
            Node::Neg(arg) => match &self.nodes[*arg] {
                Node::Number(text) => integer_prefix(text)?.checked_neg(),
                _ => None,
            },
            _ => None,
        }
    }

    /// Collect only surviving self leaves. Lag argument children have already
    /// been consumed by namespace construction and are not collection edges.
    /// Reach is memoized by constructed root; symbol roles are read afresh at
    /// each stanza callback, since change_type does not change node identity.
    pub(crate) fn path_value_facts(
        &mut self,
        root: NodeId,
        lagged_symbol: impl Fn(Name) -> bool,
    ) -> PathValueFacts {
        if !self.path_value_reach.contains_key(&root) {
            let reach = self.collect_path_value_reach(root);
            self.path_value_reach.insert(root, reach);
        }
        let reach = &self.path_value_reach[&root];
        let max_lag = reach
            .symbol_lags
            .iter()
            .map(|(symbol, lag)| if lagged_symbol(*symbol) { *lag } else { 0 })
            .chain(reach.has_unlagged_leaf.then_some(0))
            .max()
            .unwrap_or(0);
        PathValueFacts {
            max_lag,
            self_variables: reach.self_variables.clone(),
        }
    }

    fn collect_path_value_reach(&self, root: NodeId) -> PathValueReach {
        let mut seen = HashSet::new();
        let mut symbol_lags = HashMap::new();
        let mut has_unlagged_leaf = false;
        let mut self_variables = HashSet::new();
        let mut pending = vec![root];
        while let Some(id) = pending.pop() {
            if !seen.insert(id) {
                continue;
            }
            match &self.nodes[id] {
                Node::PathNamespace(reference) => {
                    let lag = -i64::from(reference.lag);
                    symbol_lags
                        .entry(reference.symbol)
                        .and_modify(|maximum: &mut i64| *maximum = (*maximum).max(lag))
                        .or_insert(lag);
                    if reference.kind == PathNamespaceKind::SelfValue {
                        self_variables.insert((reference.symbol, reference.lag));
                    }
                }
                Node::Neg(arg) | Node::SteadyState(arg) | Node::Expectation(_, arg) => {
                    pending.push(*arg);
                }
                Node::Binary(_, lhs, rhs) => pending.extend([*lhs, *rhs]),
                Node::Builtin(_, args) | Node::Call(_, args) => {
                    has_unlagged_leaf |= args.is_empty();
                    pending.extend(args);
                }
                _ => has_unlagged_leaf = true,
            }
        }
        PathValueReach {
            symbol_lags: symbol_lags.into_iter().collect(),
            has_unlagged_leaf,
            self_variables: self_variables.into_iter().collect(),
        }
    }

    /// The pin uses its JSON expression display in constructor refusals.
    /// Emit with an explicit stack, so a long written expression needs no
    /// recursive walk and no cached full string for every intermediate node.
    pub(crate) fn display<'a>(&self, id: NodeId, name: impl Fn(Name) -> &'a str) -> String {
        enum Part {
            Node(NodeId),
            Text(String),
        }
        let mut pending = vec![Part::Node(id)];
        let mut output = String::new();
        while let Some(part) = pending.pop() {
            let node = match part {
                Part::Text(text) => {
                    output.push_str(&text);
                    continue;
                }
                Part::Node(node) => node,
            };
            match &self.nodes[node] {
                Node::Number(text) => output.push_str(text),
                Node::Ident(symbol, timing, _) => {
                    output.push_str(name(*symbol));
                    if *timing != 0 {
                        output.push_str(&format!("({timing})"));
                    }
                }
                Node::PathNamespace(reference) => {
                    match &reference.kind {
                        PathNamespaceKind::Initval => output.push_str("initval"),
                        PathNamespaceKind::SelfValue => output.push_str("self"),
                        PathNamespaceKind::Prev => output.push_str("prev"),
                        PathNamespaceKind::LearntIn(period) => {
                            output.push_str("learnt_in(");
                            match period {
                                PathLearningPeriod::Integer(period) => {
                                    output.push_str(&period.to_string());
                                }
                                PathLearningPeriod::Date(date) => output.push_str(date),
                            }
                            output.push(')');
                        }
                        PathNamespaceKind::Database(database) => output.push_str(database),
                    }
                    output.push('.');
                    output.push_str(name(reference.symbol));
                    if matches!(
                        reference.kind,
                        PathNamespaceKind::SelfValue | PathNamespaceKind::Database(_)
                    ) {
                        output.push_str(&format!("({})", reference.lag));
                    }
                }
                Node::Neg(arg) => {
                    output.push_str("(-");
                    pending.push(Part::Text(")".into()));
                    pending.push(Part::Node(*arg));
                    if self.precedence(*arg) < 100 {
                        output.push('(');
                        pending.insert(pending.len() - 2, Part::Text(")".into()));
                    }
                }
                Node::Binary(op, lhs, rhs) => {
                    let prec = self.precedence(node);
                    let power = *op == BinOp::Pow as u8;
                    let left_parens = self.precedence(*lhs) < prec
                        || (power && self.pair(*lhs, BinOp::Pow).is_some());
                    let right_parens = self.precedence(*rhs) < prec
                        || (power && self.pair(*rhs, BinOp::Pow).is_some())
                        || (self.precedence(*rhs) == prec
                            && (*op == BinOp::Sub as u8 || *op == BinOp::Div as u8));
                    if right_parens {
                        pending.push(Part::Text(")".into()));
                    }
                    pending.push(Part::Node(*rhs));
                    if right_parens {
                        pending.push(Part::Text("(".into()));
                    }
                    pending.push(Part::Text(binary_text(*op).into()));
                    if left_parens {
                        pending.push(Part::Text(")".into()));
                    }
                    pending.push(Part::Node(*lhs));
                    if left_parens {
                        pending.push(Part::Text("(".into()));
                    }
                }
                Node::Builtin(callee, args) => {
                    output.push_str(callee);
                    output.push('(');
                    pending.push(Part::Text(")".into()));
                    for (index, arg) in args.iter().enumerate().rev() {
                        pending.push(Part::Node(*arg));
                        if index > 0 {
                            pending.push(Part::Text(",".into()));
                        }
                    }
                }
                Node::Call(callee, args) => {
                    output.push_str(name(*callee));
                    output.push('(');
                    pending.push(Part::Text(")".into()));
                    for (index, arg) in args.iter().enumerate().rev() {
                        pending.push(Part::Node(*arg));
                        if index > 0 {
                            pending.push(Part::Text(",".into()));
                        }
                    }
                }
                Node::SteadyState(arg) | Node::Expectation(_, arg) => {
                    match self.nodes[node] {
                        Node::Expectation(shift, _) => {
                            output.push_str(&format!("EXPECTATION({shift})("))
                        }
                        _ => output.push_str("STEADY_STATE("),
                    }
                    pending.push(Part::Text(")".into()));
                    pending.push(Part::Node(*arg));
                }
                Node::Opaque(_) => output.push('?'),
            }
        }
        output
    }

    fn precedence(&self, node: NodeId) -> u8 {
        match self.nodes[node] {
            Node::Binary(op, _, _) => match op {
                op if op == BinOp::EqEq as u8 || op == BinOp::Ne as u8 => 1,
                op if op == BinOp::Lt as u8
                    || op == BinOp::Gt as u8
                    || op == BinOp::Le as u8
                    || op == BinOp::Ge as u8 =>
                {
                    2
                }
                op if op == BinOp::Add as u8 || op == BinOp::Sub as u8 => 3,
                op if op == BinOp::Mul as u8 || op == BinOp::Div as u8 => 4,
                _ => 5,
            },
            _ => 100,
        }
    }

    fn intern(&mut self, node: Node, value: Option<f64>) -> NodeId {
        if let Some(&id) = self.intern.get(&(self.scope, node.clone())) {
            if self.values[id].is_none() {
                self.values[id] = value;
            }
            return id;
        }
        let id = self.nodes.len();
        self.nodes.push(node.clone());
        self.values.push(value);
        self.intern.insert((self.scope, node), id);
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
}

fn integer_prefix(text: &str) -> Option<i32> {
    let bytes = text.as_bytes();
    let start = usize::from(matches!(bytes.first(), Some(b'+' | b'-')));
    let end = start
        + bytes[start..]
            .iter()
            .take_while(|byte| byte.is_ascii_digit())
            .count();
    (end > start).then(|| text[..end].parse().ok()).flatten()
}

/// DynareBison's date_expr action wraps the written DATE and appends each
/// offset token. Spaces and comments do not enter that generated identity.
pub(crate) fn path_learning_date(text: &str) -> String {
    let compact: String = crate::lexer::tokenize(text)
        .iter()
        .filter(|token| token.kind != crate::lexer::TokenKind::Eof)
        .map(|token| token.text(text))
        .collect();
    match compact.split_once('+') {
        Some((date, offsets)) => format!("dates('{date}')+{offsets}"),
        None => format!("dates('{compact}')"),
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

fn binary_text(op: u8) -> &'static str {
    const TEXT: &[&str] = &["+", "-", "*", "/", "^", "<", ">", "<=", ">=", "==", "!="];
    TEXT[op as usize]
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
