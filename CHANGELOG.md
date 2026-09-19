# Changelog

## v0.5.2

Covers more of the checks the Dynare preprocessor performs. Codes `E219`–`E334` and `W201`–`W204`. No new command family.

Diagnostics implemented in this release:
- **Commands**: option walks on `estimation` (DSGE-VAR, the `datafile` gate, deprecated options), `sensitivity`, `identification`, `discretionary_policy`, `stoch_simul`, `prior_function` / `posterior_function`, and symbol lists on commands we already record.
- **Blocks we now parse**: `histval`, `estimated_params_init` / `estimated_params_bounds`, `osr_params_bounds`, `epilogue`, `change_type`, `trend_var` / `log_trend_var` / `var(deflator=…)`, `filter_initial_state`, `optim_weights` contents, `ramsey_constraints` body, `external_function`, `init2shocks`, `homotopy_setup`, `shock_groups`.
- **Names, types, macro**: MATLAB/Octave function name as a variable, external function as a bare `var`, macro type mismatches, `log(0)` and division by zero while building an expression, shock type checks (`var` / `stderr` / `cov` / `corr` / `skew`).
- **Duplicates and reuse**: an equation tag twice, an option declared twice, an empty option vector, several `varobs` statements, namespace-qualified misuse, and Warnings for `restriction_fname` and a symbol listed twice.
- **Reserved token**: `dsge_prior_weight` is refused wherever an expression is expected, while a declaration, `estimated_params`, or a model-local `#` may still name it.

Other changes:
- Trend declarations, epilogue helpers, and `external_function(name=…)` names join the duplicate-declaration pass (`E030` / `W031`).
- An unused `varexo_det` no longer reports `E021` (Dynare accepts it), and the `first_deriv_provided` / `second_deriv_provided` function names join the duplicate-declaration pass (`E030` / `W031`).
- `external_function` with the Jacobian and Hessian from the same non-top-level function is refused with Dynare's message (`E334`).

## v0.5.1

Until this release, some diagnostics arrived only by running a local Dynare preprocessor and overlaying its messages (`P###`), which is too late for typing. Some refuses appear only after Dynare rewrites the equations — those points do not land on the `.mod` you edit. 

This release make dygnosis a second preprocessor, which drop the official one and implements all diagnostics itself, except rewrite refuses we cannot map back to the original `.mod`. Dynare 7.1 remains the ground truth.

Diagnostics implemented in this release:
- **Written clash**: Errors Dynare refuses only after transform, when the file you edit is enough to decide: `shocks(surprise)` without `occbin_constraints` (`E178`), `occbin_constraints` with an incompatible command (`E179`), `varexo_det` clashes, two `planner_objective` statements with Ramsey, `shock_paths` mixed with `shocks` / `mshocks` / `endval` / controlled paths.
- **Check-class**: Errors Dynare refuses at check on syntax we already parse: empty model with a run command, perfect-foresight mixed with stochastic context, `model(linear)` nonsmooth ops, policy clashes, solver before setup, `initval`/`endval` order and `all_values_required`.
- Warning Dynare also emits: nonsmooth ops in a stochastic context (`W200`). Isolated `log` in `model(linear)` stays extra Warning (`W140`).

Other changes:
- `dygnosis explain --list` marks each code **shared**, **skipped**, or **added**.
- Include-cycle is extra Warning (`W062`), not Error.

## v0.5.0
- **Occasional constraints (OccBin)**
  - Parse `occbin_constraints` (name / bind / relax / error_*).
  - Keep every source bind/relax equation; count a named pair as one for the equation-count gap.
  - Store all equation tag keys (`bind`, `relax`, `mcp`, `static`, `dynamic`, …). Flag tags are empty strings.
  - Parse complementarity `⟂` / `_|_` on a model equation.
- **Check**
  - Structural OccBin Errors they refuse at check (duplicate blocks, more than two constraints, missing regime / bind / name, …).
  - Warning when an `mcp` tag is used instead of `⟂` (they warn and accept).
- **Catalog / MCP**
  - `dynare_list_options` knows `occbin_constraints`.
  - `dynare_equations` rows include `tags` and, when present, complementarity.

## v0.4.0
- **Expand view**
  - See the model after `@#if` / `@#for` / `@{…}` and includes (effective text).
  - Jump from each counted equation to the source that wrote it (origin).
- **MCP**
  - `dynare_expand`: effective text and origin for each counted equation.
  - `dynare_equations`: additive origin on each counted row.
- **Editor**
  - Command `dynare/showEffectiveModel`: source URI, effective text, and origins.

## v0.3.0
- **Companion files for one `.mod`**
  - Jump from the `.mod` to companions (for example `FILENAME_steadystate.m`, a `datafile=`, a helper `.m`, `run_FILENAME.m`).
  - MCP: `dynare_related_files` (includes and companions).
- **Check changes**
  - `W160` when a named data file or helper path does not resolve.
  - `I050` (no `initval` / `steady_state_model` block) is quiet when `FILENAME_steadystate.m` is present.

## v0.2.0
- **Fewer false Errors**
  - A `method_of_moments(...)` (and other catalog commands) option list was misread as missing semicolons (`E001`). That false Error stopped later checks on the same file, so real MoM / similar work could not start. Those lists are now one statement; only a true missing `;` still fires `E001`.
  - Declaring the same name twice in `var`, `varexo`, or `parameters` is a Warning (`W031`), matching the preprocessor. Declaring one name as two different types, or `#` twice, is still an Error (`E030`).
- **Equations as structured objects**
  - New MCP tool `dynare_equations`: each model equation with index, lhs, rhs, and timing at each use, plus the equation-count gap. For Ramsey / discretionary policy with N instruments, the gap check (`W013`) expects −N equations, not a square file.
  - `dynare_compare_models` diffs equations by index; it no longer lists shared equation text.
  - Editor outline groups endogenous names by timing (predetermined, forward-looking, mixed, static).
- **Check many files**
  - `dygnosis check` can take a directory of `*.mod` files (skips `+` folders). Exit code 1 only when there are Errors, not Warnings.

## v0.1.1

Audit of v0.1.0 against two principles:

- A check is built in instead of delegated to the Dynare preprocessor if (1) it is new; (2) it powers an editor feature; or (3) it helps while typing. The audit finds no check to drop.
- Where checks exist in both dygnosis and the preprocessor, the latter is ground truth. Severity must match (Error or Warning in both), and parity is pinned by honesty tests.

Other changes:

- A check that exists only in dygnosis can only be a Warning or Information. A Warning means the preprocessor would accept the file, but something looks wrong.
- MCP tools are simplified to nine.

## v0.1.0

This release of dygnosis is a Rust rewrite of the thin analysis core from [LLMacro-Dynare-LSP](https://github.com/pdwhoward/LLMacro-Dynare-LSP) (Python).

### Behaviour vs the Python origin

We aimed to keep the useful diagnostic **codes and messages**, not a line-for-line port of Python internals. Notable differences:

- **Native analysis** — identifiers and spans come from a real expression tree, not regex over masked text. Spans usually underline the bad token, not the whole equation or block.
- **Macros** — inactive `@#if` branches are dropped after expand; `@#for` unrolls fully (Python sometimes kept only the first list value or left dead text in slices).
- **Fewer false alarms** — e.g. call names are not treated as undeclared variables; `1/0` is non-finite (W122), not a “missing value” warning; parameter-range checks look at top-level assignments only, not values inside `steady_state_model`.
- **Where Python was wrong, Rust ships the fix** — e.g. I050 is “no `initval` / `steady_state_model` block” (no fake “compute steady state” solver path); some E001/auto-fix cases report only the missing `;`.
- **One wording across transports** — LSP and MCP no longer disagree on the same warning text where Python did.

### What we left out (numerical / Dynare compute)

These stay with Dynare, not this binary:

- Numerical steady-state solve and “compute steady state” / auto-solve on edit
- Blanchard–Kahn / eigenvalue checks
- Identification and static Jacobian singularity
- Per-equation numerical residuals
- Running a full Dynare/MATLAB session from the language server
- MCP/LSP surfaces that only wrapped those numerics
- Structural block partitioning that needed numeric graph tools (Dulmage–Mendelsohn)