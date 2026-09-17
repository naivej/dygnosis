//! Diagnostic code documentation.
//!
//! Mechanical port of `python_dynare_lsp/explain.py` `_ENTRIES` for the 70
//! codes shipped through 0.5.0, then kind `emit` / `skip` / `added`.
//! 163 keys = 70 emit + 27 added + 66 skip. Catalog **0.5.1** D-clash and
//! D-check Errors are emit.
//! `I050` and `W042` use the recorded surface rewrites in
//! `dev_logs/0.1/0.1.0/22-c-explain.md` (do not advertise Compute Steady State).

use std::collections::HashMap;
use std::sync::OnceLock;

/// How a documented code relates to what we emit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExplainKind {
    /// Official refuse or warn we emit.
    Emit,
    /// Catalog skip; listed and never emitted.
    Skip,
    /// Extra diagnostic Dynare never reports.
    Added,
}

impl ExplainKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Emit => "emit",
            Self::Skip => "skip",
            Self::Added => "added",
        }
    }
}

/// Title, markdown body, and emit/skip/added kind for one diagnostic code.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExplainEntry {
    pub title: &'static str,
    pub body: &'static str,
    pub kind: ExplainKind,
}

// 163 keys: 70 emit + 27 added + 66 skip.
static ENTRIES: &[(&str, ExplainEntry)] = &[
    ("E001", ExplainEntry {
        title: "Parse error",
        body: "The Dynare parser could not interpret the source. The diagnostic range points at the offending token or the nearest recoverable position. Dynare refuses with a generic bison `ERROR` at a location.\n\n**Warrant**\n\nThe editor names the missing construct and points at a usable range; Dynare's bison location is often the next token.\n\n**Common causes**\n\n- Missing semicolon at the end of a declaration or equation\n- Unbalanced parentheses, braces, or block keywords\n- Malformed time subscript such as `y(1)` where `y(+1)` was meant\n- A reserved keyword used as an identifier\n\n**Fix**\n\nInspect the line cited and the line immediately preceding it. Dynare's preprocessor frequently flags the *next* line after a missing semicolon.",
        kind: ExplainKind::Emit,
    }),
    ("W013", ExplainEntry {
        title: "Equation count does not match endogenous variable count",
        body: "The number of equations inside the `model` block does not equal the number of endogenous variables declared in the `var` block.\n\nWhen ramsey_model, ramsey_policy, or discretionary_policy is present and instruments= lists N unique names, the expected gap is −N, not equality; a square file still warns.\n\n**Fix**\n\n- Add a missing equation, or remove a duplicate one\n- Declare the missing endogenous variable in `var`, or remove an   extra declaration\n- Check whether a commented-out equation was intended to be   active\n- For ramsey_model / ramsey_policy / discretionary_policy with N instruments, the intended gap is −N; do not add equations only to make the file square",
        kind: ExplainKind::Added,
    }),
    ("E020", ExplainEntry {
        title: "Undeclared identifier in model block",
        body: "An identifier appears in the `model` block but is not declared as a `var`, `varexo`, or `parameters` symbol. Dynare refuses: `Unknown symbol: alpph`.\n\n**Warrant**\n\nThe editor sentence names the undeclared identifier in the equation and may include a Did-you-mean suggestion; Dynare's string is the generic `Unknown symbol`.\n\n**Fix**\n\n- Add the identifier to the appropriate declaration block\n- Correct a typo (a close-match suggestion may appear)\n- If the symbol is a local helper, define it in the parameter   section before use",
        kind: ExplainKind::Emit,
    }),
    ("E023", ExplainEntry {
        title: "Predetermined variable not declared endogenous",
        body: "A name listed in `predetermined_variables` must also be declared as an endogenous variable in the `var` block. Dynare refuses: `Unknown symbol: e`.\n\n**Warrant**\n\nThe editor names the `predetermined_variables` role; Dynare's parse string is the generic `Unknown symbol`.\n\n**Fix**\n\n- Add the variable to the `var` declaration, or\n- Remove it from `predetermined_variables` if it is not actually   endogenous",
        kind: ExplainKind::Emit,
    }),
    ("E024", ExplainEntry {
        title: "Deterministic exogenous with a lead or lag",
        body: "A deterministic exogenous variable (``varexo_det``) is used with a lead or lag. Dynare refuses: `Exogenous deterministic variable tau cannot be given a lead or a lag.` Parameter leads/lags are accepted because Dynare treats them as fixed scalars.\n\n**Fix**\n\n- Remove the time subscript\n- If the dated quantity is state-dependent, model it as an endogenous variable instead",
        kind: ExplainKind::Emit,
    }),
    ("E025", ExplainEntry {
        title: "Invalid model-local (`#`) variable",
        body: "A model-local variable defined with `#` either reuses a declared `var`, `varexo`, or `parameters` name, or is used in an equation before its `#` definition. Dynare refuses: `… has wrong type or was already used on the right-hand side. You cannot use it on the left-hand side of a pound ('#') expression`.\n\n**Fix**\n\n- Rename the model-local helper so it does not clash with a declared symbol\n- Move the `#` definition above its first use\n- Or remove the declaration if the name was meant to be only a model-local helper",
        kind: ExplainKind::Emit,
    }),
    ("E026", ExplainEntry {
        title: "varexo_det with a perfect-foresight solver",
        body: "A ``varexo_det`` declaration cannot appear with ``simul``, ``perfect_foresight_solver``, or ``perfect_foresight_with_expectation_errors_solver``. Dynare refuses: `A .mod file cannot contain both one of {perfect_foresight_solver, simul, perfect_foresight_with_expectation_errors_solver} and varexo_det declaration (all exogenous variables are deterministic in this case)`.\n\n**Fix**\n\nRemove the ``varexo_det`` declaration (use ``varexo``), or drop the perfect-foresight solver.",
        kind: ExplainKind::Emit,
    }),
    ("E027", ExplainEntry {
        title: "varexo_det with Ramsey",
        body: "``ramsey_model`` and ``ramsey_policy`` cannot be used with deterministic exogenous variables. Dynare refuses: `ramsey_model and ramsey_policy are incompatible with deterministic exogenous variables`.\n\n**Fix**\n\nRemove the ``varexo_det`` declaration, or drop the Ramsey command.",
        kind: ExplainKind::Emit,
    }),
    ("E028", ExplainEntry {
        title: "varexo_det with identification",
        body: "The ``identification`` command cannot be used with deterministic exogenous variables. Dynare refuses: `identification is incompatible with deterministic exogenous variables`.\n\n**Fix**\n\nRemove the ``varexo_det`` declaration, or drop ``identification``.",
        kind: ExplainKind::Emit,
    }),
    ("E030", ExplainEntry {
        title: "Duplicate declaration across types or `#` twice",
        body: "The same identifier is declared in two different blocks (for example both `var` and `varexo`), or a model-local `#` name is defined twice. Dynare refuses: `Symbol y declared twice with different types!` and `Local model variable foo declared twice.`\n\n**Fix**\n\nRemove the extra declaration. If you intended two related but distinct symbols, rename one.",
        kind: ExplainKind::Emit,
    }),
    ("W031", ExplainEntry {
        title: "Symbol declared twice with the same type",
        body: "The same identifier is declared more than once in `var`, `varexo`, `varexo_det`, or `parameters`. Dynare accepts and warns `Symbol y declared twice.`\n\n**Fix**\n\nRemove the redundant declaration.",
        kind: ExplainKind::Emit,
    }),
    ("W054", ExplainEntry {
        title: "Duplicate equation",
        body: "Two equations inside the `model` block are textually identical. The diagnostic cites the line where the duplicate appears and the line where the first occurrence was found. A duplicate can also show up as the equation-count Warning `W013`.\n\n**Fix**\n\nRemove the duplicate.",
        kind: ExplainKind::Added,
    }),
    ("W055", ExplainEntry {
        title: "Contradictory equation (always false)",
        body: "An equation reduces to a tautological falsehood, for example `0 = 1`.\n\n**Fix**\n\nRemove the equation, or restore a variable reference that was accidentally simplified away.",
        kind: ExplainKind::Added,
    }),
    ("W056", ExplainEntry {
        title: "Duplicate parameter assignment",
        body: "The same parameter is assigned a value more than once in the parameter section. The last assignment wins.\n\n**Fix**\n\nRemove one of the assignments, or rename if two distinct parameters were intended.",
        kind: ExplainKind::Added,
    }),
    ("W057", ExplainEntry {
        title: "Stray equation outside model block",
        body: "A line that looks like a model equation appears outside the `model` ... `end;` block.\n\n**Fix**\n\nMove the equation inside the `model` block, or convert it to a parameter assignment if it belongs at the top level.",
        kind: ExplainKind::Added,
    }),
    ("W062", ExplainEntry {
        title: "Circular @#include detected",
        body: "Two or more files include each other in a loop. The message names the chain, for example `a.mod -> b.mod -> a.mod`.\n\n**Common causes**\n\n- A submodel was refactored and now includes its parent\n- Two helper files cross-include each other for shared\n  parameters or steady-state definitions\n- A copy-paste mistake duplicated the include in the wrong\n  direction\n\n**Fix**\n\nBreak the cycle by removing one `@#include` along the chain. If both files genuinely need a shared block, extract that block into a third file and have both parents include it.",
        kind: ExplainKind::Added,
    }),
    ("E061", ExplainEntry {
        title: "Could not open @#include target",
        body: "An `@#include` directive names a file that could not be opened. Dynare refuses: `Could not open F. The following directories were searched` (then the directories actually searched).\n\n**Common causes**\n\n- A typo in the filename\n- The included file lives in a directory that isn't on the search paths\n- The file was renamed or moved without updating the   directive\n\n**Fix**\n\nCorrect the filename, add the missing file, or extend the search paths so the directory containing the include is visible.",
        kind: ExplainKind::Emit,
    }),
    ("E062", ExplainEntry {
        title: "Unmatched macro block",
        body: "A Dynare macro `@#if` block has no matching `@#endif`, or a `@#for` block has no matching `@#endfor`. Dynare refuses with a generic bison syntax `ERROR`.\n\n**Warrant**\n\nThe editor names the unmatched opener or stray closer and points at that directive; Dynare's bison location does not.\n\n**Common causes**\n\n- A copy-paste deleted the closing directive\n- Mismatched closers — `@#endif` accidentally written for   a `@#for`, or vice versa\n- A nested block missing its inner closer\n\n**Fix**\n\nAdd the missing `@#endif` or `@#endfor` at the appropriate scope, or remove the stray closer. Each `@#if` needs its own `@#endif`; each `@#for` its own `@#endfor`.",
        kind: ExplainKind::Emit,
    }),
    ("E063", ExplainEntry {
        title: "Undefined macro interpolation",
        body: "An active line still contains an unresolved `@{NAME}` macro interpolation. Dynare refuses: `Unknown variable N`.\n\n**Fix**\n\nDefine the macro with `@#define NAME = value` before the line that uses it, correct the macro name, or remove the interpolation.",
        kind: ExplainKind::Emit,
    }),
    ("E064", ExplainEntry {
        title: "Macro error directive",
        body: "An active Dynare macro `@#error` directive was reached. Dynare refuses: `Macro-processing error` plus the user message from the directive.\n\n**Fix**\n\nRemove the `@#error` directive, or guard it behind a macro condition that is false for this model variant.",
        kind: ExplainKind::Emit,
    }),
    ("E065", ExplainEntry {
        title: "Invalid steady_state operand",
        body: "The `steady_state(...)` operator must not contain exogenous shocks. Dynare refuses: `Exogenous variables are not allowed in the context of the STEADY_STATE() operator.`\n\n**Fix**\n\nRemove the exogenous variable from `steady_state(...)`, replace it with the intended endogenous or parameter expression, or rewrite the equation so the shock enters outside the operator.",
        kind: ExplainKind::Emit,
    }),
    ("E999", ExplainEntry {
        title: "Additional errors truncated",
        body: "More diagnostics were produced than are shown at once. Fix the visible errors first; the next analysis pass will surface anything that was previously hidden.",
        kind: ExplainKind::Added,
    }),
    ("I050", ExplainEntry {
        title: "No initval or steady_state_model block",
        body: "The file declares variables and equations but does not include an `initval` or `steady_state_model` block. This check is presence only; this tool does not compute a numerical steady state. A sibling FILENAME_steadystate.m also counts as presence.\n\n**Fix**\n\nAdd an `initval` block with initial guesses, or a `steady_state_model` block with closed-form assignments. For a numerical solve, use Dynare (for example `steady;` in MATLAB/Octave).",
        kind: ExplainKind::Added,
    }),
    ("W010", ExplainEntry {
        title: "Parameter declared but never assigned",
        body: "A name appears in the `parameters` block but no assignment was found in the parameter section or in `steady_state_model`. At runtime, the parameter will be undefined and most computations will fail.\n\n**Fix**\n\nAssign a numerical value, or remove the declaration if the parameter is no longer used.",
        kind: ExplainKind::Added,
    }),
    ("W011", ExplainEntry {
        title: "Parameter assignment cannot be evaluated",
        body: "An assignment like `phi = 1/(1-beta)` could not be evaluated because one or more right-hand-side symbols are not yet defined. The parameter falls back to undefined.\n\n**Fix**\n\nReorder the parameter section so that dependencies appear before dependents.",
        kind: ExplainKind::Added,
    }),
    ("W012", ExplainEntry {
        title: "Undeclared helper variable in parameter section",
        body: "An identifier appears on the right-hand side of a parameter assignment but is not declared as a parameter or known helper variable.\n\n**Fix**\n\nAdd a declaration, or replace the helper with an explicit numeric value.",
        kind: ExplainKind::Added,
    }),
    ("W020", ExplainEntry {
        title: "Endogenous variable never referenced in model",
        body: "An endogenous variable is declared in `var` but does not appear in any equation. Either remove the declaration or add the missing equation that uses the variable.",
        kind: ExplainKind::Added,
    }),
    ("E021", ExplainEntry {
        title: "Exogenous variable never referenced in model",
        body: "A shock declared in `varexo` does not appear in any equation. Dynare refuses: `unused_exo not used in model block. To bypass this error, use the nostrict option. This may lead to crashes or unexpected behavior.`",
        kind: ExplainKind::Emit,
    }),
    ("W022", ExplainEntry {
        title: "Parameter declared but never referenced in model equations",
        body: "A parameter is declared and assigned but does not appear in any model equation. Dynare warns: `Parameter(s) unused_p not used in the model`.",
        kind: ExplainKind::Emit,
    }),
    ("W042", ExplainEntry {
        title: "Endogenous variable missing from steady_state_model",
        body: "The `steady_state_model` block does not assign a value for every endogenous variable. Dynare warns: `variable 'c' is not assigned a value`. Dynare will fall back to the `initval` value (or zero).\n\n**Fix**\n\nAdd the missing assignments. For a numerical solve, use Dynare (for example `steady;` in MATLAB/Octave).",
        kind: ExplainKind::Emit,
    }),
    ("E058", ExplainEntry {
        title: "Undeclared variable in initval",
        body: "An entry in the `initval` block refers to a name that is not declared as a variable. Dynare refuses: `Unknown symbol: undeclared_zzz`.\n\n**Warrant**\n\nThe editor names the undeclared `initval` / `endval` entry; Dynare's string is the generic `Unknown symbol`.\n\n**Fix**\n\nDeclare the variable, or remove the stray `initval` entry.",
        kind: ExplainKind::Emit,
    }),
    ("W051", ExplainEntry {
        title: "Exogenous variable set in initval",
        body: "Setting an exogenous variable in `initval` has no effect on the steady-state computation. Shocks are zero at the deterministic steady state by construction.",
        kind: ExplainKind::Added,
    }),
    ("W052", ExplainEntry {
        title: "Endogenous variable missing from initval",
        body: "The `initval` block does not provide an initial guess for every endogenous variable. The solver will start from zero for the missing entries, which may slow or prevent convergence on nonlinear models.",
        kind: ExplainKind::Added,
    }),
    ("E059", ExplainEntry {
        title: "Name in initval/endval is neither endogenous or exogenous",
        body: "An `initval` or `endval` entry names a symbol that is not endogenous or exogenous (for example a parameter). Dynare refuses: `… is neither endogenous or exogenous.`\n\n**Fix**\n\nAssign parameters before the model block, or inside `steady_state_model`. Use `initval` / `endval` only for endogenous or exogenous variables.",
        kind: ExplainKind::Emit,
    }),
    ("W060", ExplainEntry {
        title: "Exogenous variables declared but no shocks block",
        body: "One or more exogenous variables are declared in `varexo` but the file contains no `shocks` block specifying their variance-covariance structure. The model is then deterministic.\n\n**Fix**\n\nAdd a `shocks` block to define the shock processes, or remove the unused `varexo` declarations.",
        kind: ExplainKind::Added,
    }),
    ("W061", ExplainEntry {
        title: "Ambiguous include parent context",
        body: "The active include file is reachable from more than one parent model, so the editor cannot tell which parent's declarations apply.\n\n**Fix**\n\nOpen the intended parent `.mod` file.",
        kind: ExplainKind::Added,
    }),
    ("W070", ExplainEntry {
        title: "Parameter outside its conventional range",
        body: "A parameter assignment falls outside the theoretically admissible range for its standard interpretation. The conventional-range table is opinionated but conservative: it flags values that violate the *theoretical* admissible range under the parameter's conventional meaning, not values that simply look unusual.\n\n**Common causes**\n\n- Unit error: e.g. `beta = 99` when 0.99 was meant\n- Sign error on a quantity that must be non-negative   (variance, standard deviation, depreciation rate)\n- Gross-vs-net confusion on a rate parameter\n\n**Fix**\n\nCorrect the value, or — if the calibration is intentional — ignore the warning.",
        kind: ExplainKind::Added,
    }),
    ("E090", ExplainEntry {
        title: "Observed variable is not a declared endogenous variable",
        body: "A name listed in ``varobs`` is not a declared endogenous variable. Dynare refuses: `e is not endogenous.`\n\n**Warrant**\n\nThe editor names the ``varobs`` role and, when the name is already declared as something else, says so; Dynare's string is only `N is not endogenous.`\n\n**Fix**\n\nDeclare the variable in ``var``, or remove it from ``varobs`` if it was a typo or an exogenous/parameter name.",
        kind: ExplainKind::Emit,
    }),
    ("W091", ExplainEntry {
        title: "Duplicate observed variable",
        body: "A variable is listed more than once in ``varobs``. Each observed variable should appear exactly once.\n\n**Fix**\n\nRemove the duplicate entry.",
        kind: ExplainKind::Added,
    }),
    ("W092", ExplainEntry {
        title: "Stochastic singularity",
        body: "There are more observed variables (``varobs``) than shocks (structural shocks plus measurement errors). The likelihood is then stochastically singular and estimation cannot proceed: the model cannot generate enough independent variation to match the observed series.\n\n**Fix**\n\nAdd structural shocks, add measurement errors on the observed variables (an ``stderr`` on an observed variable), or reduce the number of observed variables so that observables ≤ shocks.",
        kind: ExplainKind::Added,
    }),
    ("E093", ExplainEntry {
        title: "estimated_params references an undeclared symbol",
        body: "An ``estimated_params`` entry names a symbol that is not declared with the expected role: a plain entry must name a parameter, an ``stderr`` entry must name a shock or observed variable, and a ``corr`` entry must name two declared shocks or variables. Dynare refuses: `Unknown symbol: not_a_param`.\n\n**Warrant**\n\nThe editor names the ``estimated_params`` role (parameter, stderr, or corr); Dynare's string is the generic `Unknown symbol`.\n\n**Fix**\n\nDeclare the symbol, or correct the name / entry type.",
        kind: ExplainKind::Emit,
    }),
    ("W094", ExplainEntry {
        title: "estimated_params bound or initial-value inconsistency",
        body: "An ``estimated_params`` entry has a lower bound that is not below its upper bound, or an initial value that lies outside the ``[lower, upper]`` interval.\n\n**Fix**\n\nOrder the bounds so that lower < upper and place the initial value inside them.",
        kind: ExplainKind::Added,
    }),
    ("E095", ExplainEntry {
        title: "observation_trends variable not in varobs",
        body: "A variable given a trend in ``observation_trends`` is not listed in ``varobs``. Dynare refuses: `variable y in observation_trends block is not an observed variable`.\n\n**Fix**\n\nAdd the variable to ``varobs`` or remove its trend specification.",
        kind: ExplainKind::Emit,
    }),
    ("E100", ExplainEntry {
        title: "planner_objective and optimal-policy commands go together",
        body: "``planner_objective`` must appear with ``ramsey_model``, ``ramsey_policy``, ``osr``, or ``discretionary_policy``, and those commands (except ``osr``) need ``planner_objective``. Dynare refuses either missing direction: `A planner_objective statement must be used with a ramsey_model, a ramsey_policy, osr, or a discretionary_policy statement and vice versa.`\n\n**Fix**\n\nAdd the missing ``planner_objective <expression>;``, or add a matching policy command.",
        kind: ExplainKind::Emit,
    }),
    ("E101", ExplainEntry {
        title: "Policy instrument is not a declared endogenous variable",
        body: "An ``instruments=(...)`` entry names a symbol that is not a declared endogenous variable. Dynare refuses: `Unknown symbol: not_endo`.\n\n**Warrant**\n\nThe editor names the policy instrument; Dynare's string is the generic `Unknown symbol`.\n\n**Fix**\n\nDeclare the instrument in ``var``, or correct the instrument name.",
        kind: ExplainKind::Emit,
    }),
    ("W102", ExplainEntry {
        title: "planner_discount is not a valid discount factor",
        body: "``planner_discount`` must be a discount factor in the interval (0, 1]. A value outside this range is almost certainly a mistake (for example, entering a discount rate instead of a factor).\n\n**Fix**\n\nSet ``planner_discount`` to a value such as 0.99.",
        kind: ExplainKind::Added,
    }),
    ("E103", ExplainEntry {
        title: "osr is missing osr_params or optim_weights",
        body: "Optimal simple rules (``osr``) need an ``osr_params`` statement (the parameters to optimise). Dynare refuses when it is missing: `The osr statement requires the osr_params statement.` Dynare also refuses when neither ``optim_weights`` nor ``planner_objective`` is present: `The osr statement requires either an optim_weights block or a planner_objective.`\n\n**Fix**\n\nAdd the missing ``osr_params`` statement, and either an ``optim_weights`` block or a ``planner_objective``.",
        kind: ExplainKind::Emit,
    }),
    ("E104", ExplainEntry {
        title: "More than one planner_objective with Ramsey",
        body: "With ``ramsey_model`` or ``ramsey_policy``, only one ``planner_objective`` statement is allowed. Dynare refuses: `there can only be one planner_objective statement`.\n\n**Fix**\n\nKeep a single ``planner_objective``.",
        kind: ExplainKind::Emit,
    }),
    ("W110", ExplainEntry {
        title: "Shock correlation outside [-1, 1]",
        body: "A ``corr`` entry in the shocks block sets a correlation whose magnitude exceeds one. A correlation coefficient must lie in [-1, 1], and the implied covariance matrix would not be positive semidefinite.\n\n**Fix**\n\nSet the correlation to a value in [-1, 1].",
        kind: ExplainKind::Added,
    }),
    ("E111", ExplainEntry {
        title: "Shock variance or correlation specified more than once",
        body: "A shock's variance / standard error, or a correlation pair, is specified more than once in the shocks block. Dynare refuses: `shocks: variance or stderr of shock on e declared twice` and `shocks: covariance or correlation shock on variable pair (e, u) declared twice`.\n\n**Fix**\n\nKeep a single specification per shock variance and per correlation pair.",
        kind: ExplainKind::Emit,
    }),
    ("E113", ExplainEntry {
        title: "shock_paths with shocks, mshocks, endval, or controlled paths",
        body: "A ``shock_paths`` block cannot appear with ``shocks``, ``mshocks``, ``endval``, or ``perfect_foresight_controlled_paths``. Dynare refuses: `the 'shock_paths' block cannot be used in conjunction with either 'shocks', 'mshocks', 'endval' or 'perfect_foresight_controlled_paths' blocks.`.\n\n**Fix**\n\nKeep ``shock_paths`` and drop the other block, or drop ``shock_paths``.",
        kind: ExplainKind::Emit,
    }),
    ("W112", ExplainEntry {
        title: "Negative shock variance",
        body: "A shocks-block ``var e = ...`` entry sets a variance that folds to a negative constant. A variance is a squared quantity and should be non-negative.\n\n(The ``stderr`` form is not flagged: a negative standard error is squared, so the variance is still positive.)\n\n**Fix**\n\nUse a non-negative value. Recall the ``var`` form sets the variance, i.e. the standard error *squared* (e.g. ``var e = 0.01^2;``).",
        kind: ExplainKind::Added,
    }),
    ("W120", ExplainEntry {
        title: "Stochastic command with no stochastic exogenous variable",
        body: "``stoch_simul`` / ``estimation`` drive the model with stochastic shocks, but the model declares no stochastic ``varexo``. ``varexo_det`` declarations are deterministic and do not count as stochastic shocks.\n\n**Fix**\n\nDeclare at least one stochastic exogenous variable (a dummy ``varexo`` plus a shocks-block entry is enough if the model is otherwise deterministic).",
        kind: ExplainKind::Added,
    }),
    ("W121", ExplainEntry {
        title: "Parameter used with a lead or lag",
        body: "A declared parameter appears with a time subscript such as ``beta(+1)`` or ``rho(-1)`` in the model block. Dynare warns: `The following parameter(s) are used with a lead or a lag: betta`.\n\n**Fix**\n\nRemove the time index, or declare the symbol with ``var`` / ``varexo`` if it really is a variable.",
        kind: ExplainKind::Emit,
    }),
    ("W122", ExplainEntry {
        title: "Deep parameter assigned a non-finite value",
        body: "A parameter that is used in the model equations is assigned a non-finite value (``NaN`` or ``Inf``) while a run command (``steady``, ``stoch_simul``, ``perfect_foresight_*``, ``estimation``, ...) is present.\n\n**Fix**\n\nAssign a finite numeric value before the run command.",
        kind: ExplainKind::Added,
    }),
    ("E130", ExplainEntry {
        title: "Variable used before assignment in steady_state_model",
        body: "The ``steady_state_model`` block is evaluated top to bottom as a sequence of assignments, so every variable on a right-hand side must already have been assigned above. Dynare refuses: `variable 'n' is undefined in the declaration of variable 'log_n'`.\n\n**Fix**\n\nReorder the assignments so each variable is computed before it is used.",
        kind: ExplainKind::Emit,
    }),
    ("E170", ExplainEntry {
        title: "Multiple occbin_constraints blocks",
        body: "The file has more than one ``occbin_constraints`` block. Dynare refuses: `Multiple 'occbin_constraints' blocks are not allowed`.\n\n**Fix**\n\nKeep a single ``occbin_constraints`` block.",
        kind: ExplainKind::Emit,
    }),
    ("E171", ExplainEntry {
        title: "Too many constraints in occbin_constraints",
        body: "An ``occbin_constraints`` block lists more than two named constraints. Dynare refuses: `only up to two constraints are supported in 'occbin_constraints' block`.\n\n**Fix**\n\nRemove the extra constraint. OccBin supports at most two.",
        kind: ExplainKind::Emit,
    }),
    ("E172", ExplainEntry {
        title: "OccBin regime is not defined",
        body: "A model equation with ``bind`` / ``relax`` tags is missing a copy for one combination of those constraints. Dynare refuses: `for equation 'NAME', the regime corresponding to bind='…' and relax='…' is not defined`.\n\n**Fix**\n\nAdd the missing bind or relax equation for that ``name``.",
        kind: ExplainKind::Emit,
    }),
    ("E173", ExplainEntry {
        title: "bind or relax tag without a name tag",
        body: "A model equation has a ``bind`` or ``relax`` tag but no ``name`` tag. Dynare refuses: `An equation with a 'bind' or 'relax' tag must have a 'name' tag`.\n\n**Fix**\n\nAdd ``[name='…']`` on that equation.",
        kind: ExplainKind::Emit,
    }),
    ("E174", ExplainEntry {
        title: "Missing bind expression",
        body: "A named OccBin constraint has no ``bind`` inequality (a non-comparison such as ``bind i;`` counts as missing). Dynare refuses: `The 'bind' expression is missing in constraint 'NAME'`.\n\n**Fix**\n\nAdd a ``bind`` inequality on that constraint.",
        kind: ExplainKind::Emit,
    }),
    ("E175", ExplainEntry {
        title: "No equation for an OccBin constraint",
        body: "An ``occbin_constraints`` name is never mentioned in a ``bind`` or ``relax`` equation tag. Dynare refuses: `No equation has been declared for constraint 'NAME'`.\n\n**Fix**\n\nAdd a model equation tagged ``bind`` or ``relax`` with that constraint name.",
        kind: ExplainKind::Emit,
    }),
    ("E176", ExplainEntry {
        title: "Constraint listed in both bind and relax",
        body: "The same constraint name appears in both the ``bind`` and ``relax`` tags on one equation. Dynare refuses: `The constraint 'C' is both in the 'bind' and 'relax' tags`.\n\n**Fix**\n\nKeep the name in ``bind`` or ``relax``, not both.",
        kind: ExplainKind::Emit,
    }),
    ("E177", ExplainEntry {
        title: "Duplicate OccBin regime",
        body: "Two equations with the same ``name`` tag declare the same bind/relax combination. Dynare refuses: `The regime corresponding to bind='…' has already been declared for this equation`.\n\n**Fix**\n\nRemove the duplicate regime equation.",
        kind: ExplainKind::Emit,
    }),
    ("E178", ExplainEntry {
        title: "shocks(surprise) without occbin_constraints",
        body: "A ``shocks(surprise)`` block requires an ``occbin_constraints`` block. Dynare refuses: `the 'shocks(surprise)' block can only be used in conjunction with the 'occbin_constraints' block.`.\n\n**Fix**\n\nAdd ``occbin_constraints``, or drop the ``surprise`` option.",
        kind: ExplainKind::Emit,
    }),
    ("E179", ExplainEntry {
        title: "occbin_constraints with an incompatible command",
        body: "An ``occbin_constraints`` block can only be used with ``estimation``, ``stoch_simul``, and ``calib_smoother``. Dynare refuses: `the 'occbin_constraints' block is not compatible with commands other than 'estimation', 'stoch_simul', and 'calib_smoother'.`.\n\n**Fix**\n\nRemove the incompatible command, or drop ``occbin_constraints``.",
        kind: ExplainKind::Emit,
    }),
    ("E180", ExplainEntry {
        title: "mcp tag and perpendicular together",
        body: "One equation has both an ``mcp`` tag and a complementarity condition after ``⟂`` / ``_|_``. Dynare refuses: `Can't have both an 'mcp' tag and a complementarity condition after the perpendicular symbol`.\n\n**Fix**\n\nKeep one form.",
        kind: ExplainKind::Emit,
    }),
    ("E181", ExplainEntry {
        title: "bind or relax is not an inequality",
        body: "The ``bind`` or ``relax`` expression is a comparison other than ``<``, ``>``, ``<=``, or ``>=`` (for example ``==``). Dynare refuses: `The 'bind' expression must be an inequality constraint` / `The 'relax' expression must be an inequality constraint`. A non-binary ``bind i;`` is a missing bind (E174), not this code.\n\n**Fix**\n\nWrite an inequality.",
        kind: ExplainKind::Emit,
    }),
    ("E182", ExplainEntry {
        title: "Forbidden expression in occbin_constraints",
        body: "An ``occbin_constraints`` expression uses a lead/lag, a model-local, an exogenous variable, ``EXPECTATION``, ``var_expectation``, ``pac_expectation``, ``pac_target_nonstationary``, or ``SUM()``. Dynare refuses, for example: `Leads and lags on variables are forbidden in 'occbin_constraints'. Note that you can achieve the same effect by introducing an auxiliary variable in the model.`; `Model local variable z cannot be used in 'occbin_constraints'.`; `Exogenous variable e cannot be used in 'occbin_constraints'.`; `The 'expectation' operator is forbidden in 'occbin_constraints'.`; `The SUM() operator is forbidden in occbin_constraints block`. ``STEADY_STATE`` itself is allowed.\n\n**Fix**\n\nUse contemporaneous endogenous variables (and parameters). Introduce an auxiliary variable for leads, lags, or exogenous terms.",
        kind: ExplainKind::Emit,
    }),
    ("E183", ExplainEntry {
        title: "Complementarity condition has an incorrect form",
        body: "The condition after ``⟂`` / ``_|_`` is not an inequality on a contemporaneous endogenous variable with constant bounds. Dynare refuses: `Complementarity condition has an incorrect form` (an extra ``: …`` detail is appended when that form can be named).\n\n**Fix**\n\nWrite an inequality such as ``i >= 0`` or ``0 <= i <= 1``.",
        kind: ExplainKind::Emit,
    }),
    ("E184", ExplainEntry {
        title: "Duplicate clause in an OccBin constraint",
        body: "A named constraint repeats ``bind``, ``relax``, ``error_bind``, or ``error_relax``. Dynare refuses: `The '{}' clause is declared multiple times`. The last copy is what the parser keeps.\n\n**Fix**\n\nKeep a single copy of that clause.",
        kind: ExplainKind::Emit,
    }),
    ("E185", ExplainEntry {
        title: "Invalid or reused OccBin constraint name",
        body: "A ``bind`` / ``relax`` tag piece or an ``occbin_constraints`` name is not a letter-or-underscore identifier, or ``occbin_NAME_bind`` is already declared as a variable. Dynare refuses: `The string '{}' is not a valid Occbin constraint name (contains unauthorized characters)` and `The name '{}' is already used. Please use another name for Occbin constraint '{}'`.\n\n**Fix**\n\nUse letters, digits, and underscores, and pick a name that does not collide with ``occbin_NAME_bind``.",
        kind: ExplainKind::Emit,
    }),
    ("W131", ExplainEntry {
        title: "Variable silently overwritten in steady_state_model",
        body: "A variable is assigned more than once in the ``steady_state_model`` block and the later assignment does not use the earlier value. Dynare warns: `in the 'steady_state_model' block, variable 'n' is declared twice`. (An in-place transformation that reuses the value, such as the ``A = log(A)`` log-model idiom, is intentional and is not flagged.)\n\n**Fix**\n\nRemove the redundant assignment, or fold the two into one.",
        kind: ExplainKind::Emit,
    }),
    ("W140", ExplainEntry {
        title: "Nonlinear operator in a linear model",
        body: "The model is declared ``linear`` (``model(linear);``) but an equation applies an operator Dynare still accepts at check, such as ``log``, ``exp``, products, division, or powers, to a variable.\n\n**Fix**\n\nRemove the ``linear`` option, or rewrite the equation without the nonlinear operator.",
        kind: ExplainKind::Added,
    }),
    ("W150", ExplainEntry {
        title: "Deprecated command or option",
        body: "A deprecated command or option is used. Dynare warns: `The 'simul' statement is deprecated. Please use 'perfect_foresight_setup' and 'perfect_foresight_solver' instead.`; `The 'ramsey_policy' statement is deprecated. Please use 'ramsey_model', 'stoch_simul', and 'evaluate_planner_objective' instead.`; `The 'aim_solver' option is deprecated. It has been superseded by the 'dr=aim' option.`; `the 'bytecode' option is deprecated and will be removed in a future release of Dynare.`\n\n**Fix**\n\nSwitch to the modern command or option form.",
        kind: ExplainKind::Emit,
    }),
    ("W160", ExplainEntry {
        title: "Named companion file was not found",
        body: "A named option or quoted path points at a companion file that was not found next to the `.mod` or on the search paths. Missing convention files (`FILENAME_steadystate.m`, `FILENAME_prior_restrictions.m`, `run_FILENAME.m`) and a missing identifier helper are not this warning.\n\n**Fix**\n\nAdd the file next to this `.mod`, correct the path, or add its directory to the search paths.",
        kind: ExplainKind::Added,
    }),
    ("W170", ExplainEntry {
        title: "Obsolete mcp complementarity tag",
        body: "A complementarity condition is written with the ``mcp`` tag and no ``⟂`` / ``_|_`` after the equation. Dynare accepts and warns: `Specifying complementarity conditions with the 'mcp' tag is obsolete. Please consider switching to the new syntax using the perpendicular symbol.` An equation that has both forms is an Error (E180), not this Warning.\n\n**Warrant**\n\nThe editor keeps the shorter `Use ⟂ or _|_ after the equation` line instead of Dynare's longer `Please consider switching…` sentence.\n\n**Fix**\n\nWrite the condition after ``⟂`` or ``_|_`` instead of ``[mcp=…]``.",
        kind: ExplainKind::Emit,
    }),
    ("E200", ExplainEntry {
        title: "write_latex_steady_state_model without steady_state_model",
        body: "Dynare refuses: `You cannot have a write_latex_steady_state_model statement without a steady_state_model block.`\n\n**Fix**\n\nAdd a ``steady_state_model`` block, or drop ``write_latex_steady_state_model``.",
        kind: ExplainKind::Emit,
    }),
    ("E201", ExplainEntry {
        title: "No model equation with a run command",
        body: "Dynare refuses: `At least one model equation must be declared!` when the file has no non-``#`` model equation and a ``check``, perfect-foresight solver, PFEE solver, or stochastic command is present.\n\n**Fix**\n\nAdd at least one model equation, or drop the run command.",
        kind: ExplainKind::Emit,
    }),
    ("E202", ExplainEntry {
        title: "discretionary_policy with Ramsey",
        body: "Dynare refuses: `You cannot use the discretionary_policy command when you use either ramsey_model or ramsey_policy and vice versa`.\n\n**Fix**\n\nKeep either discretionary policy or Ramsey, not both.",
        kind: ExplainKind::Emit,
    }),
    ("E203", ExplainEntry {
        title: "ramsey_constraints without Ramsey",
        body: "Dynare refuses: `A ramsey_constraints block requires the presence of a ramsey_model or ramsey_policy statement`.\n\n**Fix**\n\nAdd ``ramsey_model`` or ``ramsey_policy``, or drop ``ramsey_constraints``.",
        kind: ExplainKind::Emit,
    }),
    ("E204", ExplainEntry {
        title: "osr has both optim_weights and planner_objective",
        body: "Dynare refuses: `The osr statement cannot have both optim_weights and a planner_objective; they are mutually exclusive.`\n\n**Fix**\n\nKeep either ``optim_weights`` or ``planner_objective``, not both.",
        kind: ExplainKind::Emit,
    }),
    ("E205", ExplainEntry {
        title: "Perfect-foresight and stochastic commands in the same file",
        body: "Dynare refuses: `A .mod file cannot contain both one of {perfect_foresight_solver, simul, perfect_foresight_with_expectation_errors_solver} and one of {stoch_simul, estimation, osr, ramsey_policy, discretionary_policy}. This is not possible: one cannot mix perfect foresight context with stochastic context in the same file.`\n\n**Fix**\n\nKeep either the perfect-foresight solver or the stochastic command, not both.",
        kind: ExplainKind::Emit,
    }),
    ("E206", ExplainEntry {
        title: "model use_dll with bytecode",
        body: "Dynare refuses: `In 'model' block, 'use_dll' option is not compatible with 'bytecode'`.\n\n**Fix**\n\nDrop ``use_dll`` or drop ``bytecode``.",
        kind: ExplainKind::Emit,
    }),
    ("E207", ExplainEntry {
        title: "no_static with a stochastic, steady, or check command",
        body: "Dynare refuses: `no_static option is incompatible with stoch_simul, estimation, osr, ramsey_policy, discretionary_policy, steady and check commands`.\n\n**Fix**\n\nDrop ``no_static``, or drop the incompatible command.",
        kind: ExplainKind::Emit,
    }),
    ("E208", ExplainEntry {
        title: "[static] and [dynamic] equation counts differ",
        body: "Dynare refuses: `the number of equations marked [static] must be equal to the number of equations marked [dynamic]`.\n\n**Fix**\n\nGive each ``[static]`` equation a matching ``[dynamic]`` equation, or drop the tags.",
        kind: ExplainKind::Emit,
    }),
    ("E209", ExplainEntry {
        title: "[static]/[dynamic] tags with Ramsey or discretionary policy",
        body: "Dynare refuses: `marking equations as [static] or [dynamic] is not possible with ramsey_model, ramsey_policy or discretionary_policy`.\n\n**Fix**\n\nDrop the tags, or drop the Ramsey / discretionary command.",
        kind: ExplainKind::Emit,
    }),
    ("W200", ExplainEntry {
        title: "Nonsmooth operator in a stochastic context",
        body: "Dynare warns: `you are using a function (max, min, abs, sign) or an operator (<, >, <=, >=, ==, !=) which is unsuitable for a stochastic context; see the reference manual, section about \"Expressions\", for more details.`\n\n**Fix**\n\nRewrite without those operators, or drop the stochastic command if the file is meant to be perfect foresight.",
        kind: ExplainKind::Emit,
    }),
    ("E210", ExplainEntry {
        title: "Nonsmooth operator on an endogenous in a linear model",
        body: "Dynare refuses: `you have declared your model 'linear' but you are using a function (max, min, abs, sign) or an operator (<, >, <=, >=, ==, !=) on an endogenous variable.`\n\n**Fix**\n\nDrop ``linear``, or rewrite without that operator on endogenous variables.",
        kind: ExplainKind::Emit,
    }),
    ("E211", ExplainEntry {
        title: "Nonsmooth operator on an exogenous in a linear non-PF model",
        body: "Dynare refuses: `you have declared your model 'linear' but you are using a function (max, min, abs, sign) or an operator (<, >, <=, >=, ==, !=) on an exogenous variable in a non-perfect-foresight context.`\n\n**Fix**\n\nDrop ``linear``, add a perfect-foresight solver, or rewrite without that operator on exogenous variables.",
        kind: ExplainKind::Emit,
    }),
    ("E212", ExplainEntry {
        title: "Estimated parameter used in a shock expression",
        body: "Dynare refuses: `some estimated parameters (…) also appear in the expressions defining the variance/covariance matrix of shocks; this is not allowed.`\n\n**Fix**\n\nUse a calibrated parameter in the shocks block, or drop that name from ``estimated_params``.",
        kind: ExplainKind::Emit,
    }),
    ("E213", ExplainEntry {
        title: "perfect_foresight_solver before setup",
        body: "Dynare refuses: `A 'perfect_foresight_setup' command must come before 'perfect_foresight_solver'`.\n\n**Fix**\n\nPut ``perfect_foresight_setup;`` before ``perfect_foresight_solver;``.",
        kind: ExplainKind::Emit,
    }),
    ("E214", ExplainEntry {
        title: "PFEE solver before PFEE setup",
        body: "Dynare refuses: `A 'perfect_foresight_with_expectation_errors_setup' command must come before 'perfect_foresight_with_expectation_errors_solver'`.\n\n**Fix**\n\nPut the PFEE setup command before the PFEE solver.",
        kind: ExplainKind::Emit,
    }),
    ("E215", ExplainEntry {
        title: "discretionary_policy without instruments",
        body: "Dynare refuses: `discretionary_policy: the instruments option is required.`\n\n**Fix**\n\nAdd ``instruments=(…)`` on ``discretionary_policy``.",
        kind: ExplainKind::Emit,
    }),
    ("E216", ExplainEntry {
        title: "extended_path without periods",
        body: "Dynare refuses: `the 'periods' option of 'extended_path' is mandatory`.\n\n**Fix**\n\nWrite ``extended_path(periods=…);``.",
        kind: ExplainKind::Emit,
    }),
    ("E217", ExplainEntry {
        title: "initval after endval",
        body: "Dynare refuses: `an 'initval' block cannot appear after an 'endval' block`.\n\n**Fix**\n\nMove ``initval`` before ``endval``, or drop one of the blocks.",
        kind: ExplainKind::Emit,
    }),
    ("E218", ExplainEntry {
        title: "initval/endval all_values_required is incomplete",
        body: "``initval`` or ``endval`` was opened with ``all_values_required`` but some variables have no assignment. Dynare refuses: `You have not set the following endogenous variables in initval:`; `You have not set the following exogenous variables in initval:`; `You have not set the following endogenous variables in endval:`; `You have not set the following exogenous variables in endval:`.\n\n**Fix**\n\nAssign every endogenous and exogenous in that block, or drop ``all_values_required``.",
        kind: ExplainKind::Emit,
    }),
    ("E186", ExplainEntry {
        title: "Unused endogenous after substitution",
        body: "Dynare refuses: `Error: <name> not used in the model block`. Catching step: transform (rewrite). Owner: skip-rewrite E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("E187", ExplainEntry {
        title: "Ramsey FOCs leave Lagrange unused",
        body: "Dynare refuses: `the following Lagrange multiplier(s) do(es) not appear in first-order conditions`. Catching step: transform (rewrite). Owner: skip-rewrite E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("E188", ExplainEntry {
        title: "Equation count after AUX, Ramsey, log, or leads",
        body: "Dynare refuses: `There are <n> equations but <m> endogenous variables!`. Catching step: transform (rewrite). Owner: skip-rewrite E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("E189", ExplainEntry {
        title: "Division by zero substituting constants",
        body: "Dynare refuses: `Division by zero when substituting constants…`. Catching step: transform (rewrite). Owner: skip-rewrite E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("E190", ExplainEntry {
        title: "Partial information EXPECTATION(0) is not a single variable",
        body: "Dynare refuses: `EXPECTATION(0)(X) can only be used when X is a single variable`. Catching step: transform (rewrite). Owner: skip-rewrite E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("W186", ExplainEntry {
        title: "Possible auxiliary name in a symbol list",
        body: "Dynare warns: `WARNING: symbol_list variable … possible auxiliary variable name`. Catching step: check. Owner: skip-rewrite W. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S001", ExplainEntry {
        title: "dsge_prior_weight already declared with dsge_var",
        body: "Dynare refuses: `dsge_prior_weight should not be declared as a model variable / parameter when the dsge_var option is passed`. Catching step: transform (written clash). Owner: skip 0.6 E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S002", ExplainEntry {
        title: "shocks(learnt_in) without PFEE setup and solver",
        body: "Dynare refuses: `'shocks(learnt_in=…)' block can only be used in conjunction with…`. Catching step: transform (written clash). Owner: skip 0.11 E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S003", ExplainEntry {
        title: "endval(learnt_in) without PFEE setup and solver",
        body: "Dynare refuses: `'endval(learnt_in=…)' block can only be used…`. Catching step: transform (written clash). Owner: skip 0.11 E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S004", ExplainEntry {
        title: "perfect_foresight_controlled_paths(learnt_in) without PFEE",
        body: "Dynare refuses: `'perfect_foresight_controlled_paths(learnt_in=…)'…`. Catching step: transform (written clash). Owner: skip 0.11 E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S005", ExplainEntry {
        title: "shock_paths(learnt_in) without PFEE setup and solver",
        body: "Dynare refuses: `'shock_paths(learnt_in=…)'…`. Catching step: transform (written clash). Owner: skip 0.11 E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S006", ExplainEntry {
        title: "DSGE-VAR bayesian_irf shock count",
        body: "Dynare refuses: `When estimating a DSGE-Var and the bayesian_irf option… the number of shocks must equal…`. Catching step: transform (written clash). Owner: skip 0.6 E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S007", ExplainEntry {
        title: "DSGE-VAR fewer shocks than observed variables",
        body: "Dynare refuses: `number of shocks must be greater than or equal to the number of observed variables`. Catching step: transform (written clash). Owner: skip 0.6 E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S008", ExplainEntry {
        title: "Heterogeneous model equation count after AUX",
        body: "Dynare refuses: `There are <n> equations but <m> endogenous variables in the model for heterogeneity dimension`. Catching step: transform (rewrite). Owner: skip-rewrite E (0.9). This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S009", ExplainEntry {
        title: "var(log) endogenous in a VAR, TCM, or PAC equation",
        body: "Dynare refuses: `the following variables are declared with var(log) and therefore cannot appear in a VAR/TCM/PAC equation`. Catching step: transform (written clash). Owner: skip 0.8 E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S010", ExplainEntry {
        title: "var_model or TCM equation name does not exist",
        body: "Dynare refuses: `no equation is named '<eqtag>' / looking for equation tag … failed`. Catching step: transform (written clash). Owner: skip 0.8 E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S011", ExplainEntry {
        title: "VAR or TCM equation shape",
        body: "Dynare refuses: `in Equation <tag>. A VAR/trend component model may only…`. Catching step: transform (written clash). Owner: skip 0.8 E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S012", ExplainEntry {
        title: "TCM or PAC rewrite",
        body: "Dynare refuses: `PAC/TCM rewrite messages`. Catching step: transform (rewrite). Owner: skip-rewrite 0.8 E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S013", ExplainEntry {
        title: "var_expectation_model auxiliary, name, or linear form",
        body: "Dynare refuses: `var_expectation_model <name> refers to nonexistent… / not expected form / name used several times`. Catching step: transform (written clash). Owner: skip 0.8 E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S014", ExplainEntry {
        title: "Remaining pac_expectation after substitution",
        body: "Dynare refuses: `unknown pac_model / no matching pac_target_info`. Catching step: transform (rewrite). Owner: skip-rewrite 0.8 E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S015", ExplainEntry {
        title: "Default equation tag collides with existing name",
        body: "Dynare refuses: `Error creating default equation tag…`. Catching step: transform (written clash). Owner: skip 0.6 E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S016", ExplainEntry {
        title: "exclude_eqs, include_eqs, model_remove, or model_replace",
        body: "Dynare refuses: `various exclude_eqs / model_remove…`. Catching step: transform (written clash). Owner: skip 0.6 E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S017", ExplainEntry {
        title: "dsge_prior_weight missing from estimated_params",
        body: "Dynare refuses: `dsge_prior_weight must be referenced in the estimated_params block`. Catching step: check. Owner: skip 0.6 E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S018", ExplainEntry {
        title: "dsge_prior_weight estimated with calibrated dsge_var",
        body: "Dynare refuses: `the prior weight cannot be calibrated via the dsge_var option`. Catching step: check. Owner: skip 0.6 E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S019", ExplainEntry {
        title: "dsge_prior_weight estimated without dsge_var",
        body: "Dynare refuses: `the dsge_var option must be passed to the estimation statement`. Catching step: check. Owner: skip 0.6 E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S020", ExplainEntry {
        title: "Heterogeneity with an unsupported command or option",
        body: "Dynare refuses: `'<cmd>' … is not supported for heterogeneous models`. Catching step: check. Owner: skip 0.9 E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S021", ExplainEntry {
        title: "discretionary_policy order greater than 1",
        body: "Dynare refuses: `order > 1 is not yet implemented`. Catching step: check. Owner: skip 0.6 E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S022", ExplainEntry {
        title: "Undeclared or wrong-type name in a command symbol list",
        body: "Dynare refuses: `<cmd>: Variable X was not declared / is not one of {…}`. Catching step: check. Owner: skip 0.6 E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S023", ExplainEntry {
        title: "Multiple HP or bandpass filters",
        body: "Dynare refuses: `can only use one of HP, one-sided HP, and bandpass filters`. Catching step: check. Owner: skip 0.6 E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S024", ExplainEntry {
        title: "estimation dsge_varlag without dsge_var",
        body: "Dynare refuses: `DSGE-VAR option messages`. Catching step: check. Owner: skip 0.6 E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S025", ExplainEntry {
        title: "estimation without a data file",
        body: "Dynare refuses: `requires a data file to be supplied via the datafile option`. Catching step: check. Owner: skip 0.6 E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S026", ExplainEntry {
        title: "estimation option clash",
        body: "Dynare refuses: `estimation option messages`. Catching step: check. Owner: skip 0.6 E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S027", ExplainEntry {
        title: "prior_function or posterior_function without function",
        body: "Dynare refuses: `require the 'function' option`. Catching step: check. Owner: skip 0.6 E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S028", ExplainEntry {
        title: "Duplicate or invalid estimated_params entry",
        body: "Dynare refuses: `in '<block>' block…`. Catching step: check. Owner: skip 0.6 E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S029", ExplainEntry {
        title: "Multiple osr_params statements",
        body: "Dynare warns: `WARNING: You have more than one osr_params statement`. Catching step: check. Owner: skip 0.6 W. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S030", ExplainEntry {
        title: "osr_params_bounds before osr_params",
        body: "Dynare refuses: `you must have an osr_params statement before the osr_params_bounds block`. Catching step: check. Owner: skip 0.6 E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S031", ExplainEntry {
        title: "Exogenous variable in planner_objective",
        body: "Dynare refuses: `You cannot include exogenous variables … in the planner objective`. Catching step: check. Owner: skip 0.6 E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S032", ExplainEntry {
        title: "method_of_moments option error",
        body: "Dynare refuses: `MoM messages`. Catching step: check. Owner: skip 0.7 E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S033", ExplainEntry {
        title: "histval(all_values_required) incomplete",
        body: "Dynare refuses: `You have not set the following endogenous/exogenous variables in initval/endval`. Catching step: check. Owner: skip 0.6 E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S034", ExplainEntry {
        title: "shocks variance or correlation on the wrong type",
        body: "Dynare refuses: `shocks: setting a variance/standard error on '…' is not allowed…`. Catching step: check. Owner: skip 0.6 E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S035", ExplainEntry {
        title: "Heterogeneous shocks with a bad type",
        body: "Dynare refuses: `not a heterogeneous exogenous variable`. Catching step: check. Owner: skip 0.9 E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S036", ExplainEntry {
        title: "init2shocks duplicate endogenous",
        body: "Dynare refuses: `Init2shocks(name): enogenous variable '…' appears more than once`. Catching step: check. Owner: skip 0.6 E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S037", ExplainEntry {
        title: "shock_paths self reference without a lag",
        body: "Dynare refuses: `the use of 'self.…' without a lag is not allowed, since it is a circular reference`. Catching step: check. Owner: skip 0.11 E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S038", ExplainEntry {
        title: "PAC growth, auxname, or kind vs pac_target_info",
        body: "Dynare refuses: `PAC checkPass messages`. Catching step: check. Owner: skip 0.8 E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S039", ExplainEntry {
        title: "with_epilogue without epilogue",
        body: "Dynare refuses: `epilogue messages`. Catching step: check. Owner: skip 0.6 E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S040", ExplainEntry {
        title: "Heterogeneous lead or lag bound",
        body: "Dynare refuses: `In model(heterogeneity=…), equation N: …`. Catching step: check. Owner: skip 0.9 E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S041", ExplainEntry {
        title: "MS-SBVAR, markov_switching, or related command",
        body: "Dynare refuses: `various ms_* / data / prior ERROR`. Catching step: check. Owner: skip 0.6 E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S042", ExplainEntry {
        title: "identification order not in 1..3",
        body: "Dynare refuses: `the order option of identification command must be between 1 and 3`. Catching step: check. Owner: skip 0.6 E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S043", ExplainEntry {
        title: "restriction_fname is deprecated",
        body: "Dynare warns: `WARNING: restriction_fname is now deprecated`. Catching step: parse. Owner: skip 0.6 W. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S044", ExplainEntry {
        title: "@#includepath is not a directory",
        body: "Dynare refuses: `ERROR in macro-processor: … does not evaluate to a valid directory`. Catching step: parse. Owner: skip 0.6 E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S045", ExplainEntry {
        title: "Symbol listed twice on a command",
        body: "Dynare warns: `WARNING: In <cmd>: X found more than once in symbol list`. Catching step: parse. Owner: skip 0.6 W. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S046", ExplainEntry {
        title: "load_params_and_steady_state unknown symbol",
        body: "Dynare warns: `WARNING: Unknown symbol … in <file>`. Catching step: parse. Owner: skip 0.6 W. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S047", ExplainEntry {
        title: "load_params_and_steady_state cannot open file",
        body: "Dynare refuses: `ERROR: Can't open <file> / Unsupported variable type for …`. Catching step: parse. Owner: skip 0.6 E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S048", ExplainEntry {
        title: "identification max_dim_cova_group is 0",
        body: "Dynare refuses: `The max_dim_cova_group option to identification only accepts integers > 0`. Catching step: parse. Owner: skip 0.6 E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S049", ExplainEntry {
        title: "Constant-fold log(0) or division by zero",
        body: "Dynare refuses: `ERROR: log(0) not defined! / log10(0) not defined! / Division by zero`. Catching step: parse. Owner: skip 0.6 E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S050", ExplainEntry {
        title: "Option declared twice or empty vector",
        body: "Dynare refuses: `various`. Catching step: parse. Owner: skip 0.6 E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S051", ExplainEntry {
        title: "change_type, statement-local clash, or several ramsey_*",
        body: "Dynare refuses: `various`. Catching step: parse. Owner: skip 0.6 E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S052", ExplainEntry {
        title: "Trend, histval, filter_initial_state, or related parse",
        body: "Dynare refuses: `various`. Catching step: parse. Owner: skip 0.6 E / 0.11 E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S053", ExplainEntry {
        title: "PAC, var_model, TCM, or var_expectation parse",
        body: "Dynare refuses: `various`. Catching step: parse. Owner: skip 0.8 E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S054", ExplainEntry {
        title: "Heterogeneity dimension unknown or twice",
        body: "Dynare refuses: `various`. Catching step: parse. Owner: skip 0.9 E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S055", ExplainEntry {
        title: "mcp form errors beyond E180 and W170",
        body: "Dynare refuses: `various`. Catching step: parse. Owner: skip 0.6 E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S056", ExplainEntry {
        title: "Equation tag twice",
        body: "Dynare refuses: `various`. Catching step: parse. Owner: skip 0.6 E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S057", ExplainEntry {
        title: "varobs, varexobs, or observation_trends declared twice",
        body: "Dynare refuses: `various`. Catching step: parse. Owner: skip 0.6 E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S058", ExplainEntry {
        title: "Planner objective lead, lag, or model-local",
        body: "Dynare refuses: `various`. Catching step: parse. Owner: skip 0.6 E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S059", ExplainEntry {
        title: "Macro type mismatch, @#for tuple, or @#if not bool",
        body: "Dynare refuses: `various`. Catching step: parse. Owner: skip 0.6 E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S060", ExplainEntry {
        title: "MATLAB function name used as a variable",
        body: "Dynare refuses: `various`. Catching step: parse. Owner: skip 0.6 E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
];

fn entry_map() -> &'static HashMap<&'static str, ExplainEntry> {
    static MAP: OnceLock<HashMap<&'static str, ExplainEntry>> = OnceLock::new();
    MAP.get_or_init(|| ENTRIES.iter().copied().collect())
}

/// Return `{title, body}` for a diagnostic code, or `None`.
///
/// Lookup is case-insensitive.
pub fn explain(code: &str) -> Option<ExplainEntry> {
    let key = code.to_ascii_uppercase();
    entry_map().get(key.as_str()).copied()
}

/// Render the explanation as a single markdown string, or `None`.
///
/// Heading uses the caller's `code` string.
pub fn render_markdown(code: &str) -> Option<String> {
    let entry = explain(code)?;
    Some(format!("### {code}: {}\n\n{}\n", entry.title, entry.body))
}

/// Sorted catalog keys that have explanations.
pub fn known_codes() -> Vec<&'static str> {
    let mut codes: Vec<&'static str> = ENTRIES.iter().map(|(code, _)| *code).collect();
    codes.sort_unstable();
    codes
}
