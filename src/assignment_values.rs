//! Proven arithmetic at an assignment's execution position. No MATLAB execution.

use std::collections::HashMap;

use crate::expr::{ExprId, ExprKind};
use crate::intern::Name;
use crate::lexer::{Token, TokenKind};
use crate::model::{AssignmentIndex, ExecutionStep, Model, StatementKind};
use crate::model_info::arithmetic_number;

#[derive(Clone, Debug, PartialEq)]
pub struct AssignmentValue {
    pub statement_id: usize,
    pub value: Option<f64>,
    pub written_plain_number: bool,
}

fn calls_may_mutate(
    model: &Model,
    expr: ExprId,
    shadows: &std::collections::HashSet<Name>,
) -> bool {
    match &model.exprs.get(expr).kind {
        ExprKind::Call { callee, args } => {
            shadows.contains(callee)
                || !["exp", "log", "ln", "sqrt", "abs", "sin", "cos"].contains(&model.name(*callee))
                || args
                    .iter()
                    .any(|arg| calls_may_mutate(model, *arg, shadows))
        }
        ExprKind::Binary { lhs, rhs, .. } => {
            calls_may_mutate(model, *lhs, shadows) || calls_may_mutate(model, *rhs, shadows)
        }
        ExprKind::Unary { arg, .. }
        | ExprKind::SteadyState { arg }
        | ExprKind::Expectation { arg, .. } => calls_may_mutate(model, *arg, shadows),
        _ => false,
    }
}

#[derive(Default)]
struct NativeFlow {
    scopes: Vec<String>,
    uncertain: bool,
}

impl NativeFlow {
    fn active(&self) -> bool {
        self.uncertain || !self.scopes.is_empty()
    }

    fn observe(&mut self, model: &Model, tokens: &[Token]) {
        for (index, token) in tokens.iter().enumerate() {
            if token.kind != TokenKind::Ident {
                continue;
            }
            let boundary = index == 0
                || tokens[index - 1].kind == TokenKind::Semi
                || model
                    .source
                    .get(tokens[index - 1].span.end as usize..token.span.start as usize)
                    .is_some_and(|gap| gap.contains('\n'));
            if !boundary {
                continue;
            }
            let word = token.text(&model.source).to_ascii_lowercase();
            if [
                "if", "for", "while", "switch", "try", "parfor", "spmd", "function", "classdef",
                "do",
            ]
            .contains(&word.as_str())
                && tokens
                    .get(index + 1)
                    .is_none_or(|next| next.kind != TokenKind::Eq)
            {
                self.scopes.push(word);
            } else if [
                "end",
                "endif",
                "endfor",
                "endwhile",
                "endswitch",
                "end_try_catch",
                "endfunction",
                "endclassdef",
                "until",
            ]
            .contains(&word.as_str())
            {
                let bare = tokens.get(index + 1).is_none_or(|next| {
                    next.kind == TokenKind::Semi
                        || model
                            .source
                            .get(token.span.end as usize..next.span.start as usize)
                            .is_some_and(|gap| gap.contains('\n'))
                }) || word == "until";
                if !bare {
                    continue;
                }
                if let Some(open) = self.scopes.pop() {
                    if word != "end"
                        && !matches!(
                            (open.as_str(), word.as_str()),
                            ("if", "endif")
                                | ("for", "endfor")
                                | ("while", "endwhile")
                                | ("switch", "endswitch")
                                | ("try", "end_try_catch")
                                | ("function", "endfunction")
                                | ("classdef", "endclassdef")
                                | ("do", "until")
                        )
                    {
                        self.uncertain = true;
                    }
                } else {
                    self.uncertain = true;
                }
            } else if ["else", "elseif", "case", "otherwise", "catch"].contains(&word.as_str())
                && self.scopes.is_empty()
            {
                self.uncertain = true;
            }
        }
    }
}

/// Includes unknown occurrences, so transport grouping cannot accidentally hide
/// a conflicting macro copy before deciding whether a written hint is safe.
pub fn assignment_values(model: &Model) -> Vec<AssignmentValue> {
    let mut known: HashMap<Name, f64> = HashMap::new();
    let mut native_values = std::collections::HashSet::new();
    let mut native_shadows = std::collections::HashSet::new();
    let mut flow = NativeFlow::default();
    let mut output = Vec::new();
    for (step_index, step) in model.execution_steps.iter().enumerate() {
        match step {
            ExecutionStep::Opaque(_) => {
                known.clear();
                native_values.clear();
                if let Some(tokens) = model.opaque_tokens.get(&step_index) {
                    flow.observe(model, tokens);
                } else {
                    flow.uncertain = true;
                }
            }
            ExecutionStep::Statement(id) => {
                let statement = &model.statements[*id];
                if let Some(index) = statement.assignment {
                    let assignment = match index {
                        AssignmentIndex::Parameter(index) => &model.param_assignments[index],
                        AssignmentIndex::Helper(index) => &model.helper_assignments[index],
                    };
                    let syntax = assignment
                        .expr
                        .and_then(|expr| model.assignment_syntax.get(&expr));
                    if !syntax.is_some_and(|syntax| syntax.full_rhs) {
                        known.clear();
                        native_values.clear();
                    }
                    let value = if !flow.active() && syntax.is_some_and(|syntax| syntax.full_rhs) {
                        if assignment
                            .expr
                            .is_some_and(|expr| calls_may_mutate(model, expr, &native_shadows))
                        {
                            known.clear();
                            native_values.clear();
                        }
                        assignment
                            .expr
                            .and_then(|expr| arithmetic_number(model, expr, &known, true))
                    } else {
                        None
                    };
                    // The RHS observes the old environment; this target only
                    // shadows a callee after its assignment has been processed.
                    if assignment.native {
                        native_shadows.insert(assignment.name);
                    }
                    if let Some(value) = value {
                        known.insert(assignment.name, value);
                        if assignment.native {
                            native_values.insert(assignment.name);
                        } else {
                            native_values.remove(&assignment.name);
                        }
                    } else {
                        known.remove(&assignment.name);
                        native_values.remove(&assignment.name);
                    }
                    output.push(AssignmentValue {
                        statement_id: *id,
                        value,
                        written_plain_number: syntax
                            .is_some_and(|syntax| syntax.written_plain_number),
                    });
                } else if statement.kind == StatementKind::Declaration {
                    for declaration in model
                        .written_declarations
                        .iter()
                        .filter(|declaration| declaration.statement_id == *id)
                    {
                        if native_values.remove(&declaration.declaration.name) {
                            known.remove(&declaration.declaration.name);
                        }
                    }
                } else if statement.kind == StatementKind::Dimension
                    || (statement.kind == StatementKind::Block
                        && ["model", "model_replace", "steady_state_model"]
                            .contains(&statement.name.as_str()))
                {
                    // Definitions and equation trees do not execute calibration.
                } else {
                    known.clear();
                    native_values.clear();
                }
            }
        }
    }
    output
}
