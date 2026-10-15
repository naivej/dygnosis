//! OccBin / complementarity Errors `E170`–`E177`, `E180`–`E185` and obsolete-`mcp` Warning `W170`.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use crate::engine::diagnostics::{Diagnostic, RelatedDiagnostic, Severity};
use crate::engine::language::expr::{BinOp, ExprId, ExprKind};
use crate::engine::language::intern::Name;
use crate::engine::language::lexer::{tokenize, Token, TokenKind};
use crate::engine::language::model::{
    Equation, Model, OccbinConstraint, OccbinExpr, SymbolContext,
};
use crate::engine::language::span::Span;

pub fn check_occbin(model: &Model) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    check_blocks(model, &mut out);
    let illegal_block = out.iter().any(|d| d.code == "E170" || d.code == "E171");
    check_constraint_rows(model, &mut out);
    check_equation_tags(model, illegal_block, &mut out);
    out
}

/// Constructor, regime, equation and end-block actions are separate Parse
/// callbacks. The maximum-block and missing-regime tests remain later Check.
pub(crate) fn parse_refusals(model: &Model) -> Vec<(Diagnostic, usize)> {
    let locals = model_locals(model);
    let mut rows = Vec::new();
    for constraint in &model.occbin_constraints {
        let mut seen = HashSet::new();
        for (clause, piece) in &constraint.clauses {
            if crate::engine::control::cancelled() {
                return Vec::new();
            }
            let Some(id) = piece.expr else {
                continue;
            };
            let mut diagnostics = Vec::new();
            walk_occbin_expr_inner(
                model,
                id,
                &locals,
                Some(&piece.active_tokens),
                &mut diagnostics,
            );
            for diagnostic in diagnostics {
                let order = crate::engine::diagnostics::parse_preflight::within(
                    model,
                    piece.active_tokens.clone(),
                    diagnostic.span,
                    true,
                )
                .unwrap_or(piece.active_tokens.start);
                rows.push((diagnostic, order));
            }
            if !seen.insert(*clause) {
                let span = model.expanded_tokens[piece.active_tokens.start].span;
                rows.push((
                    error(
                        span,
                        "E184",
                        format!("The '{clause}' clause is declared multiple times"),
                    ),
                    piece.active_tokens.end,
                ));
            }
        }
        for (clause, piece) in [
            ("bind", constraint.bind.as_ref()),
            ("relax", constraint.relax.as_ref()),
        ] {
            if let Some(piece) = piece
                && piece.constructed_binary == Some(false)
            {
                rows.push((
                    error(
                        piece.span,
                        "E181",
                        format!("The '{clause}' expression must be an inequality constraint."),
                    ),
                    constraint.callback_order,
                ));
            }
        }
    }
    rows.extend(equation_parse_refusals(model));
    for constraint in &model.occbin_constraints {
        let Some(statement) = model.statements.iter().find(|statement| {
            statement.name == "occbin_constraints"
                && statement.complete
                && statement.token_range.contains(&constraint.parse_order)
        }) else {
            continue;
        };
        let parameter = format!("occbin_{}_bind", constraint.name);
        let known = model.intern.lookup(&parameter).is_some_and(|name| {
            model
                .symbol_kind_in_context(name, constraint.symbol_type_context)
                .is_some()
        }) || model.written_equations.iter().any(|row| {
            row.dimension.is_none()
                && !row.equation.static_tag
                && row.equation.active_tokens.end < statement.token_range.end
                && ["bind", "relax"].iter().any(|key| {
                    row.equation
                        .tag_map
                        .get(*key)
                        .is_some_and(|value| split_names(value).contains(&constraint.name.as_str()))
                })
        });
        if !known {
            rows.push((
                error(
                    constraint.name_span,
                    "E175",
                    format!(
                        "No equation has been declared for constraint '{}'",
                        constraint.name
                    ),
                ),
                statement.token_range.end,
            ));
        } else if constraint
            .bind
            .as_ref()
            .is_none_or(|bind| bind.constructed_binary.is_none())
        {
            let span = constraint
                .bind
                .as_ref()
                .map_or(constraint.span, |bind| bind.span);
            rows.push((
                error(
                    span,
                    "E174",
                    format!(
                        "The 'bind' expression is missing in constraint '{}'",
                        constraint.name
                    ),
                ),
                statement.token_range.end,
            ));
        }
    }
    rows
}

/// The driver creates OccBin parameters after complementarity validation and
/// before it rejects duplicate regimes. The parser shares these predicates.
fn equation_parse_refusals(model: &Model) -> Vec<(Diagnostic, usize)> {
    let mut rows: Vec<_> = model
        .equation_tag_duplicates
        .iter()
        .map(|(key, span, order)| {
            (
                error(
                    *span,
                    "E256",
                    format!("Tag '{key}' cannot be used twice for the same equation"),
                ),
                *order,
            )
        })
        .collect();
    let mut trackers = BTreeMap::<String, OccbinRegimeTracker>::new();
    for written in &model.written_equations {
        if crate::engine::control::cancelled() {
            return Vec::new();
        }
        let eq = &written.equation;
        if eq.is_local {
            continue;
        }
        let context = eq
            .rhs_expr
            .or(eq.lhs_expr)
            .and_then(|id| model.constructor_contexts.get(&id))
            .copied()
            .unwrap_or_default();
        let order = eq.active_tokens.end
            + usize::from(
                model
                    .expanded_tokens
                    .get(eq.active_tokens.end)
                    .is_some_and(|token| token.kind == TokenKind::Semi),
            );
        let mut diagnostics = Vec::new();
        if eq.tag_map.contains_key("mcp") {
            if eq.complementarity.is_some() {
                diagnostics.push(error(eq.span, "E180", "Can't have both an 'mcp' tag and a complementarity condition after the perpendicular symbol"));
            } else {
                check_mcp_form_at(
                    model,
                    eq,
                    Some(context),
                    written.dimension.is_some(),
                    &mut diagnostics,
                );
            }
        }
        if let Some(comp) = &eq.complementarity
            && comp.matched.is_none()
        {
            diagnostics.push(error(
                comp.span,
                "E183",
                "Complementarity condition has an incorrect form",
            ));
        }
        let has_bind = eq.tag_map.contains_key("bind");
        let has_relax = eq.tag_map.contains_key("relax");
        if written.dimension.is_none() && !eq.static_tag && (has_bind || has_relax) {
            if !eq.tag_map.contains_key("name") {
                diagnostics.push(error(
                    eq.span,
                    "E173",
                    "An equation with a 'bind' or 'relax' tag must have a 'name' tag.",
                ));
            }
            check_tag_pieces_at(model, eq, "bind", Some(context), &mut diagnostics);
            check_tag_pieces_at(model, eq, "relax", Some(context), &mut diagnostics);
            let bind = split_names(eq.tag_map.get("bind").map_or("", String::as_str));
            let relax = split_names(eq.tag_map.get("relax").map_or("", String::as_str));
            if let Some(name) = bind.iter().find(|name| relax.contains(name)) {
                diagnostics.push(error(
                    eq.span,
                    "E176",
                    format!("The constraint '{name}' is both in the 'bind' and 'relax' tags"),
                ));
            } else if bind.iter().chain(&relax).all(|name| is_occbin_ident(name))
                && let Some(name) = eq.tag_map.get("name")
            {
                let tracker = trackers.entry(name.clone()).or_default();
                if let Err(RegimeError::AlreadyPresent {
                    bind,
                    relax,
                    earlier,
                }) = tracker.add_regime(
                    &bind,
                    &relax,
                    RelatedDiagnostic::equation(model, eq, eq.span),
                ) {
                    diagnostics.push(error(eq.span, "E177", format!("The regime corresponding to {} has already been declared for this equation", render_bind_relax(&bind, &relax))).with_related(*earlier));
                }
            }
        }
        rows.extend(
            diagnostics
                .into_iter()
                .map(|diagnostic| (diagnostic, order)),
        );
    }
    rows
}

#[cfg(test)]
mod parse_receipt_tests {
    use super::*;
    use crate::engine::language::parser::parse;

    // Constructor errors stop inside their retained clause execution. A later
    // OccBin structural Check error must not mark that Parse prefix refused.
    #[test]
    fn parse_receipts_stop_at_the_constructor_not_check_structure() {
        let model = parse("var y;varexo e;occbin_constraints;name 'c';bind e>0;end;");
        let rows = parse_refusals(&model);
        let (diagnostic, order) = rows.first().unwrap();
        assert_eq!(diagnostic.code, "E182");
        assert_eq!(model.expanded_tokens[order - 1].text(&model.source), "e");
        let model = parse("var y;model;[name='c',bind='c']y=0;[name='d',bind='d']y=0;end;occbin_constraints;name 'c';bind y>0;end;occbin_constraints;name 'd';bind y>0;end;");
        assert!(check_occbin(&model).iter().any(|row| row.code == "E170"));
        assert!(parse_refusals(&model).is_empty());
    }

    #[test]
    fn equation_and_end_block_callbacks_keep_original_receipts() {
        // Guards equation Parse errors and end-block refusals from becoming
        // complete named prefixes. A duplicate tag refuses before RHS calls.
        let model = parse("var y;model;[bind='c']y=0;end;database later;");
        assert!(parse_refusals(&model)
            .iter()
            .any(|(diagnostic, _)| diagnostic.code == "E173"));
        let model = parse("var y;model;[name='a',name='b']y=pac_expectation(m);end;");
        let (_, order) = parse_refusals(&model)
            .into_iter()
            .find(|(diagnostic, _)| diagnostic.code == "E256")
            .unwrap();
        assert!(order < model.named_model_operators[0].keyword_order);
        let model = parse("var y;model;[name='a',bind='c']y=0;end;occbin_constraints;name 'c';relax y>0;end;database later;");
        let (diagnostic, order) = parse_refusals(&model)
            .into_iter()
            .find(|(diagnostic, _)| diagnostic.code == "E174")
            .unwrap();
        assert_eq!(model.expanded_tokens[order].text(&model.source), "database");
        assert_eq!(model.expanded_tokens[order - 2].text(&model.source), "end");
        assert!(diagnostic.span.start < model.expanded_tokens[order - 2].span.start);
        let model = parse("var y;occbin_constraints;name 'c';bind y>0;end;database later;model;[name='a',bind='c']y=0;end;");
        assert!(parse_refusals(&model)
            .iter()
            .any(|(diagnostic, _)| diagnostic.code == "E175"));
    }

    #[test]
    fn every_clause_and_legacy_mcp_literal_use_their_actual_parse_rules() {
        // Guards an overwritten clause from hiding its earlier constructor
        // failure, and raw token heuristics from rejecting accepted MCP text.
        let model = parse("var y;varexo e;occbin_constraints;name 'c';bind e>0;bind y>0;end;");
        let rows = parse_refusals(&model);
        let first = rows.iter().min_by_key(|(_, order)| *order).unwrap();
        assert_eq!(first.0.code, "E182");
        assert_eq!(model.expanded_tokens[first.1 - 1].text(&model.source), "e");
        assert!(rows.iter().any(|(diagnostic, _)| diagnostic.code == "E184"));
        let model = parse("var y;model;[mcp='y>1+p']y=0;end;");
        assert!(parse_refusals(&model).is_empty());
        let model = parse("var y;model;[mcp='y(-1)>0']y=0;end;");
        assert!(parse_refusals(&model)
            .iter()
            .any(|(diagnostic, _)| diagnostic.code == "E262"));
    }
}

fn error(span: Span, code: &str, message: impl Into<String>) -> Diagnostic {
    Diagnostic::new(span, Severity::Error, code, message)
}

fn check_mcp_form(model: &Model, eq: &Equation, out: &mut Vec<Diagnostic>) {
    let context = eq
        .rhs_expr
        .or(eq.lhs_expr)
        .and_then(|id| model.constructor_contexts.get(&id))
        .copied();
    check_mcp_form_at(model, eq, context, false, out);
}

fn check_mcp_form_at(
    model: &Model,
    eq: &Equation,
    context: Option<SymbolContext>,
    heterogeneous: bool,
    out: &mut Vec<Diagnostic>,
) {
    check_mcp_form_using(model, eq, context, heterogeneous, &model.intern, out);
}

pub(crate) fn before_occbin_parameters(
    model: &Model,
    eq: &Equation,
    context: SymbolContext,
    heterogeneous: bool,
    intern: &crate::engine::language::intern::Interner,
) -> bool {
    if eq
        .complementarity
        .as_ref()
        .is_some_and(|comp| comp.matched.is_none())
        || eq.complementarity.is_some() && eq.tag_map.contains_key("mcp")
    {
        return false;
    }
    let mut diagnostics = Vec::new();
    check_mcp_form_using(
        model,
        eq,
        Some(context),
        heterogeneous,
        intern,
        &mut diagnostics,
    );
    diagnostics.is_empty()
}

fn check_mcp_form_using(
    model: &Model,
    eq: &Equation,
    context: Option<SymbolContext>,
    heterogeneous: bool,
    intern: &crate::engine::language::intern::Interner,
    out: &mut Vec<Diagnostic>,
) {
    let Some(value) = eq.tag_map.get("mcp") else {
        return;
    };
    let ineq = value.find('<').or_else(|| value.find('>'));
    let Some(i) = ineq else {
        out.push(error(
            eq.span,
            "E265",
            "'mcp' tag does not contain an inequality",
        ));
        return;
    };
    let name = value[..i].trim();
    let rhs = value[i + 1..].trim();
    let kind = intern.lookup(name).and_then(|name| {
        context.map_or_else(
            || model.final_symbol_kind(name),
            |context| model.symbol_kind_in_context(name, context),
        )
    });
    if kind.is_none() {
        out.push(error(
            eq.span,
            "E262",
            "Left-hand side of expression in 'mcp' tag is not a variable",
        ));
        return;
    }
    if heterogeneous {
        out.push(error(
            eq.span,
            "E479",
            "'mcp' tags are not allowed in heterogeneous model blocks",
        ));
        return;
    }
    let ordinary = intern.lookup(name).is_some_and(|name| {
        context.is_none_or(|context| !model.heterogeneous_in_context(name, context))
    });
    if kind != Some("var") || !ordinary {
        out.push(error(
            eq.span,
            "E263",
            "Left-hand side of expression in 'mcp' tag is not an endogenous variable",
        ));
        return;
    }
    if !mcp_constant_prefix(rhs) {
        out.push(error(
            eq.span,
            "E264",
            "Right-hand side of expression in 'mcp' tag should be a constant",
        ));
    }
}

/// The legacy tag writer uses strtod and does not require its end pointer to
/// consume the rest. This is a numeric prefix, not a Dynare expression.
fn mcp_constant_prefix(text: &str) -> bool {
    let text = text.strip_prefix(['+', '-']).unwrap_or(text);
    let lower = text.to_ascii_lowercase();
    lower.starts_with("inf")
        || lower.starts_with("nan")
        || text.as_bytes().first().is_some_and(u8::is_ascii_digit)
        || text
            .strip_prefix('.')
            .and_then(|text| text.as_bytes().first())
            .is_some_and(u8::is_ascii_digit)
}

/// Heterogeneous `[mcp]` stops at the variable lookup. A declared name is
/// **E479** in `check_d_hank`; the endogenous and constant checks never run.
fn check_het_mcp_lhs(model: &Model, eq: &Equation, out: &mut Vec<Diagnostic>) {
    let Some(value) = eq.tag_map.get("mcp") else {
        return;
    };
    let tokens: Vec<Token> = tokenize(value)
        .into_iter()
        .filter(|t| t.kind != TokenKind::Eof)
        .collect();
    let Some(i) = tokens.iter().position(|t| {
        matches!(
            t.kind,
            TokenKind::Lt | TokenKind::Gt | TokenKind::Le | TokenKind::Ge
        )
    }) else {
        out.push(error(
            eq.span,
            "E265",
            "'mcp' tag does not contain an inequality",
        ));
        return;
    };
    let lhs = &tokens[..i];
    if !mcp_lhs_is_var(lhs) {
        out.push(error(
            eq.span,
            "E262",
            "Left-hand side of expression in 'mcp' tag is not a variable",
        ));
        return;
    }
    let name = lhs[0].text(value);
    let declared = model
        .endogenous
        .iter()
        .chain(&model.exogenous)
        .chain(&model.deterministic_exogenous)
        .chain(&model.parameters)
        .chain(&model.model_local_variables)
        .any(|decl| model.name(decl.name) == name);
    if !declared {
        out.push(error(
            eq.span,
            "E262",
            "Left-hand side of expression in 'mcp' tag is not a variable",
        ));
    }
}

fn mcp_lhs_is_var(tokens: &[Token]) -> bool {
    if tokens.is_empty() || tokens[0].kind != TokenKind::Ident {
        return false;
    }
    if tokens.len() == 1 {
        return true;
    }
    // ident ( signed_integer )
    if tokens.len() >= 4
        && tokens[1].kind == TokenKind::LParen
        && (tokens[2].kind == TokenKind::Number
            || tokens[2].kind == TokenKind::Plus
            || tokens[2].kind == TokenKind::Minus)
        && tokens.last().is_some_and(|t| t.kind == TokenKind::RParen)
    {
        return true;
    }
    false
}

fn split_names(s: &str) -> Vec<&str> {
    s.split(',').filter(|p| !p.is_empty()).collect()
}

pub(crate) fn is_occbin_ident(s: &str) -> bool {
    let mut chars = s.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first.is_ascii_alphabetic() || first == '_') {
        return false;
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn is_inequality(kind: &ExprKind) -> bool {
    matches!(
        kind,
        ExprKind::Binary {
            op: BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge,
            ..
        }
    )
}

fn is_binary(kind: &ExprKind) -> bool {
    matches!(kind, ExprKind::Binary { .. })
}

fn token_text<'a>(model: &'a Model, tok: &'a Token) -> &'a str {
    tok.text(&model.source)
}

fn check_blocks(model: &Model, out: &mut Vec<Diagnostic>) {
    let blocks = &model.occbin_constraints_blocks;
    if blocks.len() > 1 {
        out.push(error(
            blocks[1],
            "E170",
            "Multiple 'occbin_constraints' blocks are not allowed",
        ));
    } else if blocks.len() == 1 && model.occbin_constraints.len() > 2 {
        out.push(error(
            model.occbin_constraints[2].span,
            "E171",
            "only up to two constraints are supported in 'occbin_constraints' block",
        ));
    }
}

fn check_constraint_rows(model: &Model, out: &mut Vec<Diagnostic>) {
    let mentioned = mentioned_constraints(model);
    for c in &model.occbin_constraints {
        if crate::engine::control::cancelled() {
            return Default::default();
        }
        if !is_occbin_ident(&c.name) {
            out.push(error(
                c.name_span,
                "E185",
                format!(
                    "The string '{}' is not a valid Occbin constraint name (contains unauthorized characters)",
                    c.name
                ),
            ));
        }
        let param = format!("occbin_{}_bind", c.name);
        if name_is_var_like(model, &param) {
            out.push(error(
                c.name_span,
                "E185",
                format!(
                    "The name '{param}' is already used. Please use another name for Occbin constraint '{}'",
                    c.name
                ),
            ));
        }
        if !mentioned.contains(c.name.as_str()) {
            out.push(error(
                c.name_span,
                "E175",
                format!("No equation has been declared for constraint '{}'", c.name),
            ));
        }
        match bind_root(model, c) {
            BindRoot::Missing { span } => {
                out.push(error(
                    span,
                    "E174",
                    format!(
                        "The 'bind' expression is missing in constraint '{}'",
                        c.name
                    ),
                ));
            }
            BindRoot::NotInequality { span, which } => {
                out.push(error(
                    span,
                    "E181",
                    format!("The '{which}' expression must be an inequality constraint."),
                ));
            }
            BindRoot::Ok => {}
        }
        if let Some(relax) = c.relax.as_ref()
            && let Some(id) = relax.expr
        {
            let kind = &model.exprs.get(id).kind;
            if is_binary(kind) && !is_inequality(kind) {
                out.push(error(
                    relax.span,
                    "E181",
                    "The 'relax' expression must be an inequality constraint.",
                ));
            }
        }
        let locals = model_locals(model);
        for expr in occbin_exprs(c) {
            if crate::engine::control::cancelled() {
                return Default::default();
            }
            if let Some(id) = expr.expr {
                walk_occbin_expr_inner(model, id, &locals, None, out);
            }
        }
    }
    if !model.occbin_constraints_blocks.is_empty() {
        let tokens = tokenize(&model.source);
        for &block in &model.occbin_constraints_blocks {
            if crate::engine::control::cancelled() {
                return Default::default();
            }
            check_duplicate_clauses(model, &tokens, block, out);
        }
    }
}

enum BindRoot {
    Ok,
    Missing { span: Span },
    NotInequality { span: Span, which: &'static str },
}

fn bind_root(model: &Model, c: &OccbinConstraint) -> BindRoot {
    let Some(bind) = c.bind.as_ref() else {
        return BindRoot::Missing { span: c.span };
    };
    let Some(id) = bind.expr else {
        return BindRoot::Missing { span: bind.span };
    };
    let kind = &model.exprs.get(id).kind;
    if is_inequality(kind) {
        BindRoot::Ok
    } else if is_binary(kind) {
        BindRoot::NotInequality {
            span: bind.span,
            which: "bind",
        }
    } else {
        BindRoot::Missing { span: bind.span }
    }
}

fn occbin_exprs(c: &OccbinConstraint) -> impl Iterator<Item = &OccbinExpr> {
    [
        c.bind.as_ref(),
        c.relax.as_ref(),
        c.error_bind.as_ref(),
        c.error_relax.as_ref(),
    ]
    .into_iter()
    .flatten()
}

fn mentioned_constraints(model: &Model) -> HashSet<String> {
    let mut out = HashSet::new();
    for eq in &model.equations {
        if crate::engine::control::cancelled() {
            return Default::default();
        }
        if eq.is_local {
            continue;
        }
        if let Some(bind) = eq.tag_map.get("bind") {
            out.extend(split_names(bind).into_iter().map(str::to_string));
        }
        if let Some(relax) = eq.tag_map.get("relax") {
            out.extend(split_names(relax).into_iter().map(str::to_string));
        }
    }
    out
}

fn name_is_var_like(model: &Model, name: &str) -> bool {
    model
        .endogenous
        .iter()
        .chain(model.exogenous.iter())
        .chain(model.deterministic_exogenous.iter())
        .any(|d| model.name(d.name) == name)
}

fn check_duplicate_clauses(
    model: &Model,
    tokens: &[Token],
    block: Span,
    out: &mut Vec<Diagnostic>,
) {
    let in_block: Vec<&Token> = tokens
        .iter()
        .filter(|t| t.span.start >= block.start && t.span.end <= block.end)
        .collect();
    let mut i = 0;
    while i < in_block.len() && in_block[i].kind != TokenKind::Semi {
        if crate::engine::control::cancelled() {
            return Default::default();
        }
        i += 1;
    }
    if i < in_block.len() {
        i += 1;
    }
    let mut seen: HashSet<String> = HashSet::new();
    while i < in_block.len() {
        if crate::engine::control::cancelled() {
            return Default::default();
        }
        let tok = in_block[i];
        if tok.kind == TokenKind::Eof {
            break;
        }
        if tok.kind != TokenKind::Ident {
            i += 1;
            continue;
        }
        let word = token_text(model, tok);
        if word.eq_ignore_ascii_case("end") {
            break;
        }
        let clause = clause_keyword(word);
        if let Some(clause) = clause {
            if clause == "name" {
                seen.clear();
            } else if !seen.insert(clause.to_string()) {
                out.push(error(
                    tok.span,
                    "E184",
                    format!("The '{clause}' clause is declared multiple times"),
                ));
            }
            while i < in_block.len() && in_block[i].kind != TokenKind::Semi {
                if crate::engine::control::cancelled() {
                    return Default::default();
                }
                i += 1;
            }
            if i < in_block.len() {
                i += 1;
            }
            continue;
        }
        i += 1;
    }
}

fn clause_keyword(word: &str) -> Option<&'static str> {
    const CLAUSES: &[&str] = &["name", "bind", "relax", "error_bind", "error_relax"];
    CLAUSES
        .iter()
        .copied()
        .find(|c| word.eq_ignore_ascii_case(c))
}

fn model_locals(model: &Model) -> HashSet<Name> {
    let mut out = HashSet::new();
    for eq in &model.equations {
        if crate::engine::control::cancelled() {
            return Default::default();
        }
        if !eq.is_local {
            continue;
        }
        let Some(id) = eq.lhs_expr else {
            continue;
        };
        if let ExprKind::Ident { name, .. } = model.exprs.get(id).kind {
            out.insert(name);
        }
    }
    out
}

enum DeclClass {
    Endogenous,
    Exo,
    Parameter,
    Local,
}

fn decl_class(model: &Model, name: Name, locals: &HashSet<Name>) -> Option<DeclClass> {
    match model.final_kind_or_written_if_excluded(name) {
        Some("var") => return Some(DeclClass::Endogenous),
        Some("varexo" | "varexo_det") => return Some(DeclClass::Exo),
        Some("parameters") => return Some(DeclClass::Parameter),
        _ => {}
    }
    if locals.contains(&name) {
        return Some(DeclClass::Local);
    }
    None
}

fn walk_occbin_expr_inner(
    model: &Model,
    id: ExprId,
    locals: &HashSet<Name>,
    owner: Option<&std::ops::Range<usize>>,
    out: &mut Vec<Diagnostic>,
) {
    let expr = model.exprs.get(id);
    match &expr.kind {
        ExprKind::Ident {
            name,
            timing,
            ident_span,
            timing_span,
        } => {
            if *timing != 0 {
                let end = timing_span.map(|t| t.end).unwrap_or(ident_span.end);
                out.push(error(
                    Span {
                        start: ident_span.start,
                        end,
                    },
                    "E182",
                    "Leads and lags on variables are forbidden in 'occbin_constraints'. Note that you can achieve the same effect by introducing an auxiliary variable in the model.",
                ));
                return;
            }
            let class = if let Some(owner) = owner {
                model
                    .model_expression_uses
                    .iter()
                    .find(|use_| {
                        use_.name == *name
                            && use_.span == *ident_span
                            && owner.contains(&use_.parse_order)
                    })
                    .and_then(
                        |use_| match model.symbol_kind_in_context(*name, use_.context) {
                            Some("var") => Some(DeclClass::Endogenous),
                            Some("varexo" | "varexo_det") => Some(DeclClass::Exo),
                            Some("parameters") => Some(DeclClass::Parameter),
                            Some("model_local_variable") => Some(DeclClass::Local),
                            _ => None,
                        },
                    )
            } else {
                decl_class(model, *name, locals)
            };
            match class {
                Some(DeclClass::Exo) => {
                    let n = model.name(*name);
                    out.push(error(
                        *ident_span,
                        "E182",
                        format!("Exogenous variable {n} cannot be used in 'occbin_constraints'."),
                    ));
                }
                Some(DeclClass::Local) => {
                    let n = model.name(*name);
                    out.push(error(
                        *ident_span,
                        "E182",
                        format!("Model local variable {n} cannot be used in 'occbin_constraints'."),
                    ));
                }
                Some(DeclClass::Endogenous | DeclClass::Parameter) | None => {}
            }
        }
        ExprKind::Expectation { arg, .. } => {
            out.push(error(
                expr.span,
                "E182",
                "The 'expectation' operator is forbidden in 'occbin_constraints'.",
            ));
            walk_occbin_expr_inner(model, *arg, locals, owner, out);
        }
        ExprKind::Call { callee, args } => {
            let callee_s = model.name(*callee);
            let forbidden = [
                (
                    "var_expectation",
                    "The 'var_expectation' operator is forbidden in 'occbin_constraints'.",
                ),
                (
                    "pac_expectation",
                    "The 'pac_expectation' operator is forbidden in 'occbin_constraints'.",
                ),
                (
                    "pac_target_nonstationary",
                    "The 'pac_target_nonstationary' operator is forbidden in 'occbin_constraints'.",
                ),
                (
                    "sum",
                    "The SUM() operator is forbidden in occbin_constraints block",
                ),
            ];
            if let Some((_, msg)) = forbidden
                .iter()
                .find(|(kw, _)| callee_s.eq_ignore_ascii_case(kw))
            {
                out.push(error(expr.span, "E182", *msg));
            }
            for arg in args {
                if crate::engine::control::cancelled() {
                    return Default::default();
                }
                walk_occbin_expr_inner(model, *arg, locals, owner, out);
            }
        }
        ExprKind::Unary { arg, .. } | ExprKind::SteadyState { arg } => {
            walk_occbin_expr_inner(model, *arg, locals, owner, out);
        }
        ExprKind::Binary { lhs, rhs, .. } => {
            walk_occbin_expr_inner(model, *lhs, locals, owner, out);
            walk_occbin_expr_inner(model, *rhs, locals, owner, out);
        }
        ExprKind::Number | ExprKind::String | ExprKind::Error | ExprKind::PathNamespace { .. } => {}
    }
}

fn check_equation_tags(model: &Model, illegal_block: bool, out: &mut Vec<Diagnostic>) {
    let mut trackers: BTreeMap<String, OccbinRegimeTracker> = BTreeMap::new();
    let mut first_span: BTreeMap<String, Span> = BTreeMap::new();
    for eq in &model.equations {
        if crate::engine::control::cancelled() {
            return Default::default();
        }
        if eq.is_local {
            continue;
        }
        let has_bind = eq.tag_map.contains_key("bind");
        let has_relax = eq.tag_map.contains_key("relax");
        if has_bind || has_relax {
            if !eq.tag_map.contains_key("name") {
                out.push(error(
                    eq.span,
                    "E173",
                    "An equation with a 'bind' or 'relax' tag must have a 'name' tag.",
                ));
            }
            check_tag_pieces(model, eq, "bind", out);
            check_tag_pieces(model, eq, "relax", out);
        }
        if has_bind && has_relax {
            let bind_names = split_names(eq.tag_map.get("bind").map(String::as_str).unwrap_or(""));
            let relax_names =
                split_names(eq.tag_map.get("relax").map(String::as_str).unwrap_or(""));
            if let Some(c) = bind_names
                .iter()
                .find(|n| relax_names.iter().any(|r| r == *n))
            {
                out.push(error(
                    eq.span,
                    "E176",
                    format!("The constraint '{c}' is both in the 'bind' and 'relax' tags"),
                ));
            }
        }
        for (key, span) in &eq.tag_twice {
            if crate::engine::control::cancelled() {
                return Default::default();
            }
            out.push(error(
                *span,
                "E256",
                format!("Tag '{key}' cannot be used twice for the same equation"),
            ));
        }
        if eq.tag_map.contains_key("mcp") && eq.complementarity.is_some() {
            out.push(error(
                eq.span,
                "E180",
                "Can't have both an 'mcp' tag and a complementarity condition after the perpendicular symbol",
            ));
        } else if eq.tag_map.contains_key("mcp") && eq.complementarity.is_none() {
            let mut d = Diagnostic::new(
                eq.span,
                Severity::Warning,
                "W170",
                "Specifying complementarity conditions with the 'mcp' tag is obsolete. Use ⟂ or _|_ after the equation.",
            );
            d.tags.push(2);
            out.push(d);
            check_mcp_form(model, eq, out);
        }
        if let Some(comp) = eq.complementarity.as_ref()
            && comp.matched.is_none()
        {
            out.push(error(
                comp.span,
                "E183",
                "Complementarity condition has an incorrect form",
            ));
        }
        if illegal_block {
            continue;
        }
        if !(has_bind || has_relax) {
            continue;
        }
        let Some(eq_name) = eq.tag_map.get("name") else {
            continue;
        };
        first_span.entry(eq_name.clone()).or_insert(eq.span);
        let bind_names = split_names(eq.tag_map.get("bind").map(String::as_str).unwrap_or(""));
        let relax_names = split_names(eq.tag_map.get("relax").map(String::as_str).unwrap_or(""));
        if bind_names.iter().any(|n| relax_names.contains(n)) {
            continue;
        }
        if bind_names
            .iter()
            .chain(relax_names.iter())
            .any(|n| !is_occbin_ident(n))
        {
            continue;
        }
        let tracker = trackers.entry(eq_name.clone()).or_default();
        if let Err(RegimeError::AlreadyPresent {
            bind,
            relax,
            earlier,
        }) = tracker.add_regime(
            &bind_names,
            &relax_names,
            RelatedDiagnostic::equation(model, eq, eq.span),
        ) {
            let rendering = render_bind_relax(&bind, &relax);
            out.push(error(
                eq.span,
                "E177",
                format!(
                    "The regime corresponding to {rendering} has already been declared for this equation"
                ),
            ).with_related(*earlier));
        }
    }
    // Heterogeneous bodies take the same `⟂` / `_|_` conditions, and 7.2 refuses
    // a wrong form there like anywhere else. The `mcp` reading of those bodies is
    // the heterogeneous owner's (02), so only the form check walks them here.
    for block in &model.heterogeneous_models {
        if crate::engine::control::cancelled() {
            return Default::default();
        }
        for eq in &block.equations {
            if crate::engine::control::cancelled() {
                return Default::default();
            }
            if let Some(comp) = eq.complementarity.as_ref()
                && comp.matched.is_none()
            {
                out.push(error(
                    comp.span,
                    "E183",
                    "Complementarity condition has an incorrect form",
                ));
            }
            if eq.tag_map.contains_key("mcp") && eq.complementarity.is_some() {
                out.push(error(
                    eq.span,
                    "E180",
                    "Can't have both an 'mcp' tag and a complementarity condition after the perpendicular symbol",
                ));
            } else if eq.tag_map.contains_key("mcp") {
                let mut warning = Diagnostic::new(
                    eq.span,
                    Severity::Warning,
                    "W170",
                    "Specifying complementarity conditions with the 'mcp' tag is obsolete. Use ⟂ or _|_ after the equation.",
                );
                warning.tags.push(2);
                out.push(warning);
                check_het_mcp_lhs(model, eq, out);
            }
        }
    }
    if illegal_block {
        return;
    }
    for (eq_name, tracker) in &trackers {
        if crate::engine::control::cancelled() {
            return Default::default();
        }
        if let Some(missing) = tracker.first_missing() {
            let span = first_span[eq_name];
            let rendering = render_bind_relax(&missing.0, &missing.1);
            out.push(error(
                span,
                "E172",
                format!(
                    "for equation '{eq_name}', the regime corresponding to {rendering} is not defined"
                ),
            ));
        }
    }
}

fn check_tag_pieces(model: &Model, eq: &Equation, key: &str, out: &mut Vec<Diagnostic>) {
    check_tag_pieces_at(model, eq, key, None, out);
}

fn check_tag_pieces_at(
    model: &Model,
    eq: &Equation,
    key: &str,
    context: Option<SymbolContext>,
    out: &mut Vec<Diagnostic>,
) {
    let Some(value) = eq.tag_map.get(key) else {
        return;
    };
    for piece in split_names(value) {
        if crate::engine::control::cancelled() {
            return Default::default();
        }
        if !is_occbin_ident(piece) {
            out.push(error(
                eq.span,
                "E185",
                format!(
                    "The string '{piece}' is not a valid Occbin constraint name (contains unauthorized characters)"
                ),
            ));
        } else {
            let param = format!("occbin_{piece}_bind");
            let collision = context.map_or_else(
                || name_is_var_like(model, &param),
                |context| {
                    model
                        .intern
                        .lookup(&param)
                        .and_then(|name| model.symbol_kind_in_context(name, context))
                        .is_some_and(|kind| kind != "parameters")
                },
            );
            if collision {
                out.push(error(
                    eq.span,
                    "E185",
                    format!(
                        "The name '{param}' is already used. Please use another name for Occbin constraint '{piece}'"
                    ),
                ));
            }
        }
    }
}

fn render_bind_relax(bind: &[String], relax: &[String]) -> String {
    let mut s = String::new();
    if !bind.is_empty() {
        s.push_str("bind='");
        s.push_str(&bind.join(","));
    }
    if !bind.is_empty() && !relax.is_empty() {
        s.push_str("' and ");
    }
    if !relax.is_empty() {
        s.push_str("relax='");
        s.push_str(&relax.join(","));
    }
    s.push('\'');
    s
}

#[derive(Default)]
struct OccbinRegimeTracker {
    constraints: Vec<String>,
    regimes_present: BTreeMap<Vec<bool>, RelatedDiagnostic>,
}

enum RegimeError {
    AlreadyPresent {
        bind: Vec<String>,
        relax: Vec<String>,
        earlier: Box<RelatedDiagnostic>,
    },
}

impl OccbinRegimeTracker {
    fn add_regime(
        &mut self,
        constraints_bind: &[&str],
        constraints_relax: &[&str],
        owner: RelatedDiagnostic,
    ) -> Result<(), RegimeError> {
        let mut bind_sorted: Vec<&str> = constraints_bind.to_vec();
        bind_sorted.sort_unstable();
        bind_sorted.dedup();
        let mut relax_sorted: Vec<&str> = constraints_relax.to_vec();
        relax_sorted.sort_unstable();
        relax_sorted.dedup();
        let mut constraints_union: Vec<&str> = bind_sorted
            .iter()
            .copied()
            .chain(relax_sorted.iter().copied())
            .collect();
        constraints_union.sort_unstable();
        constraints_union.dedup();
        for c in &constraints_union {
            if crate::engine::control::cancelled() {
                return Ok(());
            }
            if !self.constraints.iter().any(|x| x == c) {
                self.constraints.push((*c).to_string());
                let regimes_copy: Vec<_> = self
                    .regimes_present
                    .iter()
                    .map(|(bits, site)| (bits.clone(), site.clone()))
                    .collect();
                self.regimes_present.clear();
                for (r, site) in regimes_copy {
                    if crate::engine::control::cancelled() {
                        return Ok(());
                    }
                    let mut r0 = r.clone();
                    let mut r1 = r;
                    r0.push(false);
                    r1.push(true);
                    self.regimes_present.insert(r0, site.clone());
                    self.regimes_present.insert(r1, site);
                }
            }
        }
        let mut new_regime_template = vec![false; self.constraints.len()];
        for c in constraints_bind {
            if crate::engine::control::cancelled() {
                return Ok(());
            }
            if let Some(i) = self.constraints.iter().position(|x| x == c) {
                new_regime_template[i] = true;
            }
        }
        let mut new_regimes: BTreeSet<Vec<bool>> = BTreeSet::new();
        new_regimes.insert(new_regime_template);
        let mut all_sorted = self.constraints.clone();
        all_sorted.sort();
        let mut union_owned: Vec<String> =
            constraints_union.iter().map(|s| (*s).to_string()).collect();
        union_owned.sort();
        let not_mentioned: Vec<String> = all_sorted
            .into_iter()
            .filter(|c| !union_owned.iter().any(|u| u == c))
            .collect();
        for c in not_mentioned {
            if crate::engine::control::cancelled() {
                return Ok(());
            }
            let i = self.constraints.iter().position(|x| x == &c).unwrap();
            let copy: Vec<Vec<bool>> = new_regimes.iter().cloned().collect();
            for mut r2 in copy {
                if crate::engine::control::cancelled() {
                    return Ok(());
                }
                r2[i] = true;
                new_regimes.insert(r2);
            }
        }
        for r in new_regimes {
            if crate::engine::control::cancelled() {
                return Ok(());
            }
            if let Some(earlier) = self.regimes_present.get(&r) {
                let (bind, relax) = self.convert_bit_vector(&r);
                return Err(RegimeError::AlreadyPresent {
                    bind,
                    relax,
                    earlier: Box::new(earlier.clone()),
                });
            }
            self.regimes_present.insert(r, owner.clone());
        }
        Ok(())
    }

    fn convert_bit_vector(&self, r: &[bool]) -> (Vec<String>, Vec<String>) {
        let mut bind = Vec::new();
        let mut relax = Vec::new();
        for (i, c) in self.constraints.iter().enumerate() {
            if crate::engine::control::cancelled() {
                return Default::default();
            }
            if r.get(i).copied().unwrap_or(false) {
                bind.push(c.clone());
            } else {
                relax.push(c.clone());
            }
        }
        (bind, relax)
    }

    fn first_missing(&self) -> Option<(Vec<String>, Vec<String>)> {
        if self.constraints.is_empty() {
            return None;
        }
        let mut r = vec![false; self.constraints.len()];
        loop {
            if crate::engine::control::cancelled() {
                return Default::default();
            }
            if !self.regimes_present.contains_key(&r) {
                return Some(self.convert_bit_vector(&r));
            }
            if let Some(idx) = r.iter().position(|b| !b) {
                for b in r.iter_mut().take(idx) {
                    if crate::engine::control::cancelled() {
                        return Default::default();
                    }
                    *b = false;
                }
                r[idx] = true;
            } else {
                break;
            }
        }
        None
    }
}
