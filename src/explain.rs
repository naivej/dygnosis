//! Diagnostic code documentation.
//!
//! Mechanical port of `python_dynare_lsp/explain.py` `_ENTRIES` for the 54
//! thin codes. `I050` and `W042` use the recorded surface rewrites in
//! `dev_logs/0.1/0.1.0/22-c-explain.md` (do not advertise Compute Steady State).

use std::collections::HashMap;
use std::sync::OnceLock;

/// Title and markdown body for one diagnostic code.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExplainEntry {
    pub title: &'static str,
    pub body: &'static str,
}

static ENTRIES: &[(&str, ExplainEntry)] = &[
    ("E001", ExplainEntry {
        title: "Parse error",
        body: "The Dynare parser could not interpret the source. The diagnostic range points at the offending token or the nearest recoverable position.\n\n**Common causes**\n\n- Missing semicolon at the end of a declaration or equation\n- Unbalanced parentheses, braces, or block keywords\n- Malformed time subscript such as `y(1)` where `y(+1)` was meant\n- A reserved keyword used as an identifier\n\n**Fix**\n\nInspect the line cited and the line immediately preceding it. Dynare's preprocessor frequently flags the *next* line after a missing semicolon.",
    }),
    ("W013", ExplainEntry {
        title: "Equation count does not match endogenous variable count",
        body: "The number of equations inside the `model` block does not equal the number of endogenous variables declared in the `var` block. This is an extra Warning: they accept a non-square file at check. The LSP catches this within milliseconds of editing, before Dynare is invoked.\n\n**Fix**\n\n- Add a missing equation, or remove a duplicate one\n- Declare the missing endogenous variable in `var`, or remove an   extra declaration\n- Check whether a commented-out equation was intended to be   active",
    }),
    ("E020", ExplainEntry {
        title: "Undeclared identifier in model block",
        body: "An identifier appears in the `model` block but is not declared as a `var`, `varexo`, or `parameters` symbol. The diagnostic names the exact identifier and the equation it appears in.\n\n**Fix**\n\n- Add the identifier to the appropriate declaration block\n- Correct a typo (the LSP suggests close matches when available)\n- If the symbol is a local helper, define it in the parameter   section before use",
    }),
    ("E023", ExplainEntry {
        title: "Predetermined variable not declared endogenous",
        body: "A name listed in `predetermined_variables` must also be declared as an endogenous variable in the `var` block. Dynare requires the variable to exist before it can be marked predetermined.\n\n**Fix**\n\n- Add the variable to the `var` declaration, or\n- Remove it from `predetermined_variables` if it is not actually   endogenous",
    }),
    ("E024", ExplainEntry {
        title: "Unsupported time subscript",
        body: "A deterministic exogenous variable is used with a lead or lag in a context where the LSP cannot safely interpret the dated value. Parameter leads/lags are accepted because Dynare treats them as fixed scalars.\n\n**Fix**\n\n- Remove the time subscript if the symbol is meant to be a fixed   scalar\n- If the dated quantity is state-dependent, model it as an   endogenous variable instead",
    }),
    ("E025", ExplainEntry {
        title: "Model-local variable shadows a declared symbol",
        body: "A model-local variable defined with `#` uses the same name as a declared `var`, `varexo`, or `parameters` symbol. That hides the declared steady-state value inside the model block and can make solver diagnostics misleading.\n\n**Fix**\n\n- Rename the model-local helper, for example `#y_local = ...`\n- Or remove the declaration if the name was intended to be only   a model-local helper",
    }),
    ("E030", ExplainEntry {
        title: "Duplicate declaration across blocks",
        body: "The same identifier is declared in more than one block (for example, in both `var` and `varexo`, or twice in `parameters`). Dynare requires each name to belong to exactly one symbol class.\n\n**Fix**\n\nRemove the duplicate declaration. If you intended two related but distinct symbols, rename one (the LSP's rename action propagates the change across the file).",
    }),
    ("W054", ExplainEntry {
        title: "Duplicate equation",
        body: "Two equations inside the `model` block are textually identical. The diagnostic cites the line where the duplicate appears and the line where the first occurrence was found. This is an extra Warning: they accept duplicate equations. A duplicate can also show up as our equation-count Warning `W013`.\n\n**Fix**\n\nRemove the duplicate.",
    }),
    ("W055", ExplainEntry {
        title: "Contradictory equation (always false)",
        body: "An equation reduces to a tautological falsehood, for example `0 = 1`. The LSP detects this by symbolic simplification of constant-only equations. This is an extra Warning: they accept `0 = 1` at check.\n\n**Fix**\n\nRemove the equation, or restore a variable reference that was accidentally simplified away.",
    }),
    ("W056", ExplainEntry {
        title: "Duplicate parameter assignment",
        body: "The same parameter is assigned a value more than once in the parameter section. This is an extra Warning: they accept; the last assignment wins.\n\n**Fix**\n\nRemove one of the assignments, or rename if two distinct parameters were intended.",
    }),
    ("W057", ExplainEntry {
        title: "Stray equation outside model block",
        body: "A line that looks like a model equation appears outside the `model` ... `end;` block. This is an extra Warning: they accept a stray top-level equation such as `0 = 1` at check.\n\n**Fix**\n\nMove the equation inside the `model` block, or convert it to a parameter assignment if it belongs at the top level.",
    }),
    ("E060", ExplainEntry {
        title: "Circular @#include detected",
        body: "Two or more files reach themselves through the chain of `@#include` directives. Dynare's macro preprocessor expands includes inline, so a cycle would either loop forever or — in the real preprocessor — be rejected with a hard error. The language server reports the cycle as a chain of file names: `a.mod -> b.mod -> a.mod`.\n\n**Common causes**\n\n- A submodel was refactored and now includes its parent\n- Two helper files cross-include each other for shared\n  parameters or steady-state definitions\n- A copy-paste mistake duplicated the include in the wrong\n  direction\n\n**Fix**\n\nBreak the cycle by removing one `@#include` along the chain. If both files genuinely need a shared block, extract that block into a third file and have both parents include it.",
    }),
    ("E061", ExplainEntry {
        title: "Cannot resolve @#include target",
        body: "An `@#include` directive names a file that the language server could not find. It looked in the directory of the including file first, then in each configured workspace search path, and ran out of candidates.\n\n**Common causes**\n\n- A typo in the filename\n- The included file lives in a directory that isn't on the   language server's search paths\n- The file was renamed or moved without updating the   directive\n\n**Fix**\n\nCorrect the filename, add the missing file, or extend the search paths so the directory containing the include is visible to the LSP.",
    }),
    ("E062", ExplainEntry {
        title: "Unmatched macro block",
        body: "A Dynare macro `@#if` block has no matching `@#endif`, or a `@#for` block has no matching `@#endfor`. Dynare's preprocessor expands these directives at build time and will reject unbalanced control flow.\n\n**Common causes**\n\n- A copy-paste deleted the closing directive\n- Mismatched closers — `@#endif` accidentally written for   a `@#for`, or vice versa\n- A nested block missing its inner closer\n\n**Fix**\n\nAdd the missing `@#endif` or `@#endfor` at the appropriate scope, or remove the stray closer. Each `@#if` needs its own `@#endif`; each `@#for` its own `@#endfor`.",
    }),
    ("E063", ExplainEntry {
        title: "Undefined macro interpolation",
        body: "An active line still contains an unresolved `@{NAME}` macro interpolation after the language server applied the simple `@#define` substitutions it can evaluate. Dynare's macro preprocessor cannot produce valid model code unless that macro name is defined in scope.\n\n**Fix**\n\nDefine the macro with `@#define NAME = value` before the line that uses it, correct the macro name, or remove the interpolation.",
    }),
    ("E064", ExplainEntry {
        title: "Macro error directive",
        body: "An active Dynare macro `@#error` directive was reached. Dynare's macro preprocessor stops when this directive is active.\n\n**Fix**\n\nRemove the `@#error` directive, or guard it behind a macro condition that is false for this model variant.",
    }),
    ("E065", ExplainEntry {
        title: "Invalid steady_state operand",
        body: "The `steady_state(...)` operator must refer to model endogenous or parameter expressions, not exogenous shocks. Exogenous variables are not valid operands for this Dynare operator.\n\n**Fix**\n\nRemove the exogenous variable from `steady_state(...)`, replace it with the intended endogenous or parameter expression, or rewrite the equation so the shock enters outside the operator.",
    }),
    ("E999", ExplainEntry {
        title: "Additional errors truncated",
        body: "More diagnostics were produced than the server displays at once. Fix the visible errors first; the next analysis pass will surface anything that was previously hidden.",
    }),
    ("I050", ExplainEntry {
        title: "No initval or steady_state_model block",
        body: "The file declares variables and equations but does not include an `initval` or `steady_state_model` block. This check is presence only; this tool does not compute a numerical steady state.\n\n**Fix**\n\nAdd an `initval` block with initial guesses, or a `steady_state_model` block with closed-form assignments. For a numerical solve, use Dynare (for example `steady;` in MATLAB/Octave).",
    }),
    ("P000", ExplainEntry {
        title: "Dynare preprocessor diagnostic",
        body: "A message emitted by the external Dynare preprocessor (``dynare-preprocessor`` binary) when it parses the file. Codes prefixed ``P`` come from the preprocessor, not from this language server's static analysis.  The message text and source location are passed through directly.\n\n**Common reasons**\n\n- A genuine syntax error the preprocessor caught\n- A semantic error involving ``check``, ``stoch_simul``,   or other runtime statements\n- The preprocessor binary timed out (``P000``: timeout)\n\n**Fix**\n\nRead the underlying message; the preprocessor's error reporting points at the source location.  For timeouts, consider whether the model is too large for the configured timeout or whether a non-terminating macro loop is present.",
    }),
    ("W010", ExplainEntry {
        title: "Parameter declared but never assigned",
        body: "A name appears in the `parameters` block but no assignment was found in the parameter section or in `steady_state_model`. At runtime, the parameter will be undefined and most computations will fail.\n\n**Fix**\n\nAssign a numerical value, or remove the declaration if the parameter is no longer used.",
    }),
    ("W011", ExplainEntry {
        title: "Parameter assignment cannot be evaluated",
        body: "An assignment like `phi = 1/(1-beta)` could not be evaluated because one or more right-hand-side symbols are not yet defined. The parameter falls back to undefined.\n\n**Fix**\n\nReorder the parameter section so that dependencies appear before dependents.",
    }),
    ("W012", ExplainEntry {
        title: "Undeclared helper variable in parameter section",
        body: "An identifier appears on the right-hand side of a parameter assignment but is not declared as a parameter or known helper variable.\n\n**Fix**\n\nAdd a declaration, or replace the helper with an explicit numeric value.",
    }),
    ("W020", ExplainEntry {
        title: "Endogenous variable never referenced in model",
        body: "An endogenous variable is declared in `var` but does not appear in any equation. Either remove the declaration or add the missing equation that uses the variable.",
    }),
    ("E021", ExplainEntry {
        title: "Exogenous variable never referenced in model",
        body: "A shock declared in `varexo` does not appear in any equation. They refuse: `unused_exo not used in model block`. The `nostrict` option bypasses that check.",
    }),
    ("W022", ExplainEntry {
        title: "Parameter declared but never referenced in model equations",
        body: "A parameter is declared and assigned but does not appear in any model equation. Often the result of stripping an equation but forgetting to remove the parameter.",
    }),
    ("W042", ExplainEntry {
        title: "Endogenous variable missing from steady_state_model",
        body: "The `steady_state_model` block does not assign a value for every endogenous variable. Dynare will fall back to the `initval` value (or zero), which usually produces an inconsistent steady state.\n\n**Fix**\n\nAdd the missing assignments. For a numerical solve, use Dynare (for example `steady;` in MATLAB/Octave).",
    }),
    ("E058", ExplainEntry {
        title: "Undeclared variable in initval",
        body: "An entry in the `initval` block refers to a name that is not declared as a variable. They refuse: `Unknown symbol: undeclared_zzz`.\n\n**Fix**\n\nDeclare the variable, or remove the stray `initval` entry.",
    }),
    ("W051", ExplainEntry {
        title: "Exogenous variable set in initval",
        body: "Setting an exogenous variable in `initval` has no effect on the steady-state computation. Shocks are zero at the deterministic steady state by construction.",
    }),
    ("W052", ExplainEntry {
        title: "Endogenous variable missing from initval",
        body: "The `initval` block does not provide an initial guess for every endogenous variable. The solver will start from zero for the missing entries, which may slow or prevent convergence on nonlinear models.",
    }),
    ("E059", ExplainEntry {
        title: "Name in initval/endval is neither endogenous or exogenous",
        body: "An `initval` or `endval` entry names a symbol that is not endogenous or exogenous (for example a parameter). They refuse: `… is neither endogenous or exogenous.`\n\n**Fix**\n\nAssign parameters before the model block, or inside `steady_state_model`. Use `initval` / `endval` only for endogenous or exogenous variables.",
    }),
    ("W060", ExplainEntry {
        title: "Exogenous variables declared but no shocks block",
        body: "One or more exogenous variables are declared in `varexo` but the file contains no `shocks` block specifying their variance-covariance structure. The model is then deterministic.\n\n**Fix**\n\nAdd a `shocks` block to define the shock processes, or remove the unused `varexo` declarations.",
    }),
    ("W061", ExplainEntry {
        title: "Ambiguous include parent context",
        body: "The active include file is reachable from more than one parent model, so the language server cannot safely infer which parent declarations and block context should apply.\n\n**Fix**\n\nOpen or run the intended parent `.mod` file, or provide only that parent and its include closure when calling workspace tools.",
    }),
    ("W070", ExplainEntry {
        title: "Parameter outside its conventional range",
        body: "A parameter assignment falls outside the theoretically admissible range for its standard interpretation. The conventional-range table is opinionated but conservative: it flags values that violate the *theoretical* admissible range under the parameter's conventional meaning, not values that simply look unusual.\n\n**Common causes**\n\n- Unit error: e.g. `beta = 99` when 0.99 was meant\n- Sign error on a quantity that must be non-negative   (variance, standard deviation, depreciation rate)\n- Gross-vs-net confusion on a rate parameter\n\n**Fix**\n\nCorrect the value, or — if the calibration is intentional — ignore the warning. This is a soft check, not a structural error: Dynare will accept any numeric value.",
    }),
    ("E090", ExplainEntry {
        title: "Observed variable is not a declared endogenous variable",
        body: "A name listed in ``varobs`` is not a declared endogenous variable. They refuse: `e is not endogenous.`\n\n**Fix**\n\nDeclare the variable in ``var``, or remove it from ``varobs`` if it was a typo or an exogenous/parameter name.",
    }),
    ("W091", ExplainEntry {
        title: "Duplicate observed variable",
        body: "A variable is listed more than once in ``varobs``. Each observed variable should appear exactly once.\n\n**Fix**\n\nRemove the duplicate entry.",
    }),
    ("W092", ExplainEntry {
        title: "Stochastic singularity",
        body: "There are more observed variables (``varobs``) than shocks (structural shocks plus measurement errors). The likelihood is then stochastically singular and estimation cannot proceed: the model cannot generate enough independent variation to match the observed series.\n\n**Fix**\n\nAdd structural shocks, add measurement errors on the observed variables (an ``stderr`` on an observed variable), or reduce the number of observed variables so that observables ≤ shocks.",
    }),
    ("E093", ExplainEntry {
        title: "estimated_params references an undeclared symbol",
        body: "An ``estimated_params`` entry names a symbol that is not declared with the expected role: a plain entry must name a parameter, an ``stderr`` entry must name a shock or observed variable, and a ``corr`` entry must name two declared shocks or variables. They refuse: `Unknown symbol: not_a_param`.\n\n**Fix**\n\nDeclare the symbol, or correct the name / entry type.",
    }),
    ("W094", ExplainEntry {
        title: "estimated_params bound or initial-value inconsistency",
        body: "An ``estimated_params`` entry has a lower bound that is not below its upper bound, or an initial value that lies outside the ``[lower, upper]`` interval. Dynare needs a non-empty bound interval containing the starting value.\n\n**Fix**\n\nOrder the bounds so that lower < upper and place the initial value inside them.",
    }),
    ("E095", ExplainEntry {
        title: "observation_trends variable not in varobs",
        body: "A variable given a trend in ``observation_trends`` is not listed in ``varobs``. They refuse: `variable y in observation_trends block is not an observed variable`.\n\n**Fix**\n\nAdd the variable to ``varobs`` or remove its trend specification.",
    }),
    ("E100", ExplainEntry {
        title: "Optimal-policy command requires a planner_objective",
        body: "``ramsey_model``, ``ramsey_policy``, and ``discretionary_policy`` optimise a planner's loss function. They refuse when ``planner_objective`` is missing: `A planner_objective statement must be used with a ramsey_model… and vice versa.`\n\n**Fix**\n\nAdd a ``planner_objective <expression>;`` statement before the policy command.",
    }),
    ("E101", ExplainEntry {
        title: "Policy instrument is not a declared endogenous variable",
        body: "An ``instruments=(...)`` entry names a symbol that is not a declared endogenous variable. They refuse: `Unknown symbol: not_endo`.\n\n**Fix**\n\nDeclare the instrument in ``var``, or correct the instrument name.",
    }),
    ("W102", ExplainEntry {
        title: "planner_discount is not a valid discount factor",
        body: "``planner_discount`` must be a discount factor in the interval (0, 1]. A value outside this range is almost certainly a mistake (for example, entering a discount rate instead of a factor).\n\n**Fix**\n\nSet ``planner_discount`` to a value such as 0.99.",
    }),
    ("E103", ExplainEntry {
        title: "osr is missing osr_params or optim_weights",
        body: "Optimal simple rules (``osr``) need an ``osr_params`` statement (the parameters to optimise). They refuse: `The osr statement requires the osr_params statement.` This check also flags a missing ``optim_weights`` block.\n\n**Fix**\n\nAdd the missing ``osr_params`` statement and/or ``optim_weights`` block.",
    }),
    ("W110", ExplainEntry {
        title: "Shock correlation outside [-1, 1]",
        body: "A ``corr`` entry in the shocks block sets a correlation whose magnitude exceeds one. A correlation coefficient must lie in [-1, 1], and the implied covariance matrix would not be positive semidefinite.\n\n**Fix**\n\nSet the correlation to a value in [-1, 1].",
    }),
    ("E111", ExplainEntry {
        title: "Shock variance or correlation specified more than once",
        body: "A shock's variance / standard error, or a correlation pair, is specified more than once in the shocks block. They refuse: `shocks: variance or stderr of shock on e declared twice`.\n\n**Fix**\n\nKeep a single specification per shock variance and per correlation pair.",
    }),
    ("W112", ExplainEntry {
        title: "Negative shock variance",
        body: "A shocks-block ``var e = ...`` entry sets a variance that folds to a negative constant. A variance is a squared quantity and should be non-negative. This is an extra Warning: they accept a negative ``var e`` at check.\n\n(The ``stderr`` form is not flagged: Dynare squares the standard error, so a negative ``stderr`` still yields a valid variance.)\n\n**Fix**\n\nUse a non-negative value. Recall the ``var`` form sets the variance, i.e. the standard error *squared* (e.g. ``var e = 0.01^2;``).",
    }),
    ("W120", ExplainEntry {
        title: "Stochastic command with no stochastic exogenous variable",
        body: "``stoch_simul`` / ``estimation`` drive the model with stochastic shocks, but the model declares no stochastic ``varexo``. ``varexo_det`` declarations are deterministic and do not count as stochastic shocks. This is an extra Warning: they accept ``stoch_simul`` / ``estimation`` with no stochastic ``varexo`` at check.\n\n**Fix**\n\nDeclare at least one stochastic exogenous variable (a dummy ``varexo`` plus a shocks-block entry is enough if the model is otherwise deterministic).",
    }),
    ("W121", ExplainEntry {
        title: "Parameter used with a lead or lag",
        body: "A declared parameter appears with a time subscript such as ``beta(+1)`` or ``rho(-1)`` in the model block. Parameters are time-invariant constants, so a lead/lag on one is meaningless and almost always means the symbol should have been declared as a variable, or that the time index is stray.\n\n**Fix**\n\nRemove the time index, or declare the symbol with ``var`` / ``varexo`` if it really is a variable.",
    }),
    ("W122", ExplainEntry {
        title: "Deep parameter assigned a non-finite value",
        body: "A parameter that is used in the model equations is assigned a non-finite value (``NaN`` or ``Inf``) while a run command (``steady``, ``stoch_simul``, ``perfect_foresight_*``, ``estimation``, ...) is present. This is an extra Warning: they accept ``Inf`` / ``NaN`` at check.\n\n**Fix**\n\nAssign a finite numeric value before the run command.",
    }),
    ("E130", ExplainEntry {
        title: "Variable used before assignment in steady_state_model",
        body: "The ``steady_state_model`` block is evaluated top to bottom as a sequence of assignments, so every variable on a right-hand side must already have been assigned above. A variable is referenced before its own assignment. They refuse: `variable 'n' is undefined in the declaration of variable 'log_n'`.\n\n**Fix**\n\nReorder the assignments so each variable is computed before it is used.",
    }),
    ("W131", ExplainEntry {
        title: "Variable silently overwritten in steady_state_model",
        body: "A variable is assigned more than once in the ``steady_state_model`` block and the later assignment does not use the earlier value, so the first assignment is dead. (An in-place transformation that reuses the value, such as the ``A = log(A)`` log-model idiom, is intentional and is not flagged.)\n\n**Fix**\n\nRemove the redundant assignment, or fold the two into one.",
    }),
    ("W140", ExplainEntry {
        title: "Nonlinear operator in a linear model",
        body: "The model is declared ``linear`` (``model(linear);``) but an equation applies a nonlinear operator to a variable. Examples include variable-dependent functions (such as ``log(y)`` or ``abs(e)``), products or ratios involving multiple variables (``c*k`` or ``a/y``), powers involving variables (``k^2``), and comparisons. This is an extra Warning: they accept a nonlinear operator in ``model(linear)`` at check.\n\n**Fix**\n\nRemove the ``linear`` option, or rewrite the equation without the nonlinear operator.",
    }),
    ("W150", ExplainEntry {
        title: "Deprecated command or option",
        body: "A deprecated command or option is used; current Dynare warns and may remove it in a future release. Commands: ``simul`` → ``perfect_foresight_setup`` + ``perfect_foresight_solver``; ``ramsey_policy`` → ``ramsey_model`` + ``stoch_simul``. Options: ``aim_solver`` → ``dr = aim``; ``bytecode`` (being removed).\n\n**Fix**\n\nSwitch to the modern command or option form.",
    }),
];

fn entry_map() -> &'static HashMap<&'static str, ExplainEntry> {
    static MAP: OnceLock<HashMap<&'static str, ExplainEntry>> = OnceLock::new();
    MAP.get_or_init(|| ENTRIES.iter().copied().collect())
}

fn is_preprocessor_code(key: &str) -> bool {
    let rest = match key.as_bytes() {
        [b'P', rest @ ..] => rest,
        _ => return false,
    };
    !rest.is_empty() && rest.iter().all(u8::is_ascii_digit)
}

/// Return `{title, body}` for a diagnostic code, or `None`.
///
/// Lookup is case-insensitive. `P` + digits (`P001`, `P123`, …) routes to
/// the `P000` preprocessor-passthrough entry.
pub fn explain(code: &str) -> Option<ExplainEntry> {
    let mut key = code.to_ascii_uppercase();
    if is_preprocessor_code(&key) {
        key = "P000".to_string();
    }
    entry_map().get(key.as_str()).copied()
}

/// Render the explanation as a single markdown string, or `None`.
///
/// Heading uses the caller's `code` string (`P001` keeps `### P001: …`).
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
