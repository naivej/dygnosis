//! OccBin / complementarity Errors `E170`–`E177`, `E180`–`E185` and obsolete-`mcp` Warning `W170`.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use crate::diagnostic::{Diagnostic, Severity};
use crate::expr::{BinOp, ExprId, ExprKind};
use crate::intern::Name;
use crate::lexer::{tokenize, Token, TokenKind};
use crate::model::{Equation, Model, OccbinConstraint, OccbinExpr};
use crate::span::Span;

pub fn check_occbin(model: &Model) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    check_blocks(model, &mut out);
    let illegal_block = out.iter().any(|d| d.code == "E170" || d.code == "E171");
    check_constraint_rows(model, &mut out);
    check_equation_tags(model, illegal_block, &mut out);
    out
}

fn error(span: Span, code: &str, message: impl Into<String>) -> Diagnostic {
    Diagnostic::new(span, Severity::Error, code, message)
}

fn split_names(s: &str) -> Vec<&str> {
    s.split(',').filter(|p| !p.is_empty()).collect()
}

fn is_occbin_ident(s: &str) -> bool {
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
            "Multiple 'occbin_constraints' blocks are not allowed. Keep a single block.",
        ));
    } else if blocks.len() == 1 && model.occbin_constraints.len() > 2 {
        out.push(error(
            model.occbin_constraints[2].span,
            "E171",
            "Only up to two constraints are supported in 'occbin_constraints'. Remove the extra constraint.",
        ));
    }
}

fn check_constraint_rows(model: &Model, out: &mut Vec<Diagnostic>) {
    let mentioned = mentioned_constraints(model);
    for c in &model.occbin_constraints {
        if !is_occbin_ident(&c.name) {
            out.push(error(
                c.name_span,
                "E185",
                format!(
                    "The string '{}' is not a valid Occbin constraint name (contains unauthorized characters). Use letters, digits, and underscores.",
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
                    "The name '{param}' is already used. Use another name for OccBin constraint '{}'.",
                    c.name
                ),
            ));
        }
        if !mentioned.contains(c.name.as_str()) {
            out.push(error(
                c.name_span,
                "E175",
                format!(
                    "No equation has been declared for constraint '{}'. Add a model equation tagged bind or relax with that name.",
                    c.name
                ),
            ));
        }
        match bind_root(model, c) {
            BindRoot::Missing { span } => {
                out.push(error(
                    span,
                    "E174",
                    format!(
                        "The 'bind' expression is missing in constraint '{}'. Add a bind inequality.",
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
        if let Some(relax) = c.relax.as_ref() {
            if let Some(id) = relax.expr {
                let kind = &model.exprs.get(id).kind;
                if is_binary(kind) && !is_inequality(kind) {
                    out.push(error(
                        relax.span,
                        "E181",
                        "The 'relax' expression must be an inequality constraint.",
                    ));
                }
            }
        }
        let locals = model_locals(model);
        for expr in occbin_exprs(c) {
            if let Some(id) = expr.expr {
                walk_occbin_expr_inner(model, id, &locals, out);
            }
        }
    }
    if !model.occbin_constraints_blocks.is_empty() {
        let tokens = tokenize(&model.source);
        for &block in &model.occbin_constraints_blocks {
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
        i += 1;
    }
    if i < in_block.len() {
        i += 1;
    }
    let mut seen: HashSet<String> = HashSet::new();
    while i < in_block.len() {
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
                    format!(
                        "The '{clause}' clause is declared multiple times. Keep a single '{clause}' in this constraint."
                    ),
                ));
            }
            while i < in_block.len() && in_block[i].kind != TokenKind::Semi {
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
    if model.endogenous.iter().any(|d| d.name == name) {
        return Some(DeclClass::Endogenous);
    }
    if model.exogenous.iter().any(|d| d.name == name)
        || model.deterministic_exogenous.iter().any(|d| d.name == name)
    {
        return Some(DeclClass::Exo);
    }
    if model.parameters.iter().any(|d| d.name == name) {
        return Some(DeclClass::Parameter);
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
            match decl_class(model, *name, locals) {
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
            walk_occbin_expr_inner(model, *arg, locals, out);
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
                walk_occbin_expr_inner(model, *arg, locals, out);
            }
        }
        ExprKind::Unary { arg, .. } | ExprKind::SteadyState { arg } => {
            walk_occbin_expr_inner(model, *arg, locals, out);
        }
        ExprKind::Binary { lhs, rhs, .. } => {
            walk_occbin_expr_inner(model, *lhs, locals, out);
            walk_occbin_expr_inner(model, *rhs, locals, out);
        }
        ExprKind::Number | ExprKind::String | ExprKind::Error => {}
    }
}

fn check_equation_tags(model: &Model, illegal_block: bool, out: &mut Vec<Diagnostic>) {
    let mut trackers: BTreeMap<String, OccbinRegimeTracker> = BTreeMap::new();
    let mut first_span: BTreeMap<String, Span> = BTreeMap::new();
    for eq in &model.equations {
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
                    format!(
                        "The constraint '{c}' is both in the 'bind' and 'relax' tags. Use one or the other on this equation."
                    ),
                ));
            }
        }
        if eq.tag_map.contains_key("mcp") && eq.complementarity.is_some() {
            out.push(error(
                eq.span,
                "E180",
                "Can't have both an 'mcp' tag and a complementarity condition after the perpendicular symbol. Keep one form.",
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
        }
        if let Some(comp) = eq.complementarity.as_ref() {
            if comp.matched.is_none() {
                out.push(error(
                    comp.span,
                    "E183",
                    "Complementarity condition has an incorrect form. Use an inequality on a contemporaneous endogenous, with constant bounds.",
                ));
            }
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
        if let Err(RegimeError::AlreadyPresent { bind, relax }) =
            tracker.add_regime(&bind_names, &relax_names)
        {
            let rendering = render_bind_relax(&bind, &relax);
            out.push(error(
                eq.span,
                "E177",
                format!(
                    "The regime corresponding to {rendering} has already been declared for this equation. Remove the duplicate."
                ),
            ));
        }
    }
    if illegal_block {
        return;
    }
    for (eq_name, tracker) in &trackers {
        if let Some(missing) = tracker.first_missing() {
            let span = first_span[eq_name];
            let rendering = render_bind_relax(&missing.0, &missing.1);
            out.push(error(
                span,
                "E172",
                format!(
                    "For equation '{eq_name}', the regime corresponding to {rendering} is not defined. Add the missing bind/relax equation."
                ),
            ));
        }
    }
}

fn check_tag_pieces(model: &Model, eq: &Equation, key: &str, out: &mut Vec<Diagnostic>) {
    let Some(value) = eq.tag_map.get(key) else {
        return;
    };
    for piece in split_names(value) {
        if !is_occbin_ident(piece) {
            out.push(error(
                eq.span,
                "E185",
                format!(
                    "The string '{piece}' is not a valid Occbin constraint name (contains unauthorized characters). Use letters, digits, and underscores."
                ),
            ));
        } else {
            let param = format!("occbin_{piece}_bind");
            if name_is_var_like(model, &param) {
                out.push(error(
                    eq.span,
                    "E185",
                    format!(
                        "The name '{param}' is already used. Use another name for OccBin constraint '{piece}'."
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
    regimes_present: BTreeSet<Vec<bool>>,
}

enum RegimeError {
    AlreadyPresent {
        bind: Vec<String>,
        relax: Vec<String>,
    },
}

impl OccbinRegimeTracker {
    fn add_regime(
        &mut self,
        constraints_bind: &[&str],
        constraints_relax: &[&str],
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
            if !self.constraints.iter().any(|x| x == c) {
                self.constraints.push((*c).to_string());
                let regimes_copy: Vec<Vec<bool>> = self.regimes_present.iter().cloned().collect();
                self.regimes_present.clear();
                for r in regimes_copy {
                    let mut r0 = r.clone();
                    let mut r1 = r;
                    r0.push(false);
                    r1.push(true);
                    self.regimes_present.insert(r0);
                    self.regimes_present.insert(r1);
                }
            }
        }
        let mut new_regime_template = vec![false; self.constraints.len()];
        for c in constraints_bind {
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
            let i = self.constraints.iter().position(|x| x == &c).unwrap();
            let copy: Vec<Vec<bool>> = new_regimes.iter().cloned().collect();
            for mut r2 in copy {
                r2[i] = true;
                new_regimes.insert(r2);
            }
        }
        for r in new_regimes {
            if !self.regimes_present.insert(r.clone()) {
                let (bind, relax) = self.convert_bit_vector(&r);
                return Err(RegimeError::AlreadyPresent { bind, relax });
            }
        }
        Ok(())
    }

    fn convert_bit_vector(&self, r: &[bool]) -> (Vec<String>, Vec<String>) {
        let mut bind = Vec::new();
        let mut relax = Vec::new();
        for (i, c) in self.constraints.iter().enumerate() {
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
            if !self.regimes_present.contains(&r) {
                return Some(self.convert_bit_vector(&r));
            }
            if let Some(idx) = r.iter().position(|b| !b) {
                for b in r.iter_mut().take(idx) {
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
