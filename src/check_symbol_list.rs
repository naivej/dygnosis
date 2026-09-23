//! Trailing / `osr_params` symbol lists: undeclared, wrong type, `stoch_simul` duplicates.

use std::collections::HashSet;

use crate::diagnostic::{Diagnostic, Severity};
use crate::expr::ExprKind;
use crate::intern::Name;
use crate::model::{Equation, Model};

/// The types one command's list accepts. 7.1 builds its aux-prefix regex from
/// these types, so the set decides two things at once: what a name must be, and
/// which undeclared spellings are passed over as possible auxiliary names.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Allowed {
    /// `stoch_simul`, `estimation`, `calib_smoother`, `ramsey_policy`,
    /// `discretionary_policy`, `osr`, `ms_irf`, `plot_conditional_forecast`,
    /// `forecast`, and the four decomposition commands that share the set.
    Endogenous,
    /// `rplot`, `dynasave`, `dynatype`: the endogenous plus plain `varexo`.
    EndogenousExogenous,
    /// `plot_shock_decomposition`: the endogenous plus the names an `epilogue`
    /// block writes.
    EndogenousEpilogue,
    /// `osr_params`.
    Parameter,
}

impl Allowed {
    /// Whether 7.1's regex carries the `AUX_ENDO_` / `LOG_` arms. Their
    /// `regex_str` appends those two only when `endogenous` is among the types.
    fn allows_endogenous(self) -> bool {
        self != Self::Parameter
    }
}

pub fn check_symbol_list(model: &Model) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    let declared = declared_names(model);
    let removed: HashSet<Name> = model.var_removed.iter().map(|row| row.name).collect();
    let endogenous: HashSet<Name> = model
        .endogenous
        .iter()
        .filter(|d| !removed.contains(&d.name))
        .map(|d| d.name)
        .collect();
    let exo_det: HashSet<Name> = model
        .deterministic_exogenous
        .iter()
        .map(|d| d.name)
        .collect();
    // `Model::exogenous` holds every `varexo_det` name too, and 7.1's
    // `exogenousDet` is a type of its own: `rplot ed;` is refused. Plain
    // `varexo` is therefore the exogenous minus the deterministic ones.
    let mut endogenous_exogenous = endogenous.clone();
    endogenous_exogenous.extend(
        model
            .exogenous
            .iter()
            .filter(|d| !exo_det.contains(&d.name) && !removed.contains(&d.name))
            .map(|d| d.name),
    );
    let mut endogenous_epilogue = endogenous.clone();
    endogenous_epilogue.extend(model.epilogue.iter().map(|a| a.name));
    let parameters: HashSet<Name> = model
        .parameters
        .iter()
        .filter(|d| !removed.contains(&d.name))
        .map(|d| d.name)
        .collect();
    let mut seen_stoch = HashSet::new();
    // 7.1's aux hit returns from `checkPass` outright, so the rest of that
    // statement's list is never read. Remembering only that one statement keeps
    // the later statements' lists in play.
    let mut stopped: Option<u32> = None;

    for sym in &model.command_symbols {
        if sym.command == "stoch_simul" && !seen_stoch.insert((sym.list_id, sym.name)) {
            let name = model.name(sym.name);
            out.push(Diagnostic::new(
                sym.span,
                Severity::Warning,
                "W202",
                format!(
                    "In stoch_simul: {name} found more than once in symbol list. Removing all but first occurrence."
                ),
            ));
            continue;
        }

        if stopped == Some(sym.list_id) {
            continue;
        }

        let (label, prefix, allowed) = row_for(&sym.command);
        let name = model.name(sym.name);
        if !declared.contains(&sym.name) {
            if aux_rewrite_prefix(name, allowed.allows_endogenous()) {
                stopped = Some(sym.list_id);
                continue;
            }
            out.push(Diagnostic::new(
                sym.span,
                Severity::Error,
                "E239",
                format!("{prefix}: Variable {name} was not declared."),
            ));
            continue;
        }
        let set = match allowed {
            Allowed::Endogenous => &endogenous,
            Allowed::EndogenousExogenous => &endogenous_exogenous,
            Allowed::EndogenousEpilogue => &endogenous_epilogue,
            Allowed::Parameter => &parameters,
        };
        if !set.contains(&sym.name) {
            out.push(Diagnostic::new(
                sym.span,
                Severity::Error,
                "E240",
                format!("{prefix}: Variable {name} is not one of {label}"),
            ));
        }
    }
    out
}

/// One command's list row: the label their sentence prints for the allowed
/// types, the command word the sentence itself carries, and that set.
///
/// The label is data, not a branch: four labels sit under **E240**, and one
/// command's command word differs from the word its own statement is spelled
/// with (`osr_params` prints `osr: `).
fn row_for(command: &str) -> (&'static str, &str, Allowed) {
    let (allowed, label) = match command {
        "rplot" | "dynasave" | "dynatype" => {
            (Allowed::EndogenousExogenous, "{endogenous, exogenous}")
        }
        "plot_shock_decomposition" => (Allowed::EndogenousEpilogue, "{endogenous, epilogue}"),
        "osr_params" => (Allowed::Parameter, "{parameter}"),
        _ => (Allowed::Endogenous, "{endogenous}"),
    };
    let prefix = if command == "osr_params" {
        "osr"
    } else {
        command
    };
    (label, prefix, allowed)
}

/// Every name kind their symbol table holds that a list can name, minus the one
/// kind a list cannot: a mod-file local.
///
/// A top-level `# mloc = 3;` leaves 7.1's table without the name — `forecast
/// mloc;` prints `was not declared.` on both sides — so `model.mod_file_locals`
/// stays out. The kinds below are in their table, so a command reports their
/// type sentence for them rather than the undeclared one. A `#` definition
/// **inside** the model block is one of those kinds: their `modelLocalVariable`
/// is a type of its own, and `model.equations` carries it as a local row.
fn declared_names(model: &Model) -> HashSet<Name> {
    model
        .endogenous
        .iter()
        .chain(&model.exogenous)
        .chain(&model.deterministic_exogenous)
        .chain(&model.parameters)
        .chain(&model.predetermined)
        .chain(&model.excluded_endogenous)
        .chain(&model.model_local_variables)
        .map(|d| d.name)
        .chain(model.var_removed.iter().map(|row| row.name))
        .chain(model.epilogue.iter().map(|a| a.name))
        .chain(model.trend_vars.iter().map(|t| t.name))
        .chain(model.external_function_names.iter().copied())
        .chain(
            model
                .equations
                .iter()
                .filter_map(|eq| model_local_name(model, eq)),
        )
        .chain(model.steady_state_equations.iter().filter_map(|eq| {
            match &model.exprs.get(eq.lhs_expr?).kind {
                ExprKind::Ident { name, .. } => Some(*name),
                _ => None,
            }
        }))
        .collect()
}

/// The name one `#` definition row binds: the identifier its left-hand side
/// carries.
fn model_local_name(model: &Model, eq: &Equation) -> Option<Name> {
    if !eq.is_local {
        return None;
    }
    match &model.exprs.get(eq.lhs_expr?).kind {
        ExprKind::Ident { name, .. } => Some(*name),
        _ => None,
    }
}

/// Whether an undeclared spelling is one their regex passes over. The two
/// always-present arms are theirs; the other two come with an endogenous set.
fn aux_rewrite_prefix(name: &str, allows_endogenous: bool) -> bool {
    name.starts_with("AUX_EXPECT_")
        || name.starts_with("MULT_")
        || (allows_endogenous && (name.starts_with("AUX_ENDO_") || name.starts_with("LOG_")))
}
