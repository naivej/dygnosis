//! Diagnostic code documentation.
//!
//! Mechanical port of `python_dynare_lsp/explain.py` `_ENTRIES` for the 70
//! codes shipped through 0.5.0, then kind `emit` / `skip` / `added`.
//! 136 keys = 43 emit + 27 added + 66 skip. Catalog **0.5.1** Errors
//! (D-clash / D-check) are not keys yet.
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
    /// Extra diagnostic they never report.
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

// 136 keys: 43 emit + 27 added + 66 skip.
static ENTRIES: &[(&str, ExplainEntry)] = &[
    ("E001", ExplainEntry {
        title: "Parse error",
        body: "The Dynare parser could not interpret the source. The diagnostic range points at the offending token or the nearest recoverable position.\n\n**Common causes**\n\n- Missing semicolon at the end of a declaration or equation\n- Unbalanced parentheses, braces, or block keywords\n- Malformed time subscript such as `y(1)` where `y(+1)` was meant\n- A reserved keyword used as an identifier\n\n**Fix**\n\nInspect the line cited and the line immediately preceding it. Dynare's preprocessor frequently flags the *next* line after a missing semicolon.",
        kind: ExplainKind::Emit,
    }),
    ("W013", ExplainEntry {
        title: "Equation count does not match endogenous variable count",
        body: "The number of equations inside the `model` block does not equal the number of endogenous variables declared in the `var` block. This is an extra Warning: they accept a non-square file at check. The LSP catches this within milliseconds of editing, before Dynare is invoked.\n\nWhen ramsey_model, ramsey_policy, or discretionary_policy is present and instruments= lists N unique names, the expected gap is −N, not equality; a square file still warns.\n\n**Fix**\n\n- Add a missing equation, or remove a duplicate one\n- Declare the missing endogenous variable in `var`, or remove an   extra declaration\n- Check whether a commented-out equation was intended to be   active\n- For ramsey_model / ramsey_policy / discretionary_policy with N instruments, the intended gap is −N; do not add equations only to make the file square",
        kind: ExplainKind::Added,
    }),
    ("E020", ExplainEntry {
        title: "Undeclared identifier in model block",
        body: "An identifier appears in the `model` block but is not declared as a `var`, `varexo`, or `parameters` symbol. The diagnostic names the exact identifier and the equation it appears in.\n\n**Fix**\n\n- Add the identifier to the appropriate declaration block\n- Correct a typo (the LSP suggests close matches when available)\n- If the symbol is a local helper, define it in the parameter   section before use",
        kind: ExplainKind::Emit,
    }),
    ("E023", ExplainEntry {
        title: "Predetermined variable not declared endogenous",
        body: "A name listed in `predetermined_variables` must also be declared as an endogenous variable in the `var` block. Dynare requires the variable to exist before it can be marked predetermined.\n\n**Fix**\n\n- Add the variable to the `var` declaration, or\n- Remove it from `predetermined_variables` if it is not actually   endogenous",
        kind: ExplainKind::Emit,
    }),
    ("E024", ExplainEntry {
        title: "Deterministic exogenous with a lead or lag",
        body: "A deterministic exogenous variable (``varexo_det``) is used with a lead or lag. They refuse: `Exogenous deterministic variable tau cannot be given a lead or a lag.` Parameter leads/lags are accepted because Dynare treats them as fixed scalars.\n\n**Fix**\n\n- Remove the time subscript\n- If the dated quantity is state-dependent, model it as an endogenous variable instead",
        kind: ExplainKind::Emit,
    }),
    ("E025", ExplainEntry {
        title: "Invalid model-local (`#`) variable",
        body: "A model-local variable defined with `#` either reuses a declared `var`, `varexo`, or `parameters` name, or is used in an equation before its `#` definition. They refuse: `… has wrong type or was already used on the right-hand side. You cannot use it on the left-hand side of a pound ('#') expression`.\n\n**Fix**\n\n- Rename the model-local helper so it does not clash with a declared symbol\n- Move the `#` definition above its first use\n- Or remove the declaration if the name was meant to be only a model-local helper",
        kind: ExplainKind::Emit,
    }),
    ("E030", ExplainEntry {
        title: "Duplicate declaration across types or `#` twice",
        body: "The same identifier is declared in two different blocks (for example both `var` and `varexo`), or a model-local `#` name is defined twice. They refuse: `Symbol y declared twice with different types!` and `Local model variable foo declared twice.`\n\n**Fix**\n\nRemove the extra declaration. If you intended two related but distinct symbols, rename one.",
        kind: ExplainKind::Emit,
    }),
    ("W031", ExplainEntry {
        title: "Symbol declared twice with the same type",
        body: "The same identifier is declared more than once in `var`, `varexo`, `varexo_det`, or `parameters`. They accept and WARN `Symbol y declared twice.`\n\n**Fix**\n\nRemove the redundant declaration.",
        kind: ExplainKind::Emit,
    }),
    ("W054", ExplainEntry {
        title: "Duplicate equation",
        body: "Two equations inside the `model` block are textually identical. The diagnostic cites the line where the duplicate appears and the line where the first occurrence was found. This is an extra Warning: they accept duplicate equations. A duplicate can also show up as our equation-count Warning `W013`.\n\n**Fix**\n\nRemove the duplicate.",
        kind: ExplainKind::Added,
    }),
    ("W055", ExplainEntry {
        title: "Contradictory equation (always false)",
        body: "An equation reduces to a tautological falsehood, for example `0 = 1`. The LSP detects this by symbolic simplification of constant-only equations. This is an extra Warning: they accept `0 = 1` at check.\n\n**Fix**\n\nRemove the equation, or restore a variable reference that was accidentally simplified away.",
        kind: ExplainKind::Added,
    }),
    ("W056", ExplainEntry {
        title: "Duplicate parameter assignment",
        body: "The same parameter is assigned a value more than once in the parameter section. This is an extra Warning: they accept; the last assignment wins.\n\n**Fix**\n\nRemove one of the assignments, or rename if two distinct parameters were intended.",
        kind: ExplainKind::Added,
    }),
    ("W057", ExplainEntry {
        title: "Stray equation outside model block",
        body: "A line that looks like a model equation appears outside the `model` ... `end;` block. This is an extra Warning: they accept a stray top-level equation such as `0 = 1` at check.\n\n**Fix**\n\nMove the equation inside the `model` block, or convert it to a parameter assignment if it belongs at the top level.",
        kind: ExplainKind::Added,
    }),
    ("E060", ExplainEntry {
        title: "Circular @#include detected",
        body: "Two or more files reach themselves through the chain of `@#include` directives. Dynare's macro preprocessor expands includes inline, so a cycle would either loop forever or — in the real preprocessor — be rejected with a hard error. The language server reports the cycle as a chain of file names: `a.mod -> b.mod -> a.mod`.\n\n**Common causes**\n\n- A submodel was refactored and now includes its parent\n- Two helper files cross-include each other for shared\n  parameters or steady-state definitions\n- A copy-paste mistake duplicated the include in the wrong\n  direction\n\n**Fix**\n\nBreak the cycle by removing one `@#include` along the chain. If both files genuinely need a shared block, extract that block into a third file and have both parents include it.",
        kind: ExplainKind::Added,
    }),
    ("E061", ExplainEntry {
        title: "Cannot resolve @#include target",
        body: "An `@#include` directive names a file that the language server could not find. It looked in the directory of the including file first, then in each configured workspace search path, and ran out of candidates.\n\n**Common causes**\n\n- A typo in the filename\n- The included file lives in a directory that isn't on the   language server's search paths\n- The file was renamed or moved without updating the   directive\n\n**Fix**\n\nCorrect the filename, add the missing file, or extend the search paths so the directory containing the include is visible to the LSP.",
        kind: ExplainKind::Emit,
    }),
    ("E062", ExplainEntry {
        title: "Unmatched macro block",
        body: "A Dynare macro `@#if` block has no matching `@#endif`, or a `@#for` block has no matching `@#endfor`. Dynare's preprocessor expands these directives at build time and will reject unbalanced control flow.\n\n**Common causes**\n\n- A copy-paste deleted the closing directive\n- Mismatched closers — `@#endif` accidentally written for   a `@#for`, or vice versa\n- A nested block missing its inner closer\n\n**Fix**\n\nAdd the missing `@#endif` or `@#endfor` at the appropriate scope, or remove the stray closer. Each `@#if` needs its own `@#endif`; each `@#for` its own `@#endfor`.",
        kind: ExplainKind::Emit,
    }),
    ("E063", ExplainEntry {
        title: "Undefined macro interpolation",
        body: "An active line still contains an unresolved `@{NAME}` macro interpolation after the language server applied the simple `@#define` substitutions it can evaluate. Dynare's macro preprocessor cannot produce valid model code unless that macro name is defined in scope.\n\n**Fix**\n\nDefine the macro with `@#define NAME = value` before the line that uses it, correct the macro name, or remove the interpolation.",
        kind: ExplainKind::Emit,
    }),
    ("E064", ExplainEntry {
        title: "Macro error directive",
        body: "An active Dynare macro `@#error` directive was reached. Dynare's macro preprocessor stops when this directive is active.\n\n**Fix**\n\nRemove the `@#error` directive, or guard it behind a macro condition that is false for this model variant.",
        kind: ExplainKind::Emit,
    }),
    ("E065", ExplainEntry {
        title: "Invalid steady_state operand",
        body: "The `steady_state(...)` operator must refer to model endogenous or parameter expressions, not exogenous shocks. Exogenous variables are not valid operands for this Dynare operator.\n\n**Fix**\n\nRemove the exogenous variable from `steady_state(...)`, replace it with the intended endogenous or parameter expression, or rewrite the equation so the shock enters outside the operator.",
        kind: ExplainKind::Emit,
    }),
    ("E999", ExplainEntry {
        title: "Additional errors truncated",
        body: "More diagnostics were produced than the server displays at once. Fix the visible errors first; the next analysis pass will surface anything that was previously hidden.",
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
        body: "A shock declared in `varexo` does not appear in any equation. They refuse: `unused_exo not used in model block`. The `nostrict` option bypasses that check.",
        kind: ExplainKind::Emit,
    }),
    ("W022", ExplainEntry {
        title: "Parameter declared but never referenced in model equations",
        body: "A parameter is declared and assigned but does not appear in any model equation. Often the result of stripping an equation but forgetting to remove the parameter.",
        kind: ExplainKind::Emit,
    }),
    ("W042", ExplainEntry {
        title: "Endogenous variable missing from steady_state_model",
        body: "The `steady_state_model` block does not assign a value for every endogenous variable. Dynare will fall back to the `initval` value (or zero), which usually produces an inconsistent steady state.\n\n**Fix**\n\nAdd the missing assignments. For a numerical solve, use Dynare (for example `steady;` in MATLAB/Octave).",
        kind: ExplainKind::Emit,
    }),
    ("E058", ExplainEntry {
        title: "Undeclared variable in initval",
        body: "An entry in the `initval` block refers to a name that is not declared as a variable. They refuse: `Unknown symbol: undeclared_zzz`.\n\n**Fix**\n\nDeclare the variable, or remove the stray `initval` entry.",
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
        body: "An `initval` or `endval` entry names a symbol that is not endogenous or exogenous (for example a parameter). They refuse: `… is neither endogenous or exogenous.`\n\n**Fix**\n\nAssign parameters before the model block, or inside `steady_state_model`. Use `initval` / `endval` only for endogenous or exogenous variables.",
        kind: ExplainKind::Emit,
    }),
    ("W060", ExplainEntry {
        title: "Exogenous variables declared but no shocks block",
        body: "One or more exogenous variables are declared in `varexo` but the file contains no `shocks` block specifying their variance-covariance structure. The model is then deterministic.\n\n**Fix**\n\nAdd a `shocks` block to define the shock processes, or remove the unused `varexo` declarations.",
        kind: ExplainKind::Added,
    }),
    ("W061", ExplainEntry {
        title: "Ambiguous include parent context",
        body: "The active include file is reachable from more than one parent model, so the language server cannot safely infer which parent declarations and block context should apply.\n\n**Fix**\n\nOpen or run the intended parent `.mod` file, or provide only that parent and its include closure when calling workspace tools.",
        kind: ExplainKind::Added,
    }),
    ("W070", ExplainEntry {
        title: "Parameter outside its conventional range",
        body: "A parameter assignment falls outside the theoretically admissible range for its standard interpretation. The conventional-range table is opinionated but conservative: it flags values that violate the *theoretical* admissible range under the parameter's conventional meaning, not values that simply look unusual.\n\n**Common causes**\n\n- Unit error: e.g. `beta = 99` when 0.99 was meant\n- Sign error on a quantity that must be non-negative   (variance, standard deviation, depreciation rate)\n- Gross-vs-net confusion on a rate parameter\n\n**Fix**\n\nCorrect the value, or — if the calibration is intentional — ignore the warning. This is a soft check, not a structural error: Dynare will accept any numeric value.",
        kind: ExplainKind::Added,
    }),
    ("E090", ExplainEntry {
        title: "Observed variable is not a declared endogenous variable",
        body: "A name listed in ``varobs`` is not a declared endogenous variable. They refuse: `e is not endogenous.`\n\n**Fix**\n\nDeclare the variable in ``var``, or remove it from ``varobs`` if it was a typo or an exogenous/parameter name.",
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
        body: "An ``estimated_params`` entry names a symbol that is not declared with the expected role: a plain entry must name a parameter, an ``stderr`` entry must name a shock or observed variable, and a ``corr`` entry must name two declared shocks or variables. They refuse: `Unknown symbol: not_a_param`.\n\n**Fix**\n\nDeclare the symbol, or correct the name / entry type.",
        kind: ExplainKind::Emit,
    }),
    ("W094", ExplainEntry {
        title: "estimated_params bound or initial-value inconsistency",
        body: "An ``estimated_params`` entry has a lower bound that is not below its upper bound, or an initial value that lies outside the ``[lower, upper]`` interval. This is an extra Warning: they accept inconsistent bounds at check.\n\n**Fix**\n\nOrder the bounds so that lower < upper and place the initial value inside them.",
        kind: ExplainKind::Added,
    }),
    ("E095", ExplainEntry {
        title: "observation_trends variable not in varobs",
        body: "A variable given a trend in ``observation_trends`` is not listed in ``varobs``. They refuse: `variable y in observation_trends block is not an observed variable`.\n\n**Fix**\n\nAdd the variable to ``varobs`` or remove its trend specification.",
        kind: ExplainKind::Emit,
    }),
    ("E100", ExplainEntry {
        title: "Optimal-policy command requires a planner_objective",
        body: "``ramsey_model``, ``ramsey_policy``, and ``discretionary_policy`` optimise a planner's loss function. They refuse when ``planner_objective`` is missing: `A planner_objective statement must be used with a ramsey_model… and vice versa.`\n\n**Fix**\n\nAdd a ``planner_objective <expression>;`` statement before the policy command.",
        kind: ExplainKind::Emit,
    }),
    ("E101", ExplainEntry {
        title: "Policy instrument is not a declared endogenous variable",
        body: "An ``instruments=(...)`` entry names a symbol that is not a declared endogenous variable. They refuse: `Unknown symbol: not_endo`.\n\n**Fix**\n\nDeclare the instrument in ``var``, or correct the instrument name.",
        kind: ExplainKind::Emit,
    }),
    ("W102", ExplainEntry {
        title: "planner_discount is not a valid discount factor",
        body: "``planner_discount`` must be a discount factor in the interval (0, 1]. A value outside this range is almost certainly a mistake (for example, entering a discount rate instead of a factor).\n\n**Fix**\n\nSet ``planner_discount`` to a value such as 0.99.",
        kind: ExplainKind::Added,
    }),
    ("E103", ExplainEntry {
        title: "osr is missing osr_params or optim_weights",
        body: "Optimal simple rules (``osr``) need an ``osr_params`` statement (the parameters to optimise). They refuse when it is missing: `The osr statement requires the osr_params statement.` They also refuse when neither ``optim_weights`` nor ``planner_objective`` is present: `The osr statement requires either an optim_weights block or a planner_objective.` This check flags a missing ``osr_params`` statement and a missing ``optim_weights`` block.\n\n**Fix**\n\nAdd the missing ``osr_params`` statement and/or ``optim_weights`` block.",
        kind: ExplainKind::Emit,
    }),
    ("W110", ExplainEntry {
        title: "Shock correlation outside [-1, 1]",
        body: "A ``corr`` entry in the shocks block sets a correlation whose magnitude exceeds one. A correlation coefficient must lie in [-1, 1], and the implied covariance matrix would not be positive semidefinite.\n\n**Fix**\n\nSet the correlation to a value in [-1, 1].",
        kind: ExplainKind::Added,
    }),
    ("E111", ExplainEntry {
        title: "Shock variance or correlation specified more than once",
        body: "A shock's variance / standard error, or a correlation pair, is specified more than once in the shocks block. They refuse: `shocks: variance or stderr of shock on e declared twice`.\n\n**Fix**\n\nKeep a single specification per shock variance and per correlation pair.",
        kind: ExplainKind::Emit,
    }),
    ("W112", ExplainEntry {
        title: "Negative shock variance",
        body: "A shocks-block ``var e = ...`` entry sets a variance that folds to a negative constant. A variance is a squared quantity and should be non-negative. This is an extra Warning: they accept a negative ``var e`` at check.\n\n(The ``stderr`` form is not flagged: Dynare squares the standard error, so a negative ``stderr`` still yields a valid variance.)\n\n**Fix**\n\nUse a non-negative value. Recall the ``var`` form sets the variance, i.e. the standard error *squared* (e.g. ``var e = 0.01^2;``).",
        kind: ExplainKind::Added,
    }),
    ("W120", ExplainEntry {
        title: "Stochastic command with no stochastic exogenous variable",
        body: "``stoch_simul`` / ``estimation`` drive the model with stochastic shocks, but the model declares no stochastic ``varexo``. ``varexo_det`` declarations are deterministic and do not count as stochastic shocks. This is an extra Warning: they accept ``stoch_simul`` / ``estimation`` with no stochastic ``varexo`` at check.\n\n**Fix**\n\nDeclare at least one stochastic exogenous variable (a dummy ``varexo`` plus a shocks-block entry is enough if the model is otherwise deterministic).",
        kind: ExplainKind::Added,
    }),
    ("W121", ExplainEntry {
        title: "Parameter used with a lead or lag",
        body: "A declared parameter appears with a time subscript such as ``beta(+1)`` or ``rho(-1)`` in the model block. Parameters are time-invariant constants, so a lead/lag on one is meaningless and almost always means the symbol should have been declared as a variable, or that the time index is stray.\n\n**Fix**\n\nRemove the time index, or declare the symbol with ``var`` / ``varexo`` if it really is a variable.",
        kind: ExplainKind::Emit,
    }),
    ("W122", ExplainEntry {
        title: "Deep parameter assigned a non-finite value",
        body: "A parameter that is used in the model equations is assigned a non-finite value (``NaN`` or ``Inf``) while a run command (``steady``, ``stoch_simul``, ``perfect_foresight_*``, ``estimation``, ...) is present. This is an extra Warning: they accept ``Inf`` / ``NaN`` at check.\n\n**Fix**\n\nAssign a finite numeric value before the run command.",
        kind: ExplainKind::Added,
    }),
    ("E130", ExplainEntry {
        title: "Variable used before assignment in steady_state_model",
        body: "The ``steady_state_model`` block is evaluated top to bottom as a sequence of assignments, so every variable on a right-hand side must already have been assigned above. A variable is referenced before its own assignment. They refuse: `variable 'n' is undefined in the declaration of variable 'log_n'`.\n\n**Fix**\n\nReorder the assignments so each variable is computed before it is used.",
        kind: ExplainKind::Emit,
    }),
    ("E170", ExplainEntry {
        title: "Multiple occbin_constraints blocks",
        body: "The file has more than one ``occbin_constraints`` block. They refuse: `Multiple 'occbin_constraints' blocks are not allowed`.\n\n**Fix**\n\nKeep a single ``occbin_constraints`` block.",
        kind: ExplainKind::Emit,
    }),
    ("E171", ExplainEntry {
        title: "Too many constraints in occbin_constraints",
        body: "An ``occbin_constraints`` block lists more than two named constraints. They refuse: `only up to two constraints are supported in 'occbin_constraints' block`.\n\n**Fix**\n\nRemove the extra constraint. OccBin supports at most two.",
        kind: ExplainKind::Emit,
    }),
    ("E172", ExplainEntry {
        title: "OccBin regime is not defined",
        body: "A model equation with ``bind`` / ``relax`` tags is missing a copy for one combination of those constraints. They refuse: `for equation 'NAME', the regime corresponding to bind='…' and relax='…' is not defined`.\n\n**Fix**\n\nAdd the missing bind or relax equation for that ``name``.",
        kind: ExplainKind::Emit,
    }),
    ("E173", ExplainEntry {
        title: "bind or relax tag without a name tag",
        body: "A model equation has a ``bind`` or ``relax`` tag but no ``name`` tag. They refuse: `An equation with a 'bind' or 'relax' tag must have a 'name' tag`.\n\n**Fix**\n\nAdd ``[name='…']`` on that equation.",
        kind: ExplainKind::Emit,
    }),
    ("E174", ExplainEntry {
        title: "Missing bind expression",
        body: "A named OccBin constraint has no ``bind`` inequality (a non-comparison such as ``bind i;`` counts as missing). They refuse: `The 'bind' expression is missing in constraint 'NAME'`.\n\n**Fix**\n\nAdd a ``bind`` inequality on that constraint.",
        kind: ExplainKind::Emit,
    }),
    ("E175", ExplainEntry {
        title: "No equation for an OccBin constraint",
        body: "An ``occbin_constraints`` name is never mentioned in a ``bind`` or ``relax`` equation tag. They refuse: `No equation has been declared for constraint 'NAME'`.\n\n**Fix**\n\nAdd a model equation tagged ``bind`` or ``relax`` with that constraint name.",
        kind: ExplainKind::Emit,
    }),
    ("E176", ExplainEntry {
        title: "Constraint listed in both bind and relax",
        body: "The same constraint name appears in both the ``bind`` and ``relax`` tags on one equation. They refuse: `The constraint 'C' is both in the 'bind' and 'relax' tags`.\n\n**Fix**\n\nKeep the name in ``bind`` or ``relax``, not both.",
        kind: ExplainKind::Emit,
    }),
    ("E177", ExplainEntry {
        title: "Duplicate OccBin regime",
        body: "Two equations with the same ``name`` tag declare the same bind/relax combination. They refuse: `The regime corresponding to bind='…' has already been declared for this equation`.\n\n**Fix**\n\nRemove the duplicate regime equation.",
        kind: ExplainKind::Emit,
    }),
    ("E180", ExplainEntry {
        title: "mcp tag and perpendicular together",
        body: "One equation has both an ``mcp`` tag and a complementarity condition after ``⟂`` / ``_|_``. They refuse: `Can't have both an 'mcp' tag and a complementarity condition after the perpendicular symbol`.\n\n**Fix**\n\nKeep one form.",
        kind: ExplainKind::Emit,
    }),
    ("E181", ExplainEntry {
        title: "bind or relax is not an inequality",
        body: "The ``bind`` or ``relax`` expression is a comparison other than ``<``, ``>``, ``<=``, or ``>=`` (for example ``==``). They refuse: `The 'bind' expression must be an inequality constraint` / `The 'relax' expression must be an inequality constraint`. A non-binary ``bind i;`` is a missing bind (E174), not this code.\n\n**Fix**\n\nWrite an inequality.",
        kind: ExplainKind::Emit,
    }),
    ("E182", ExplainEntry {
        title: "Forbidden expression in occbin_constraints",
        body: "An ``occbin_constraints`` expression uses a lead/lag, a model-local, an exogenous variable, ``EXPECTATION``, ``var_expectation``, ``pac_expectation``, ``pac_target_nonstationary``, or ``SUM()``. They refuse, for example: `Leads and lags on variables are forbidden in 'occbin_constraints'. Note that you can achieve the same effect by introducing an auxiliary variable in the model.`; `Model local variable z cannot be used in 'occbin_constraints'.`; `Exogenous variable e cannot be used in 'occbin_constraints'.`; `The 'expectation' operator is forbidden in 'occbin_constraints'.`; `The SUM() operator is forbidden in occbin_constraints block`. ``STEADY_STATE`` itself is allowed.\n\n**Fix**\n\nUse contemporaneous endogenous variables (and parameters). Introduce an auxiliary variable for leads, lags, or exogenous terms.",
        kind: ExplainKind::Emit,
    }),
    ("E183", ExplainEntry {
        title: "Complementarity condition has an incorrect form",
        body: "The condition after ``⟂`` / ``_|_`` is not an inequality on a contemporaneous endogenous variable with constant bounds. They refuse: `Complementarity condition has an incorrect form` (an extra ``: …`` detail is appended when that form can be named).\n\n**Fix**\n\nWrite an inequality such as ``i >= 0`` or ``0 <= i <= 1``.",
        kind: ExplainKind::Emit,
    }),
    ("E184", ExplainEntry {
        title: "Duplicate clause in an OccBin constraint",
        body: "A named constraint repeats ``bind``, ``relax``, ``error_bind``, or ``error_relax``. They refuse: `The '{}' clause is declared multiple times`. The last copy is what the parser keeps.\n\n**Fix**\n\nKeep a single copy of that clause.",
        kind: ExplainKind::Emit,
    }),
    ("E185", ExplainEntry {
        title: "Invalid or reused OccBin constraint name",
        body: "A ``bind`` / ``relax`` tag piece or an ``occbin_constraints`` name is not a letter-or-underscore identifier, or ``occbin_NAME_bind`` is already declared as a variable. They refuse: `The string '{}' is not a valid Occbin constraint name (contains unauthorized characters)` and `The name '{}' is already used. Please use another name for Occbin constraint '{}'`.\n\n**Fix**\n\nUse letters, digits, and underscores, and pick a name that does not collide with ``occbin_NAME_bind``.",
        kind: ExplainKind::Emit,
    }),
    ("W131", ExplainEntry {
        title: "Variable silently overwritten in steady_state_model",
        body: "A variable is assigned more than once in the ``steady_state_model`` block and the later assignment does not use the earlier value, so the first assignment is dead. (An in-place transformation that reuses the value, such as the ``A = log(A)`` log-model idiom, is intentional and is not flagged.)\n\n**Fix**\n\nRemove the redundant assignment, or fold the two into one.",
        kind: ExplainKind::Emit,
    }),
    ("W140", ExplainEntry {
        title: "Nonlinear operator in a linear model",
        body: "The model is declared ``linear`` (``model(linear);``) but an equation applies a nonlinear operator to a variable. Examples include variable-dependent functions (such as ``log(y)`` or ``abs(e)``), products or ratios involving multiple variables (``c*k`` or ``a/y``), powers involving variables (``k^2``), and comparisons. This is an extra Warning: they accept a nonlinear operator in ``model(linear)`` at check.\n\n**Fix**\n\nRemove the ``linear`` option, or rewrite the equation without the nonlinear operator.",
        kind: ExplainKind::Added,
    }),
    ("W150", ExplainEntry {
        title: "Deprecated command or option",
        body: "A deprecated command or option is used; current Dynare warns and may remove it in a future release. Commands: ``simul`` → ``perfect_foresight_setup`` + ``perfect_foresight_solver``; ``ramsey_policy`` → ``ramsey_model`` + ``stoch_simul``. Options: ``aim_solver`` → ``dr = aim``; ``bytecode`` (being removed).\n\n**Fix**\n\nSwitch to the modern command or option form.",
        kind: ExplainKind::Emit,
    }),
    ("W160", ExplainEntry {
        title: "Named companion file was not found",
        body: "A catalog option or quoted path names a companion file that this tool could not resolve next to the `.mod` or on the search paths. This is an extra Warning: they accept a missing named file at check. Missing convention files (`FILENAME_steadystate.m`, `FILENAME_prior_restrictions.m`, `run_FILENAME.m`) and a missing identifier helper are not this code.\n\n**Fix**\n\nAdd the file next to this `.mod`, correct the path, or add its directory to the search paths.",
        kind: ExplainKind::Added,
    }),
    ("W170", ExplainEntry {
        title: "Obsolete mcp complementarity tag",
        body: "A complementarity condition is written with the ``mcp`` tag and no ``⟂`` / ``_|_`` after the equation. They accept and WARN: `Specifying complementarity conditions with the 'mcp' tag is obsolete. Please consider switching to the new syntax using the perpendicular symbol.` An equation that has both forms is an Error (E180), not this Warning.\n\n**Fix**\n\nWrite the condition after ``⟂`` or ``_|_`` instead of ``[mcp=…]``.",
        kind: ExplainKind::Emit,
    }),
    ("E186", ExplainEntry {
        title: "Unused endogenous after substitution",
        body: "They refuse: `Error: <name> not used in the model block`. Catching step: transform (rewrite). Owner: skip-rewrite E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("E187", ExplainEntry {
        title: "Ramsey FOCs leave Lagrange unused",
        body: "They refuse: `the following Lagrange multiplier(s) do(es) not appear in first-order conditions`. Catching step: transform (rewrite). Owner: skip-rewrite E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("E188", ExplainEntry {
        title: "Equation count after AUX, Ramsey, log, or leads",
        body: "They refuse: `There are <n> equations but <m> endogenous variables!`. Catching step: transform (rewrite). Owner: skip-rewrite E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("E189", ExplainEntry {
        title: "Division by zero substituting constants",
        body: "They refuse: `Division by zero when substituting constants…`. Catching step: transform (rewrite). Owner: skip-rewrite E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("E190", ExplainEntry {
        title: "Partial information EXPECTATION(0) is not a single variable",
        body: "They refuse: `EXPECTATION(0)(X) can only be used when X is a single variable`. Catching step: transform (rewrite). Owner: skip-rewrite E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("W186", ExplainEntry {
        title: "Possible auxiliary name in a symbol list",
        body: "They WARN: `WARNING: symbol_list variable … possible auxiliary variable name`. Catching step: check. Owner: skip-rewrite W. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S001", ExplainEntry {
        title: "dsge_prior_weight already declared with dsge_var",
        body: "They refuse: `dsge_prior_weight should not be declared as a model variable / parameter when the dsge_var option is passed`. Catching step: transform (written clash). Owner: skip 0.6 E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S002", ExplainEntry {
        title: "shocks(learnt_in) without PFEE setup and solver",
        body: "They refuse: `'shocks(learnt_in=…)' block can only be used in conjunction with…`. Catching step: transform (written clash). Owner: skip 0.11 E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S003", ExplainEntry {
        title: "endval(learnt_in) without PFEE setup and solver",
        body: "They refuse: `'endval(learnt_in=…)' block can only be used…`. Catching step: transform (written clash). Owner: skip 0.11 E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S004", ExplainEntry {
        title: "perfect_foresight_controlled_paths(learnt_in) without PFEE",
        body: "They refuse: `'perfect_foresight_controlled_paths(learnt_in=…)'…`. Catching step: transform (written clash). Owner: skip 0.11 E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S005", ExplainEntry {
        title: "shock_paths(learnt_in) without PFEE setup and solver",
        body: "They refuse: `'shock_paths(learnt_in=…)'…`. Catching step: transform (written clash). Owner: skip 0.11 E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S006", ExplainEntry {
        title: "DSGE-VAR bayesian_irf shock count",
        body: "They refuse: `When estimating a DSGE-Var and the bayesian_irf option… the number of shocks must equal…`. Catching step: transform (written clash). Owner: skip 0.6 E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S007", ExplainEntry {
        title: "DSGE-VAR fewer shocks than observed variables",
        body: "They refuse: `number of shocks must be greater than or equal to the number of observed variables`. Catching step: transform (written clash). Owner: skip 0.6 E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S008", ExplainEntry {
        title: "Heterogeneous model equation count after AUX",
        body: "They refuse: `There are <n> equations but <m> endogenous variables in the model for heterogeneity dimension`. Catching step: transform (rewrite). Owner: skip-rewrite E (0.9). This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S009", ExplainEntry {
        title: "var(log) endogenous in a VAR, TCM, or PAC equation",
        body: "They refuse: `the following variables are declared with var(log) and therefore cannot appear in a VAR/TCM/PAC equation`. Catching step: transform (written clash). Owner: skip 0.8 E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S010", ExplainEntry {
        title: "var_model or TCM equation name does not exist",
        body: "They refuse: `no equation is named '<eqtag>' / looking for equation tag … failed`. Catching step: transform (written clash). Owner: skip 0.8 E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S011", ExplainEntry {
        title: "VAR or TCM equation shape",
        body: "They refuse: `in Equation <tag>. A VAR/trend component model may only…`. Catching step: transform (written clash). Owner: skip 0.8 E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S012", ExplainEntry {
        title: "TCM or PAC rewrite",
        body: "They refuse: `PAC/TCM rewrite messages`. Catching step: transform (rewrite). Owner: skip-rewrite 0.8 E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S013", ExplainEntry {
        title: "var_expectation_model auxiliary, name, or linear form",
        body: "They refuse: `var_expectation_model <name> refers to nonexistent… / not expected form / name used several times`. Catching step: transform (written clash). Owner: skip 0.8 E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S014", ExplainEntry {
        title: "Remaining pac_expectation after substitution",
        body: "They refuse: `unknown pac_model / no matching pac_target_info`. Catching step: transform (rewrite). Owner: skip-rewrite 0.8 E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S015", ExplainEntry {
        title: "Default equation tag collides with existing name",
        body: "They refuse: `Error creating default equation tag…`. Catching step: transform (written clash). Owner: skip 0.6 E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S016", ExplainEntry {
        title: "exclude_eqs, include_eqs, model_remove, or model_replace",
        body: "They refuse: `various exclude_eqs / model_remove…`. Catching step: transform (written clash). Owner: skip 0.6 E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S017", ExplainEntry {
        title: "dsge_prior_weight missing from estimated_params",
        body: "They refuse: `dsge_prior_weight must be referenced in the estimated_params block`. Catching step: check. Owner: skip 0.6 E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S018", ExplainEntry {
        title: "dsge_prior_weight estimated with calibrated dsge_var",
        body: "They refuse: `the prior weight cannot be calibrated via the dsge_var option`. Catching step: check. Owner: skip 0.6 E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S019", ExplainEntry {
        title: "dsge_prior_weight estimated without dsge_var",
        body: "They refuse: `the dsge_var option must be passed to the estimation statement`. Catching step: check. Owner: skip 0.6 E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S020", ExplainEntry {
        title: "Heterogeneity with an unsupported command or option",
        body: "They refuse: `'<cmd>' … is not supported for heterogeneous models`. Catching step: check. Owner: skip 0.9 E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S021", ExplainEntry {
        title: "discretionary_policy order greater than 1",
        body: "They refuse: `order > 1 is not yet implemented`. Catching step: check. Owner: skip 0.6 E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S022", ExplainEntry {
        title: "Undeclared or wrong-type name in a command symbol list",
        body: "They refuse: `<cmd>: Variable X was not declared / is not one of {…}`. Catching step: check. Owner: skip 0.6 E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S023", ExplainEntry {
        title: "Multiple HP or bandpass filters",
        body: "They refuse: `can only use one of HP, one-sided HP, and bandpass filters`. Catching step: check. Owner: skip 0.6 E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S024", ExplainEntry {
        title: "estimation dsge_varlag without dsge_var",
        body: "They refuse: `DSGE-VAR option messages`. Catching step: check. Owner: skip 0.6 E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S025", ExplainEntry {
        title: "estimation without a data file",
        body: "They refuse: `requires a data file to be supplied via the datafile option`. Catching step: check. Owner: skip 0.6 E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S026", ExplainEntry {
        title: "estimation option clash",
        body: "They refuse: `estimation option messages`. Catching step: check. Owner: skip 0.6 E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S027", ExplainEntry {
        title: "prior_function or posterior_function without function",
        body: "They refuse: `require the 'function' option`. Catching step: check. Owner: skip 0.6 E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S028", ExplainEntry {
        title: "Duplicate or invalid estimated_params entry",
        body: "They refuse: `in '<block>' block…`. Catching step: check. Owner: skip 0.6 E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S029", ExplainEntry {
        title: "Multiple osr_params statements",
        body: "They WARN: `WARNING: You have more than one osr_params statement`. Catching step: check. Owner: skip 0.6 W. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S030", ExplainEntry {
        title: "osr_params_bounds before osr_params",
        body: "They refuse: `you must have an osr_params statement before the osr_params_bounds block`. Catching step: check. Owner: skip 0.6 E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S031", ExplainEntry {
        title: "Exogenous variable in planner_objective",
        body: "They refuse: `You cannot include exogenous variables … in the planner objective`. Catching step: check. Owner: skip 0.6 E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S032", ExplainEntry {
        title: "method_of_moments option error",
        body: "They refuse: `MoM messages`. Catching step: check. Owner: skip 0.7 E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S033", ExplainEntry {
        title: "histval(all_values_required) incomplete",
        body: "They refuse: `You have not set the following endogenous/exogenous variables in initval/endval`. Catching step: check. Owner: skip 0.6 E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S034", ExplainEntry {
        title: "shocks variance or correlation on the wrong type",
        body: "They refuse: `shocks: setting a variance/standard error on '…' is not allowed…`. Catching step: check. Owner: skip 0.6 E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S035", ExplainEntry {
        title: "Heterogeneous shocks with a bad type",
        body: "They refuse: `not a heterogeneous exogenous variable`. Catching step: check. Owner: skip 0.9 E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S036", ExplainEntry {
        title: "init2shocks duplicate endogenous",
        body: "They refuse: `Init2shocks(name): enogenous variable '…' appears more than once`. Catching step: check. Owner: skip 0.6 E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S037", ExplainEntry {
        title: "shock_paths self reference without a lag",
        body: "They refuse: `the use of 'self.…' without a lag is not allowed, since it is a circular reference`. Catching step: check. Owner: skip 0.11 E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S038", ExplainEntry {
        title: "PAC growth, auxname, or kind vs pac_target_info",
        body: "They refuse: `PAC checkPass messages`. Catching step: check. Owner: skip 0.8 E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S039", ExplainEntry {
        title: "with_epilogue without epilogue",
        body: "They refuse: `epilogue messages`. Catching step: check. Owner: skip 0.6 E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S040", ExplainEntry {
        title: "Heterogeneous lead or lag bound",
        body: "They refuse: `In model(heterogeneity=…), equation N: …`. Catching step: check. Owner: skip 0.9 E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S041", ExplainEntry {
        title: "MS-SBVAR, markov_switching, or related command",
        body: "They refuse: `various ms_* / data / prior ERROR`. Catching step: check. Owner: skip 0.6 E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S042", ExplainEntry {
        title: "identification order not in 1..3",
        body: "They refuse: `the order option of identification command must be between 1 and 3`. Catching step: check. Owner: skip 0.6 E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S043", ExplainEntry {
        title: "restriction_fname is deprecated",
        body: "They WARN: `WARNING: restriction_fname is now deprecated`. Catching step: parse. Owner: skip 0.6 W. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S044", ExplainEntry {
        title: "@#includepath is not a directory",
        body: "They refuse: `ERROR in macro-processor: … does not evaluate to a valid directory`. Catching step: parse. Owner: skip 0.6 E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S045", ExplainEntry {
        title: "Symbol listed twice on a command",
        body: "They WARN: `WARNING: In <cmd>: X found more than once in symbol list`. Catching step: parse. Owner: skip 0.6 W. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S046", ExplainEntry {
        title: "load_params_and_steady_state unknown symbol",
        body: "They WARN: `WARNING: Unknown symbol … in <file>`. Catching step: parse. Owner: skip 0.6 W. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S047", ExplainEntry {
        title: "load_params_and_steady_state cannot open file",
        body: "They refuse: `ERROR: Can't open <file> / Unsupported variable type for …`. Catching step: parse. Owner: skip 0.6 E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S048", ExplainEntry {
        title: "identification max_dim_cova_group is 0",
        body: "They refuse: `The max_dim_cova_group option to identification only accepts integers > 0`. Catching step: parse. Owner: skip 0.6 E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S049", ExplainEntry {
        title: "Constant-fold log(0) or division by zero",
        body: "They refuse: `ERROR: log(0) not defined! / log10(0) not defined! / Division by zero`. Catching step: parse. Owner: skip 0.6 E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S050", ExplainEntry {
        title: "Option declared twice or empty vector",
        body: "They refuse: `various`. Catching step: parse. Owner: skip 0.6 E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S051", ExplainEntry {
        title: "change_type, statement-local clash, or several ramsey_*",
        body: "They refuse: `various`. Catching step: parse. Owner: skip 0.6 E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S052", ExplainEntry {
        title: "Trend, histval, filter_initial_state, or related parse",
        body: "They refuse: `various`. Catching step: parse. Owner: skip 0.6 E / 0.11 E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S053", ExplainEntry {
        title: "PAC, var_model, TCM, or var_expectation parse",
        body: "They refuse: `various`. Catching step: parse. Owner: skip 0.8 E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S054", ExplainEntry {
        title: "Heterogeneity dimension unknown or twice",
        body: "They refuse: `various`. Catching step: parse. Owner: skip 0.9 E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S055", ExplainEntry {
        title: "mcp form errors beyond E180 and W170",
        body: "They refuse: `various`. Catching step: parse. Owner: skip 0.6 E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S056", ExplainEntry {
        title: "Equation tag twice",
        body: "They refuse: `various`. Catching step: parse. Owner: skip 0.6 E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S057", ExplainEntry {
        title: "varobs, varexobs, or observation_trends declared twice",
        body: "They refuse: `various`. Catching step: parse. Owner: skip 0.6 E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S058", ExplainEntry {
        title: "Planner objective lead, lag, or model-local",
        body: "They refuse: `various`. Catching step: parse. Owner: skip 0.6 E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S059", ExplainEntry {
        title: "Macro type mismatch, @#for tuple, or @#if not bool",
        body: "They refuse: `various`. Catching step: parse. Owner: skip 0.6 E. This code is never emitted.",
        kind: ExplainKind::Skip,
    }),
    ("S060", ExplainEntry {
        title: "MATLAB function name used as a variable",
        body: "They refuse: `various`. Catching step: parse. Owner: skip 0.6 E. This code is never emitted.",
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
