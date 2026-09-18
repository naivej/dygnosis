//! Trailing / `osr_params` symbol lists: undeclared, wrong type, `stoch_simul` duplicates.

use std::collections::HashSet;

use crate::diagnostic::{Diagnostic, Severity};
use crate::intern::Name;
use crate::model::{CommandSymbol, Model};

pub fn check_symbol_list(model: &Model) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    let declared = declared_names(model);
    let endogenous: HashSet<Name> = model.endogenous.iter().map(|d| d.name).collect();
    let parameters: HashSet<Name> = model.parameters.iter().map(|d| d.name).collect();
    let mut seen_stoch = HashSet::new();

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

        let (cmd, allowed_label, allowed) = allowed_for(sym, model, &endogenous, &parameters);
        let name = model.name(sym.name);
        if !declared.contains(&sym.name) {
            if aux_rewrite_prefix(name) {
                continue;
            }
            out.push(Diagnostic::new(
                sym.span,
                Severity::Error,
                "E239",
                format!("{cmd}: Variable {name} was not declared."),
            ));
            continue;
        }
        if !allowed.contains(&sym.name) {
            out.push(Diagnostic::new(
                sym.span,
                Severity::Error,
                "E240",
                format!("{cmd}: Variable {name} is not one of {allowed_label}"),
            ));
        }
    }
    out
}

fn allowed_for<'a>(
    sym: &'a CommandSymbol,
    model: &Model,
    endogenous: &'a HashSet<Name>,
    parameters: &'a HashSet<Name>,
) -> (&'a str, &'static str, &'a HashSet<Name>) {
    if sym.command == "osr" && model.osr_params.contains(&sym.name) {
        return ("osr", "{parameter}", parameters);
    }
    (sym.command.as_str(), "{endogenous}", endogenous)
}

fn declared_names(model: &Model) -> HashSet<Name> {
    model
        .endogenous
        .iter()
        .chain(&model.exogenous)
        .chain(&model.parameters)
        .chain(&model.predetermined)
        .map(|d| d.name)
        .collect()
}

fn aux_rewrite_prefix(name: &str) -> bool {
    name.starts_with("AUX_EXPECT_")
        || name.starts_with("MULT_")
        || name.starts_with("AUX_ENDO_")
        || name.starts_with("LOG_")
}
