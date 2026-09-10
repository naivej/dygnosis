//! Index-based expression arena. Identifier refs come from walking this tree.

use crate::intern::Name;
use crate::span::Span;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ExprId(u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnOp {
    Pos,
    Neg,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Pow,
    Lt,
    Gt,
    Le,
    Ge,
    EqEq,
    Ne,
}

#[derive(Clone, Debug)]
pub enum ExprKind {
    Ident {
        name: Name,
        timing: i32,
        /// Identifier lexeme only (not whitespace or lead/lag).
        ident_span: Span,
        /// `(+1)` including parens, when a lead/lag was parsed.
        timing_span: Option<Span>,
    },
    Number,
    String,
    Unary {
        op: UnOp,
        arg: ExprId,
    },
    Binary {
        op: BinOp,
        lhs: ExprId,
        rhs: ExprId,
    },
    Call {
        callee: Name,
        args: Vec<ExprId>,
    },
    SteadyState {
        arg: ExprId,
    },
    Expectation {
        shift: i32,
        arg: ExprId,
    },
    Error,
}

#[derive(Clone, Debug)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
}

#[derive(Clone, Debug, Default)]
pub struct ExprArena {
    nodes: Vec<Expr>,
}

impl ExprArena {
    pub fn alloc(&mut self, kind: ExprKind, span: Span) -> ExprId {
        let id = ExprId(self.nodes.len() as u32);
        self.nodes.push(Expr { kind, span });
        id
    }

    pub fn get(&self, id: ExprId) -> &Expr {
        &self.nodes[id.0 as usize]
    }

    /// Visits `Ident` nodes only (not Call callees, not String/Number contents).
    pub fn walk_idents(&self, root: ExprId) -> impl Iterator<Item = IdentRef> {
        let mut out = Vec::new();
        self.collect_idents(root, &mut out);
        out.into_iter()
    }

    fn collect_idents(&self, id: ExprId, out: &mut Vec<IdentRef>) {
        let expr = self.get(id);
        match &expr.kind {
            ExprKind::Ident {
                name,
                timing,
                ident_span,
                timing_span,
            } => {
                out.push(IdentRef {
                    name: *name,
                    span: *ident_span,
                    timing: *timing,
                    timing_span: *timing_span,
                });
            }
            ExprKind::Number | ExprKind::String | ExprKind::Error => {}
            ExprKind::Unary { arg, .. } => self.collect_idents(*arg, out),
            ExprKind::Binary { lhs, rhs, .. } => {
                self.collect_idents(*lhs, out);
                self.collect_idents(*rhs, out);
            }
            ExprKind::Call { args, .. } => {
                for arg in args {
                    self.collect_idents(*arg, out);
                }
            }
            ExprKind::SteadyState { arg } | ExprKind::Expectation { arg, .. } => {
                self.collect_idents(*arg, out);
            }
        }
    }
}

/// Identifier reference from an expression tree walk.
///
/// `span` is the identifier lexeme only; `timing_span` is `(+1)` including
/// parens when a lead/lag was present.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IdentRef {
    pub name: Name,
    pub span: Span,
    pub timing: i32,
    pub timing_span: Option<Span>,
}
