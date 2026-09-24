//! Dynare 7.2 semi-structural refusals on the written `.mod` file.

use std::collections::{BTreeMap, HashSet};

use crate::diagnostic::{Diagnostic, Severity};
use crate::expr::{BinOp, ExprId, ExprKind, UnOp};
use crate::intern::Name;
use crate::model::{
    Model, NamedModelOperatorKind, PacTargetComponentRow, PacTargetInfoRow, SemiStructuralCommand,
    SemiStructuralKind, SemiStructuralOption, SemiStructuralValue, WrittenExpression,
};
use crate::span::Span;

fn error(span: Span, code: &str, message: impl Into<String>) -> Diagnostic {
    Diagnostic::new(span, Severity::Error, code, message)
}

fn warning(span: Span, code: &str, message: impl Into<String>) -> Diagnostic {
    Diagnostic::new(span, Severity::Warning, code, message)
}

fn option<'a>(command: &'a SemiStructuralCommand, name: &str) -> Option<&'a SemiStructuralOption> {
    command
        .options
        .iter()
        .rev()
        .find(|option| option.name.eq_ignore_ascii_case(name))
}

fn symbol_option(command: &SemiStructuralCommand, name: &str) -> Option<(Name, Span)> {
    match &option(command, name)?.value {
        SemiStructuralValue::Symbol { name, span } => Some((*name, *span)),
        _ => None,
    }
}

fn expression_option<'a>(
    command: &'a SemiStructuralCommand,
    name: &str,
) -> Option<&'a WrittenExpression> {
    match &option(command, name)?.value {
        SemiStructuralValue::Expression(expression) => Some(expression),
        _ => None,
    }
}

fn tags_option<'a>(command: &'a SemiStructuralCommand, name: &str) -> Option<&'a [(String, Span)]> {
    match &option(command, name)?.value {
        SemiStructuralValue::Tags(tags) => Some(tags),
        _ => None,
    }
}

fn kind_label(kind: SemiStructuralKind) -> &'static str {
    match kind {
        SemiStructuralKind::VarModel => "var_model",
        SemiStructuralKind::TrendComponentModel => "trend_component_model",
        SemiStructuralKind::VarExpectationModel => "var_expectation_model",
        SemiStructuralKind::PacModel => "pac_model",
    }
}

fn required(command: &SemiStructuralCommand, name: &str) -> Diagnostic {
    error(
        command.span,
        "E439",
        format!(
            "You must pass the '{name}' option to the '{}' statement.",
            kind_label(command.kind)
        ),
    )
}

fn declared_before(model: &Model, name: Name, at: u32) -> bool {
    model
        .endogenous
        .iter()
        .chain(&model.exogenous)
        .chain(&model.deterministic_exogenous)
        .chain(&model.parameters)
        .chain(&model.model_local_variables)
        .any(|declaration| declaration.name == name && declaration.span.start < at)
        || model
            .trend_vars
            .iter()
            .any(|trend| trend.name == name && trend.span.start < at)
        || model.equations.iter().any(|equation| {
            equation.is_local
                && equation.span.start < at
                && equation.lhs_expr.is_some_and(|id| {
                    model
                        .exprs
                        .walk_idents(id)
                        .any(|reference| reference.name == name)
                })
        })
}

fn is_parameter_before(model: &Model, name: Name, at: u32) -> bool {
    model
        .parameters
        .iter()
        .any(|declaration| declaration.name == name && declaration.span.start < at)
}

/// ParsingDriver and shared parse-level name/option checks. A parse refusal
/// prevents Dynare from running checkPass or transformPass.
pub fn check_parse(model: &Model) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for (name, span) in &model.option_twice {
        if model
            .semi_structural_commands
            .iter()
            .any(|command| command.span.start <= span.start && span.end <= command.span.end)
        {
            out.push(error(
                *span,
                "E271",
                format!("option {name} declared twice"),
            ));
        }
    }
    let mut model_names: HashSet<(SemiStructuralKind, Name)> = HashSet::new();
    for command in &model.semi_structural_commands {
        match command.kind {
            SemiStructuralKind::VarModel => {
                if option(command, "model_name").is_none() {
                    out.push(required(command, "model_name"));
                } else if option(command, "eqtags").is_none() {
                    out.push(required(command, "eqtags"));
                }
            }
            SemiStructuralKind::TrendComponentModel => {
                for name in ["model_name", "eqtags", "targets"] {
                    if option(command, name).is_none() {
                        out.push(required(command, name));
                        break;
                    }
                }
            }
            SemiStructuralKind::VarExpectationModel => {
                // Its discount is an ordinary expression, not a model
                // expression. ParsingDriver refuses forbidden symbol uses
                // while reading it, before checking the discount's form.
                if let Some(refusal) = expression_option(command, "discount")
                    .and_then(|discount| discount.expr)
                    .and_then(|id| crate::check_mom::outside_model_expression(model, id))
                {
                    out.push(refusal);
                    continue;
                }
                let variable = symbol_option(command, "variable");
                let expression = expression_option(command, "expression");
                if variable.is_some() && expression.is_some() {
                    out.push(error(
                        command.span,
                        "E441",
                        "You can't pass both the 'variable' or the 'expression' options to the var_expectation_model statement.",
                    ));
                } else if variable.is_none() && expression.is_none() {
                    out.push(error(
                        command.span,
                        "E441",
                        "You must pass either the 'variable' or the 'expression' option to the var_expectation_model statement.",
                    ));
                }
                // ParsingDriver calls SymbolTable::getID directly here. An
                // unknown variable aborts the pin without a diagnostic line.
                if let Some((name, _)) = variable {
                    if !declared_before(model, name, command.span.start) {
                        continue;
                    }
                }
                if let Some(discount) = expression_option(command, "discount") {
                    if !valid_var_discount(model, discount) {
                        out.push(error(
                            discount.span,
                            "E442",
                            "The discount factor must be a constant expression or a parameter",
                        ));
                    }
                }
                if let Some(row) = option(command, "time_shift") {
                    if let SemiStructuralValue::Integer { text, span } = &row.value {
                        if text.parse::<i32>().is_ok_and(|shift| shift > 0) {
                            out.push(error(
                                *span,
                                "E443",
                                "The 'time_shift' option must be a non-positive integer",
                            ));
                        }
                    }
                }
                for name in ["model_name", "auxiliary_model_name", "horizon"] {
                    if option(command, name).is_none() {
                        out.push(required(command, name));
                        break;
                    }
                }
            }
            SemiStructuralKind::PacModel => {
                if let Some((discount, span)) = symbol_option(command, "discount") {
                    if !declared_before(model, discount, command.span.start) {
                        out.push(error(
                            span,
                            "E058",
                            format!("Unknown symbol: {}.", model.name(discount)),
                        ));
                    } else if !is_parameter_before(model, discount, command.span.start) {
                        out.push(error(
                            span,
                            "E444",
                            format!("{} is not a parameter", model.name(discount)),
                        ));
                    }
                } else {
                    out.push(required(command, "discount"));
                }
                if option(command, "model_name").is_none() {
                    out.push(required(command, "model_name"));
                }
            }
        }
        if let Some((name, span)) = symbol_option(command, "model_name") {
            if !model_names.insert((command.kind, name)) {
                let article = match command.kind {
                    SemiStructuralKind::VarModel => "VAR model",
                    SemiStructuralKind::TrendComponentModel => "trend component model",
                    SemiStructuralKind::VarExpectationModel => "var_expectation_model",
                    SemiStructuralKind::PacModel => "PAC model",
                };
                out.push(error(
                    span,
                    "E440",
                    format!(
                        "a {article} already exists with the name {}",
                        model.name(name)
                    ),
                ));
            }
        }
    }
    for block in &model.deterministic_trends {
        for row in &block.rows {
            if let Some(refusal) = row
                .expression
                .expr
                .and_then(|id| crate::check_mom::outside_model_expression(model, id))
            {
                out.push(refusal);
                continue;
            }
            if !declared_before(model, row.name, row.name_span.start) {
                out.push(error(
                    row.name_span,
                    "E058",
                    format!("Unknown symbol: {}.", model.name(row.name)),
                ));
            }
        }
    }
    for operator in &model.named_model_operators {
        let label = match operator.kind {
            NamedModelOperatorKind::VarExpectation => "var_expectation",
            NamedModelOperatorKind::PacExpectation => "pac_expectation",
            NamedModelOperatorKind::PacTargetNonstationary => "pac_target_nonstationary",
        };
        if model.epilogue_block.is_some_and(|block| {
            block.start <= operator.span.start && operator.span.end <= block.end
        }) {
            out.push(error(
                operator.operator_span,
                "E445",
                format!("The '{label}' operator is forbidden in 'epilogue'."),
            ));
        } else if model
            .occbin_constraints_blocks
            .iter()
            .any(|block| block.start <= operator.span.start && operator.span.end <= block.end)
        {
            out.push(error(
                operator.operator_span,
                "E182",
                format!("The '{label}' operator is forbidden in 'occbin_constraints'."),
            ));
        }
    }
    out.sort_by_key(|diag| (diag.span.start, diag.span.end));
    out.into_iter().take(1).collect()
}

fn valid_var_discount(model: &Model, expression: &WrittenExpression) -> bool {
    let Some(id) = expression.expr else {
        return true;
    };
    let node = model.exprs.get(id);
    if node.interned.is_some() || matches!(node.kind, ExprKind::Number) {
        return true;
    }
    matches!(&node.kind, ExprKind::Ident { name, timing: 0, .. } if is_parameter_before(model, *name, expression.span.start))
}

/// PAC target fields and the independent deterministic-trend warning.
pub fn check_check(model: &Model) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    let mut target_infos: BTreeMap<&str, Vec<&crate::model::PacTargetInfoBlock>> = BTreeMap::new();
    for block in &model.pac_target_info {
        target_infos
            .entry(model.name(block.name))
            .or_default()
            .push(block);
    }
    let pac_commands: BTreeMap<_, _> = model
        .semi_structural_commands
        .iter()
        .filter(|command| command.kind == SemiStructuralKind::PacModel)
        .filter_map(|command| {
            symbol_option(command, "model_name").map(|(name, _)| (model.name(name), command))
        })
        .collect();
    // PacModelTable::checkPass walks its sorted growth, auxname, then kind
    // maps. A refusal exits immediately, before target-info field checks.
    for field in ["growth", "auxname", "kind"] {
        for (name, command) in &pac_commands {
            let Some(row) = option(command, field) else {
                continue;
            };
            if target_infos.contains_key(name) {
                let article = if field == "auxname" { "an" } else { "a" };
                return vec![error(
                    row.span,
                    "E437",
                    format!("for PAC model '{name}', it is not possible to declare {article} '{field}' option in the 'pac_model' command when there is also a 'pac_target_info' block"),
                )];
            }
            if field == "kind" && option(command, "auxiliary_model_name").is_none() {
                return vec![error(
                    row.span,
                    "E437",
                    format!("for PAC model '{name}', it is not possible to declare a 'kind' option in the 'pac_model' command since this is a MCE model"),
                )];
            }
        }
    }
    for (name, blocks) in target_infos {
        let span = blocks[0].span;
        let mut target = false;
        let mut nonstationary_aux = false;
        let mut nonstationary_component = false;
        let mut components = Vec::new();
        for block in blocks {
            for row in &block.rows {
                match row {
                    PacTargetInfoRow::Target(_) => target = true,
                    PacTargetInfoRow::AuxnameTargetNonstationary { .. } => nonstationary_aux = true,
                    PacTargetInfoRow::Component(component) => components.push(component),
                }
            }
        }
        if !target {
            return vec![error(
                span,
                "E438",
                format!("the block 'pac_target_info({name})' is missing the 'target' statement"),
            )];
        }
        if !nonstationary_aux {
            return vec![error(
                span,
                "E438",
                format!("the block 'pac_target_info({name})' is missing the 'auxname_target_nonstationary' statement"),
            )];
        }
        for component in components {
            let auxname = component
                .rows
                .iter()
                .any(|row| matches!(row, PacTargetComponentRow::Auxname { .. }));
            let kind = component.rows.iter().rev().find_map(|row| match row {
                PacTargetComponentRow::Kind { text, .. } => Some(text.as_str()),
                _ => None,
            });
            let growth = component
                .rows
                .iter()
                .any(|row| matches!(row, PacTargetComponentRow::Growth(_)));
            if !auxname {
                return vec![error(
                    component.span,
                    "E438",
                    format!("the block 'pac_target_info({name})' is missing the 'auxname' statement in some 'component'"),
                )];
            }
            let Some(kind) = kind else {
                return vec![error(
                    component.span,
                    "E438",
                    format!("the block 'pac_target_info({name})' is missing the 'kind' statement in some 'component'"),
                )];
            };
            if kind.eq_ignore_ascii_case("ll") && growth {
                return vec![error(
                    component.span,
                    "E438",
                    format!("in the block 'pac_target_info({name})', a component of 'kind ll' (i.e. stationary) has a 'growth' option. This is not permitted."),
                )];
            }
            if kind.eq_ignore_ascii_case("dd") || kind.eq_ignore_ascii_case("dl") {
                nonstationary_component = true;
            }
        }
        if !nonstationary_component {
            return vec![error(
                span,
                "E438",
                format!("the block 'pac_target_info({name})' must contain at least one nonstationary component (i.e. of 'kind' equal to either 'dd' or 'dl')."),
            )];
        }
    }
    // Reading an undeclared name in these model-expression slots creates an
    // implicit exogenous in Dynare. If the main model never uses it, checkPass
    // refuses with the existing unused-exogenous sentence. PAC growth (both
    // option and component row) exempts its exogenous names from that check.
    let unused = unused_implicit_exogenous(model);
    if let Some((_, span)) = unused.first() {
        let names = unused
            .iter()
            .map(|(name, _)| model.name(*name))
            .collect::<Vec<_>>()
            .join(" ");
        return vec![error(
            *span,
            "E021",
            format!("{names} not used in model block. To bypass this error, use the `nostrict` option. This may lead to crashes or unexpected behavior."),
        )];
    }
    for block in &model.deterministic_trends {
        for row in &block.rows {
            if !model
                .endogenous
                .iter()
                .any(|declaration| declaration.name == row.name)
            {
                out.push(warning(
                    row.name_span,
                    "W206",
                    format!(
                        "Warning: Non-variable symbol used in deterministic_trends: {}",
                        model.name(row.name)
                    ),
                ));
            }
        }
    }
    out
}

fn unused_implicit_exogenous(model: &Model) -> Vec<(Name, Span)> {
    let mut declared: HashSet<Name> = model
        .endogenous
        .iter()
        .chain(&model.exogenous)
        .chain(&model.deterministic_exogenous)
        .chain(&model.parameters)
        .chain(&model.model_local_variables)
        .map(|decl| decl.name)
        .collect();
    declared.extend(model.mod_file_locals.iter().copied());
    for equation in model.equations.iter().filter(|equation| equation.is_local) {
        if let Some(lhs) = equation.lhs_expr {
            declared.extend(model.exprs.walk_idents(lhs).map(|reference| reference.name));
        }
    }
    let mut exempt = HashSet::new();
    let mut expressions = Vec::new();
    for command in &model.semi_structural_commands {
        if command.kind == SemiStructuralKind::VarExpectationModel {
            if let Some(expression) = expression_option(command, "expression") {
                expressions.push(expression);
            }
        } else if command.kind == SemiStructuralKind::PacModel {
            if let Some(growth) = expression_option(command, "growth") {
                if let Some(id) = growth.expr {
                    exempt.extend(model.exprs.walk_idents(id).map(|reference| reference.name));
                }
            }
        }
    }
    for block in &model.pac_target_info {
        for row in &block.rows {
            match row {
                PacTargetInfoRow::Target(expression) => expressions.push(expression),
                PacTargetInfoRow::Component(component) => {
                    expressions.push(&component.component);
                    for row in &component.rows {
                        if let PacTargetComponentRow::Growth(growth) = row {
                            if let Some(id) = growth.expr {
                                exempt.extend(
                                    model.exprs.walk_idents(id).map(|reference| reference.name),
                                );
                            }
                        }
                    }
                }
                PacTargetInfoRow::AuxnameTargetNonstationary { .. } => {}
            }
        }
    }
    let mut seen = HashSet::new();
    let mut unused = Vec::new();
    for expression in expressions {
        let Some(id) = expression.expr else { continue };
        for reference in model.exprs.walk_idents(id) {
            if declared.contains(&reference.name)
                || exempt.contains(&reference.name)
                || !seen.insert(reference.name)
            {
                continue;
            }
            let used_in_model = model.equations.iter().any(|equation| {
                model
                    .ident_refs(equation)
                    .iter()
                    .any(|other| other.name == reference.name)
            });
            if !used_in_model {
                unused.push((reference.name, reference.span));
            }
        }
    }
    unused.sort_by_key(|(_, span)| span.start);
    unused
}

fn tagged_equation<'a>(model: &'a Model, tag: &str) -> Option<&'a crate::model::Equation> {
    model.equations.iter().find(|equation| {
        !equation.is_local
            && (equation.name == tag
                || equation
                    .tag_map
                    .get("name")
                    .is_some_and(|value| value == tag))
    })
}

fn selected_equations<'a>(
    model: &'a Model,
    command: &SemiStructuralCommand,
) -> Vec<(&'a crate::model::Equation, String, Span)> {
    let mut out = Vec::new();
    if let Some(tags) = tags_option(command, "eqtags") {
        for (tag, span) in tags {
            if let Some(equation) = tagged_equation(model, tag) {
                out.push((equation, tag.clone(), *span));
            }
        }
    }
    out
}

fn model_names(model: &Model, kind: SemiStructuralKind) -> HashSet<Name> {
    model
        .semi_structural_commands
        .iter()
        .filter(|command| command.kind == kind)
        .filter_map(|command| symbol_option(command, "model_name").map(|(name, _)| name))
        .collect()
}

#[derive(Clone, Copy)]
struct ResolvedRef {
    name: Name,
    timing: i32,
    span: Span,
}

#[derive(Clone, Copy)]
struct ResolvedOperator {
    kind: NamedModelOperatorKind,
    name: Name,
    span: Span,
}

fn local_rhs(model: &Model, name: Name, before: u32) -> Option<ExprId> {
    model
        .equations
        .iter()
        .rev()
        .filter(|equation| equation.is_local && equation.span.start < before)
        .find(|equation| {
            equation.lhs_expr.is_some_and(|lhs| {
                matches!(&model.exprs.get(lhs).kind, ExprKind::Ident { name: lhs_name, .. } if *lhs_name == name)
            })
        })
        .and_then(|equation| equation.rhs_expr)
}

fn resolved_refs(model: &Model, id: ExprId) -> Vec<ResolvedRef> {
    let mut out = Vec::new();
    collect_refs(model, id, 0, None, &mut HashSet::new(), &mut out);
    out
}

fn collect_refs(
    model: &Model,
    id: ExprId,
    offset: i32,
    origin: Option<Span>,
    visiting: &mut HashSet<Name>,
    out: &mut Vec<ResolvedRef>,
) {
    match &model.exprs.get(id).kind {
        ExprKind::Ident {
            name,
            timing,
            ident_span,
            ..
        } => {
            if let Some(rhs) = local_rhs(model, *name, ident_span.start) {
                if visiting.insert(*name) {
                    collect_refs(
                        model,
                        rhs,
                        offset.saturating_add(*timing),
                        origin.or(Some(*ident_span)),
                        visiting,
                        out,
                    );
                    visiting.remove(name);
                }
            } else {
                out.push(ResolvedRef {
                    name: *name,
                    timing: offset.saturating_add(*timing),
                    span: origin.unwrap_or(*ident_span),
                });
            }
        }
        ExprKind::Unary { arg, .. }
        | ExprKind::SteadyState { arg }
        | ExprKind::Expectation { arg, .. } => {
            collect_refs(model, *arg, offset, origin, visiting, out);
        }
        ExprKind::Binary { lhs, rhs, .. } => {
            collect_refs(model, *lhs, offset, origin, visiting, out);
            collect_refs(model, *rhs, offset, origin, visiting, out);
        }
        ExprKind::Call { args, .. } => {
            for arg in args {
                collect_refs(model, *arg, offset, origin, visiting, out);
            }
        }
        ExprKind::Number | ExprKind::String | ExprKind::Error => {}
    }
}

fn resolved_operators(model: &Model, id: ExprId) -> Vec<ResolvedOperator> {
    let mut out = Vec::new();
    collect_operators(model, id, None, &mut HashSet::new(), &mut out);
    out
}

fn collect_operators(
    model: &Model,
    id: ExprId,
    origin: Option<Span>,
    visiting: &mut HashSet<Name>,
    out: &mut Vec<ResolvedOperator>,
) {
    let node = model.exprs.get(id);
    match &node.kind {
        ExprKind::Ident {
            name, ident_span, ..
        } => {
            if let Some(rhs) = local_rhs(model, *name, ident_span.start) {
                if visiting.insert(*name) {
                    collect_operators(model, rhs, origin.or(Some(*ident_span)), visiting, out);
                    visiting.remove(name);
                }
            }
        }
        ExprKind::Unary { arg, .. }
        | ExprKind::SteadyState { arg }
        | ExprKind::Expectation { arg, .. } => {
            collect_operators(model, *arg, origin, visiting, out);
        }
        ExprKind::Binary { lhs, rhs, .. } => {
            collect_operators(model, *lhs, origin, visiting, out);
            collect_operators(model, *rhs, origin, visiting, out);
        }
        ExprKind::Call { args, .. } => {
            if let Some(operator) = model
                .named_model_operators
                .iter()
                .find(|operator| operator.span == node.span)
            {
                out.push(ResolvedOperator {
                    kind: operator.kind,
                    name: operator.name,
                    span: origin.unwrap_or(operator.name_span),
                });
            }
            for arg in args {
                collect_operators(model, *arg, origin, visiting, out);
            }
        }
        ExprKind::Number | ExprKind::String | ExprKind::Error => {}
    }
}

fn equation_refs(model: &Model, equation: &crate::model::Equation) -> Vec<ResolvedRef> {
    let mut out = Vec::new();
    if let Some(lhs) = equation.lhs_expr {
        out.extend(resolved_refs(model, lhs));
    }
    if let Some(rhs) = equation.rhs_expr {
        out.extend(resolved_refs(model, rhs));
    }
    out
}

fn equation_operators(model: &Model, equation: &crate::model::Equation) -> Vec<ResolvedOperator> {
    let mut out = Vec::new();
    if let Some(lhs) = equation.lhs_expr {
        out.extend(resolved_operators(model, lhs));
    }
    if let Some(rhs) = equation.rhs_expr {
        out.extend(resolved_operators(model, rhs));
    }
    out
}

/// Written clashes, including direct PAC operator and fixed generated-name
/// cases. Remaining S012/S014 rewrites stay silent without a source mapping.
pub fn check_transform(model: &Model) -> Vec<Diagnostic> {
    let var_models = model_names(model, SemiStructuralKind::VarModel);
    let trend_models = model_names(model, SemiStructuralKind::TrendComponentModel);
    let expectation_models = model_names(model, SemiStructuralKind::VarExpectationModel);
    let mut out = Vec::new();

    // ModFile::transformPass first asks for every selected tag, before checking
    // var(log). The selected tags are collected as a set there.
    let mut selected_tags = BTreeMap::<&str, Span>::new();
    for command in &model.semi_structural_commands {
        if matches!(
            command.kind,
            SemiStructuralKind::VarModel | SemiStructuralKind::TrendComponentModel
        ) {
            if let Some(tags) = tags_option(command, "eqtags") {
                for (tag, span) in tags {
                    selected_tags.entry(tag.as_str()).or_insert(*span);
                }
            }
        }
    }
    for (tag, span) in &selected_tags {
        if tagged_equation(model, tag).is_none() {
            out.push(error(
                *span,
                "E433",
                format!("looking for equation tag {tag} failed."),
            ));
            return out;
        }
    }

    let mut scanned = HashSet::new();
    for tag in selected_tags.keys() {
        if let Some(equation) = tagged_equation(model, tag) {
            scanned.insert(equation.span.start);
        }
    }
    for equation in model.equations.iter().filter(|equation| !equation.is_local) {
        if equation_operators(model, equation)
            .iter()
            .any(|operator| operator.kind == NamedModelOperatorKind::PacExpectation)
        {
            scanned.insert(equation.span.start);
        }
    }
    let log_names: Vec<Name> = model
        .endogenous
        .iter()
        .filter(|decl| decl.log_transform)
        .map(|decl| decl.name)
        .filter(|name| {
            model.equations.iter().any(|equation| {
                scanned.contains(&equation.span.start)
                    && equation_refs(model, equation)
                        .iter()
                        .any(|reference| reference.name == *name)
            })
        })
        .collect();
    if !log_names.is_empty() {
        let span = model
            .equations
            .iter()
            .filter(|equation| scanned.contains(&equation.span.start))
            .flat_map(|equation| equation_refs(model, equation))
            .find(|reference| log_names.contains(&reference.name))
            .map_or(Span { start: 0, end: 1 }, |reference| reference.span);
        let names = log_names
            .iter()
            .map(|name| model.name(*name))
            .collect::<Vec<_>>()
            .join(" ");
        out.push(error(
            span,
            "E432",
            format!("the following variables are declared with var(log) and therefore cannot appear in a VAR/TCM/PAC equation: {names} "),
        ));
        return out;
    }

    for command in &model.semi_structural_commands {
        if command.kind != SemiStructuralKind::TrendComponentModel {
            continue;
        }
        if let Some(tags) = tags_option(command, "targets") {
            for (tag, span) in tags {
                if tagged_equation(model, tag).is_none() {
                    out.push(error(
                        *span,
                        "E433",
                        format!("no equation is named '{tag}'"),
                    ));
                    return out;
                }
            }
        }
    }

    for command in &model.semi_structural_commands {
        if !matches!(
            command.kind,
            SemiStructuralKind::VarModel | SemiStructuralKind::TrendComponentModel
        ) {
            continue;
        }
        if let Some(diag) = check_selected_shape(model, command) {
            out.push(diag);
            return out;
        }
    }

    for command in &model.semi_structural_commands {
        if command.kind != SemiStructuralKind::VarExpectationModel {
            continue;
        }
        let Some((auxiliary, span)) = symbol_option(command, "auxiliary_model_name") else {
            continue;
        };
        if !var_models.contains(&auxiliary) && !trend_models.contains(&auxiliary) {
            let name = symbol_option(command, "model_name")
                .map(|(name, _)| model.name(name))
                .unwrap_or("");
            out.push(error(
                span,
                "E435",
                format!(
                    "var_expectation_model {name} refers to nonexistent auxiliary model {}",
                    model.name(auxiliary)
                ),
            ));
            return out;
        }
        if let Some(reason) = var_expectation_expression_reason(model, command) {
            let name = symbol_option(command, "model_name")
                .map(|(name, _)| model.name(name))
                .unwrap_or("");
            let span = expression_option(command, "expression")
                .map(|expression| expression.span)
                .or_else(|| symbol_option(command, "variable").map(|(_, span)| span))
                .unwrap_or(command.span);
            out.push(error(
                span,
                "E436",
                format!("expression in var_expectation_model {name} is not of the expected form: {reason}"),
            ));
            return out;
        }
    }
    for equation in model.equations.iter().filter(|equation| !equation.is_local) {
        for operator in equation_operators(model, equation) {
            if operator.kind == NamedModelOperatorKind::VarExpectation
                && !expectation_models.contains(&operator.name)
            {
                out.push(error(
                    operator.span,
                    "E447",
                    format!(
                        "unknown model '{}' used in var_expectation expression",
                        model.name(operator.name)
                    ),
                ));
                return out;
            }
        }
    }

    let pac_models = model_names(model, SemiStructuralKind::PacModel);
    // PacModelTable stores models in a sorted set. Transform runs the whole
    // sequence below for one model before moving to the next model name.
    let mut pac_commands: Vec<_> = model
        .semi_structural_commands
        .iter()
        .filter(|command| command.kind == SemiStructuralKind::PacModel)
        .filter_map(|command| {
            symbol_option(command, "model_name").map(|(name, span)| (command, name, span))
        })
        .collect();
    pac_commands.sort_by(|a, b| model.name(a.1).cmp(model.name(b.1)));
    let mut generated_vars = HashSet::new();
    for (command, name, name_span) in pac_commands {
        if let Some(growth) = expression_option(command, "growth") {
            if let Some(span) = written_nonlinear_growth(model, growth) {
                out.push(error(
                    span,
                    "E448",
                    "PAC growth must be a linear combination of variables",
                ));
                return out;
            }
        }
        let target_rows = model
            .pac_target_info
            .iter()
            .filter(|block| block.name == name)
            .flat_map(|block| &block.rows);
        if let Some(target) = target_rows
            .clone()
            .filter_map(|row| match row {
                PacTargetInfoRow::Target(target) => Some(target),
                _ => None,
            })
            .last()
        {
            if written_target_product_without_lhs(model, target) {
                out.push(error(
                    target.span,
                    "E458",
                    format!(
                        "there is no equation whose LHS is equal to the 'target' of 'pac_target_info({})'",
                        model.name(name)
                    ),
                ));
                return out;
            }
        }
        if let Some((target_name, span)) = target_rows
            .clone()
            .filter_map(|row| match row {
                PacTargetInfoRow::AuxnameTargetNonstationary { name, span } => Some((*name, *span)),
                _ => None,
            })
            .last()
        {
            if let Some(diag) = generated_pac_variable_clash(
                model,
                model.name(target_name),
                span,
                &mut generated_vars,
                true,
            ) {
                out.push(diag);
                return out;
            }
        }
        if let Some((auxiliary, span)) = symbol_option(command, "auxiliary_model_name") {
            if !var_models.contains(&auxiliary) && !trend_models.contains(&auxiliary) {
                out.push(error(
                    span,
                    "E446",
                    "aux_model_name not recognized as VAR model or Trend Component model",
                ));
                return out;
            }
        }
        let uses: Vec<_> = model
            .equations
            .iter()
            .filter(|equation| !equation.is_local)
            .filter_map(|equation| {
                equation_operators(model, equation)
                    .into_iter()
                    .find(|operator| {
                        operator.kind == NamedModelOperatorKind::PacExpectation
                            && operator.name == name
                    })
            })
            .collect();
        if uses.is_empty() {
            out.push(error(
                name_span,
                "E449",
                format!(
                    "the model does not contain the 'pac_expectation({})' operator.",
                    model.name(name)
                ),
            ));
            return out;
        }
        if uses.len() > 1 {
            out.push(error(
                uses[1].span,
                "E450",
                format!(
                    "It is not possible to use 'pac_expectation({})' in several equations.",
                    model.name(name)
                ),
            ));
            return out;
        }
        if let Some(diag) =
            pac_generated_name_clash(model, command, name, &var_models, &mut generated_vars)
        {
            out.push(diag);
            return out;
        }
    }
    for (index, equation) in model
        .equations
        .iter()
        .filter(|equation| !equation.is_local)
        .enumerate()
    {
        for operator in equation_operators(model, equation) {
            if operator.kind == NamedModelOperatorKind::PacExpectation
                && !pac_models.contains(&operator.name)
            {
                out.push(error(
                    operator.span,
                    "E451",
                    format!(
                        "in equation {}, the pac_expectation operator references an unknown pac_model",
                        equation_label(model, index)
                    ),
                ));
                return out;
            }
        }
    }
    for (index, equation) in model
        .equations
        .iter()
        .filter(|equation| !equation.is_local)
        .enumerate()
    {
        for operator in equation_operators(model, equation) {
            if operator.kind == NamedModelOperatorKind::PacTargetNonstationary
                && (!pac_models.contains(&operator.name)
                    || !model
                        .pac_target_info
                        .iter()
                        .any(|block| block.name == operator.name))
            {
                out.push(error(
                    operator.span,
                    "E452",
                    format!("in equation {}, the pac_target_nonstationary operator does not match a corresponding 'pac_target_info' block", equation_label(model, index)),
                ));
                return out;
            }
        }
    }
    out
}

fn equation_label(model: &Model, target_index: usize) -> String {
    let equations: Vec<_> = model
        .equations
        .iter()
        .filter(|equation| !equation.is_local)
        .collect();
    let mut used: HashSet<String> = equations
        .iter()
        .filter_map(|equation| equation.tag_map.get("name"))
        .filter(|name| !name.is_empty())
        .cloned()
        .collect();
    for (index, equation) in equations.into_iter().enumerate() {
        if let Some(explicit) = equation.tag_map.get("name").filter(|name| !name.is_empty()) {
            if index == target_index {
                return explicit.clone();
            }
            continue;
        }
        let lhs_name = equation.lhs_expr.and_then(|lhs| {
            if let ExprKind::Ident { name, .. } = &model.exprs.get(lhs).kind {
                is_endogenous(model, *name).then(|| model.name(*name).to_string())
            } else {
                None
            }
        });
        let label = lhs_name
            .filter(|name| !used.contains(name))
            .unwrap_or_else(|| (index + 1).to_string());
        used.insert(label.clone());
        if index == target_index {
            return label;
        }
    }
    (target_index + 1).to_string()
}

fn written_declaration(model: &Model, name: &str) -> Option<Span> {
    model
        .endogenous
        .iter()
        .chain(&model.exogenous)
        .chain(&model.deterministic_exogenous)
        .chain(&model.parameters)
        .chain(&model.model_local_variables)
        .find(|decl| model.name(decl.name) == name)
        .map(|decl| decl.span)
        .or_else(|| {
            model
                .trend_vars
                .iter()
                .find(|decl| model.name(decl.name) == name)
                .map(|decl| decl.span)
        })
}

fn generated_pac_variable_clash(
    model: &Model,
    name: &str,
    written_name_span: Span,
    generated_vars: &mut HashSet<String>,
    target_nonstationary: bool,
) -> Option<Diagnostic> {
    let (code, operator) = if target_nonstationary {
        ("E457", "pac_target_nonstationary")
    } else {
        ("E456", "pac_expectation")
    };
    if let Some(span) = written_declaration(model, name) {
        return Some(error(span, code, format!("the variable/parameter '{name}' conflicts with a variable that will be generated for a '{operator}' expression. Please rename it.")));
    }
    if !generated_vars.insert(name.to_string()) {
        return Some(error(written_name_span, code, format!("the variable/parameter '{name}' conflicts with a variable that will be generated for a '{operator}' expression. Please rename it.")));
    }
    None
}

fn pac_generated_name_clash(
    model: &Model,
    command: &SemiStructuralCommand,
    name: Name,
    var_models: &HashSet<Name>,
    generated_vars: &mut HashSet<String>,
) -> Option<Diagnostic> {
    let pac = model.name(name);
    let auxiliary = symbol_option(command, "auxiliary_model_name").map(|(name, _)| name);
    let has_var = auxiliary.is_some_and(|name| var_models.contains(&name));
    let target_infos = model
        .pac_target_info
        .iter()
        .filter(|block| block.name == name);

    if option(command, "growth").is_some() {
        let generated = format!("{pac}_pac_growth_neutrality_correction");
        if let Some(span) = written_declaration(model, &generated) {
            return Some(error(span, "E453", format!("The variable/parameter '{generated}' conflicts with the auxiliary parameter that will be generated for the growth neutrality correction of the '{pac}' PAC model. Please rename that parameter.")));
        }
    }
    if target_infos.clone().next().is_none() {
        if auxiliary.is_none() {
            let aux = symbol_option(command, "auxname");
            let aux_name = aux
                .map(|(name, _)| model.name(name).to_string())
                .unwrap_or_else(|| format!("mce_Z1_{pac}"));
            if let Some(diag) = generated_pac_variable_clash(
                model,
                &aux_name,
                aux.map_or(command.span, |(_, span)| span),
                generated_vars,
                false,
            ) {
                return Some(diag);
            }
            let generated = format!("mce_alpha_{pac}_1");
            if let Some(span) = written_declaration(model, &generated) {
                return Some(error(span, "E454", format!("The variable/parameter '{generated}' conflicts with a parameter that will be generated for the '{pac}' PAC model. Please rename it.")));
            }
        } else {
            if has_var {
                let generated = format!("h_{pac}_constant");
                if let Some(span) = written_declaration(model, &generated) {
                    return Some(error(span, "E455", format!("the variable/parameter '{generated}' conflicts with some auxiliary parameter that will be generated for the '{pac}' PAC model. Please rename that parameter.")));
                }
            }
            let aux = symbol_option(command, "auxname");
            let aux_name = aux
                .map(|(name, _)| model.name(name).to_string())
                .unwrap_or_else(|| format!("pac_expectation_{pac}"));
            if let Some(diag) = generated_pac_variable_clash(
                model,
                &aux_name,
                aux.map_or(command.span, |(_, span)| span),
                generated_vars,
                false,
            ) {
                return Some(diag);
            }
        }
        return None;
    }

    if auxiliary.is_none() {
        let generated = format!("mce_alpha_{pac}_1");
        if let Some(span) = written_declaration(model, &generated) {
            return Some(error(span, "E454", format!("The variable/parameter '{generated}' conflicts with a parameter that will be generated for the '{pac}' PAC model. Please rename it.")));
        }
    }
    let mut component_idx = 0;
    for block in target_infos {
        for row in &block.rows {
            if let PacTargetInfoRow::Component(component) = row {
                component_idx += 1;
                let name_component = format!("{pac}_component{component_idx}");
                if has_var {
                    let generated = format!("h_{name_component}_constant");
                    if let Some(span) = written_declaration(model, &generated) {
                        return Some(error(span, "E455", format!("the variable/parameter '{generated}' conflicts with some auxiliary parameter that will be generated for the '{pac}' PAC model. Please rename that parameter.")));
                    }
                }
                if let Some((aux_name, span)) = component.rows.iter().rev().find_map(|field| {
                    if let PacTargetComponentRow::Auxname { name, span } = field {
                        Some((model.name(*name), *span))
                    } else {
                        None
                    }
                }) {
                    if let Some(diag) =
                        generated_pac_variable_clash(model, aux_name, span, generated_vars, false)
                    {
                        return Some(diag);
                    }
                }
                if component
                    .rows
                    .iter()
                    .any(|row| matches!(row, PacTargetComponentRow::Growth(_)))
                {
                    let generated = format!("{name_component}_pac_growth_neutrality_correction");
                    if let Some(span) = written_declaration(model, &generated) {
                        return Some(error(span, "E455", format!("the variable/parameter '{generated}' conflicts with some auxiliary parameter that will be generated for the '{pac}' PAC model. Please rename that parameter.")));
                    }
                }
            }
        }
    }
    None
}

// A product of two directly written endogenous/exogenous names stays nonlinear
// through Dynare's unary and diff substitutions. Other growth forms need the
// rewritten expression before we can mirror their matcher refusal.
fn written_nonlinear_growth(model: &Model, growth: &WrittenExpression) -> Option<Span> {
    let id = growth.expr?;
    let ExprKind::Binary {
        op: BinOp::Mul,
        lhs,
        rhs,
    } = &model.exprs.get(id).kind
    else {
        return None;
    };
    let variable = |id| match &model.exprs.get(id).kind {
        ExprKind::Ident { name, .. } => is_endogenous(model, *name) || is_exogenous(model, *name),
        _ => false,
    };
    (variable(*lhs) && variable(*rhs)).then_some(growth.span)
}

// Only the direct X*X target is decided here. Dynare can add an equation for
// a unary or diff target during transformation, so those shapes stay silent.
fn written_target_product_without_lhs(model: &Model, target: &WrittenExpression) -> bool {
    let Some(id) = target.expr else { return false };
    let ExprKind::Binary {
        op: BinOp::Mul,
        lhs,
        rhs,
    } = &model.exprs.get(id).kind
    else {
        return false;
    };
    let (ExprKind::Ident { name: left, .. }, ExprKind::Ident { name: right, .. }) =
        (&model.exprs.get(*lhs).kind, &model.exprs.get(*rhs).kind)
    else {
        return false;
    };
    if left != right || !is_endogenous(model, *left) {
        return false;
    }
    !model.equations.iter().filter(|equation| !equation.is_local).any(|equation| {
        equation.lhs_expr.is_some_and(|lhs| {
            let ExprKind::Binary {
                op: BinOp::Mul,
                lhs,
                rhs,
            } = &model.exprs.get(lhs).kind else { return false };
            matches!((&model.exprs.get(*lhs).kind, &model.exprs.get(*rhs).kind),
                (ExprKind::Ident { name: a, .. }, ExprKind::Ident { name: b, .. }) if a == left && b == right)
        })
    })
}

fn is_endogenous(model: &Model, name: Name) -> bool {
    model.endogenous.iter().any(|decl| decl.name == name)
}

fn is_exogenous(model: &Model, name: Name) -> bool {
    model.exogenous.iter().any(|decl| decl.name == name)
}

fn check_selected_shape(model: &Model, command: &SemiStructuralCommand) -> Option<Diagnostic> {
    let var = command.kind == SemiStructuralKind::VarModel;
    let family = if var {
        "VAR model"
    } else {
        "trend component model"
    };
    let rows = selected_equations(model, command);
    for (equation, tag, _) in &rows {
        let lhs = equation.lhs_expr?;
        let lhs_refs = resolved_refs(model, lhs);
        let endos: HashSet<_> = lhs_refs
            .iter()
            .filter(|r| is_endogenous(model, r.name))
            .map(|r| (r.name, r.timing))
            .collect();
        let other = lhs_refs.iter().any(|r| {
            is_exogenous(model, r.name) || is_parameter_before(model, r.name, equation.span.start)
        });
        if endos.len() != 1 || other {
            let text = if var {
                "A VAR may only have one endogenous variable on the LHS. "
            } else {
                "A trend component model may only have one endogenous variable on the LHS. "
            };
            return Some(error(
                equation.span,
                "E434",
                format!("in Equation {tag}. {text}"),
            ));
        }
        if endos.iter().next().is_some_and(|(_, timing)| *timing != 0) {
            let text = if var {
                "The variable on the LHS of a VAR may not appear with a lead or a lag. "
            } else {
                "The variable on the LHS of a trend component model may not appear with a lead or a lag. "
            };
            return Some(error(
                equation.span,
                "E434",
                format!("in Equation {tag}. {text}"),
            ));
        }
        if matches!(model.exprs.get(lhs).kind, ExprKind::Binary { .. }) {
            return Some(error(
                equation.span,
                "E434",
                "you can only have variables or unary ops on LHS of VAR",
            ));
        }
    }
    // Their RHS timing checks read the original equations after the LHS table
    // is filled. `structural` admits contemporaneous endogenous variables in a VAR.
    let structural = var && option(command, "structural").is_some();
    for (equation, tag, _) in &rows {
        let Some(rhs) = equation.rhs_expr else {
            continue;
        };
        for reference in resolved_refs(model, rhs) {
            if is_endogenous(model, reference.name) {
                let text = if var {
                    if reference.timing > 0 {
                        Some("A VAR model may not have leaded endogenous variables on the RHS. ")
                    } else if reference.timing == 0 && !structural {
                        Some("A non-structural VAR model may not have contemporaneous endogenous variables on the RHS. ")
                    } else {
                        None
                    }
                } else if reference.timing >= 0 {
                    Some("A trend component model may not have leaded or contemporaneous endogenous variables on the RHS. ")
                } else {
                    None
                };
                if let Some(text) = text {
                    return Some(error(
                        reference.span,
                        "E434",
                        format!("in Equation {tag}. {text}"),
                    ));
                }
            } else if is_exogenous(model, reference.name) && reference.timing != 0 {
                return Some(error(
                    reference.span,
                    "E434",
                    format!("in Equation {tag}. A {family} may not have lagged or leaded exogenous variables on the RHS. "),
                ));
            }
        }
    }
    let mut lhs = HashSet::new();
    for (equation, _, _) in &rows {
        let Some(id) = equation.lhs_expr else {
            continue;
        };
        let key = lhs_identity(model, id);
        if !lhs.insert(key) {
            let text = if var {
                "The LHS variables of the VAR model are not unique"
            } else {
                "The LHS variables of the trend component model are not unique"
            };
            return Some(error(equation.span, "E434", text));
        }
    }
    None
}

fn lhs_identity(model: &Model, id: ExprId) -> String {
    let node = model.exprs.get(id);
    match &node.kind {
        ExprKind::Unary { arg, .. } => lhs_identity(model, *arg),
        ExprKind::Ident { name, .. } => format!("variable:{}", model.name(*name)),
        // A transformed diff/log LHS denotes an auxiliary variable. Preserve
        // the call, while stripping an outer plus/minus like Dynare does.
        _ => model.source[node.span.start as usize..node.span.end as usize]
            .chars()
            .filter(|ch| !ch.is_whitespace())
            .collect(),
    }
}

fn var_expectation_expression_reason(
    model: &Model,
    command: &SemiStructuralCommand,
) -> Option<String> {
    if let Some((name, _)) = symbol_option(command, "variable") {
        if is_parameter_before(model, name, command.span.start) {
            return Some("No variable in this expression".to_string());
        }
        if model
            .deterministic_exogenous
            .iter()
            .any(|declaration| declaration.name == name)
        {
            return Some(format!("Symbol {} not allowed here", model.name(name)));
        }
        if is_exogenous(model, name) {
            return Some("Variable is not an endogenous".to_string());
        }
        return None;
    }
    let expression = expression_option(command, "expression")?;
    let id = expression.expr?;
    // A single deterministic exogenous is already the written expression.
    // Its matcher refusal does not depend on unary/diff substitution.
    if let ExprKind::Ident { name, .. } = &model.exprs.get(id).kind {
        if model
            .deterministic_exogenous
            .iter()
            .any(|declaration| declaration.name == *name)
        {
            return Some(format!("Symbol {} not allowed here", model.name(*name)));
        }
    }
    let mut terms = Vec::new();
    decompose_additive(model, id, &mut terms);
    for term in terms {
        let mut factor = LinearFactor::default();
        match collect_linear_factors(model, term, false, &mut factor) {
            Ok(()) => {}
            Err(Some(reason)) => return Some(reason.to_string()),
            Err(None) => return None, // unary rewrite needed before deciding
        }
        let Some((name, lag)) = factor.variable else {
            return Some("No variable in this expression".to_string());
        };
        if lag != 0 {
            return Some("lead/lags are not allowed".to_string());
        }
        if !is_endogenous(model, name) {
            return Some("Variable is not an endogenous".to_string());
        }
    }
    None
}

fn decompose_additive(model: &Model, id: ExprId, terms: &mut Vec<ExprId>) {
    match &model.exprs.get(id).kind {
        ExprKind::Binary {
            op: BinOp::Add | BinOp::Sub,
            lhs,
            rhs,
        } => {
            decompose_additive(model, *lhs, terms);
            decompose_additive(model, *rhs, terms);
        }
        ExprKind::Unary { op: UnOp::Neg, arg } => decompose_additive(model, *arg, terms),
        _ => terms.push(id),
    }
}

#[derive(Default)]
struct LinearFactor {
    variable: Option<(Name, i32)>,
    parameter: bool,
}

fn collect_linear_factors(
    model: &Model,
    id: ExprId,
    denominator: bool,
    factor: &mut LinearFactor,
) -> Result<(), Option<&'static str>> {
    match &model.exprs.get(id).kind {
        ExprKind::Number => Ok(()),
        ExprKind::Ident { name, timing, .. } => {
            if denominator {
                return Err(Some("A variable or parameter cannot appear at denominator"));
            }
            if is_parameter_before(model, *name, model.exprs.get(id).span.start) {
                if factor.parameter {
                    return Err(Some("More than one parameter in this expression"));
                }
                factor.parameter = true;
            } else if model
                .deterministic_exogenous
                .iter()
                .any(|declaration| declaration.name == *name)
            {
                return Err(None);
            } else if is_endogenous(model, *name) || is_exogenous(model, *name) {
                if factor.variable.is_some() {
                    return Err(Some("More than one variable in this expression"));
                }
                factor.variable = Some((*name, *timing));
            } else {
                return Err(None);
            }
            Ok(())
        }
        ExprKind::Unary { op: UnOp::Neg, arg } => {
            collect_linear_factors(model, *arg, denominator, factor)
        }
        ExprKind::Binary {
            op: BinOp::Mul,
            lhs,
            rhs,
        } => {
            collect_linear_factors(model, *lhs, denominator, factor)?;
            collect_linear_factors(model, *rhs, denominator, factor)
        }
        ExprKind::Binary {
            op: BinOp::Div,
            lhs,
            rhs,
        } => {
            collect_linear_factors(model, *lhs, denominator, factor)?;
            collect_linear_factors(model, *rhs, !denominator, factor)
        }
        ExprKind::Binary { .. } => Err(Some("Operator not allowed in this expression")),
        ExprKind::Call { .. } => Err(None),
        _ => Err(Some(
            "Expression not allowed in linear combination of variables",
        )),
    }
}
