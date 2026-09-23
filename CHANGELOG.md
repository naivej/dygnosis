# Changelog

## v0.7.0

- Reads the written shock, path, learning-date, and database forms accepted by Dynare 7.2 and reports their parse, check, and written transform refusals (`E393`–`E425`). Existing `E343`/`E344` now reach deterministic shocks; `E111` catches covariance followed by correlation and skew/co-skew duplicates have their own codes.
- Subsample declarations, copies, and named `prior` / `options` uses now report missing or repeated ranges (`E427`–`E430`) and invalid final target types at Write (`E431`). A model use after `var_remove` reports `E426`; command lists recognize removed and model-local names through `E240`. Written denominator cancellation reaches `E278`, and malformed DATE and handed-over command forms reach `E001`. Mixed-type `corr(A,B).options` uses `E379`.
- W060 names missing selected shocks from `irf_shocks`, or warns for an unselected positive `irf` only when no plain exogenous shock has a written size. It stays quiet for bare declarations, bare `stoch_simul`, explicit zero size, an estimated standard error for the plain exogenous shock, a possible external size, and uncertain macro or include input. An observed-variable measurement-error estimate alone does not suppress it. Warnings in a file with resolved includes point to the active file.
- Compare adds `shock_setup_changes` and a **Shock setup changes** markdown section for stochastic, scheduled, surprise, multiplicative, heteroskedastic, controlled, and terminal instructions. It shows written values, timing, learning dates, controls, and overwrite status, with source locations when the input text can be verified against the parsed file. It does not infer realized paths or solver results.
- Compare also shows changed initial and terminal values, time settings, database references, and heteroskedastic data settings beside affected shocks; `shock_groups` and `init2shocks` changes have a separate **Shock analysis setup** section.

## v0.6.1

Pins language checks and optional honesty tests to Dynare 7.2.

- **E243** uses 7.2's `histval: y(0) declared twice` sentence.
- `histval_file` no longer offers `nobs` or `last_simulation_period`; either option reports Dynare's syntax error on its name. `initval_file` still accepts both.
- Duplicate checks for shocks, histval, generated IRFs, filter initial state, observation trends, optimization weights, and estimated parameters compare entries within one block. Repeating an entry in a later block stays quiet. **E248** now checks a value reference only against parameters declared in the same block. Existing same-block refusals remain.

## v0.6.0

Reads method of moments, matched moments and IRFs, and the IRF and moment calibration blocks, and reports the refusals Dynare makes on them. Codes `E382`–`E392`.

Diagnostics implemented in this release:
- **E382**–**E385** — `method_of_moments` needs `mom_method` (`GMM`, `SMM`, or `IRF_MATCHING`); GMM and SMM need `datafile`; `analytic_standard_errors` and `analytic_jacobian` need GMM; only one of the HP, one-sided HP, and bandpass filters.
- **E386** — a `matched_moments` row is not a product of endogenous variables: `Matched moment expression has incorrect format`.
- **E387**–**E392** — a matched-IRF shock that is not exogenous, a repeated endogenous/exogenous pair or tuple, `periods` and `values` (or `weights`) of different lengths, and a date written in `periods`.
- **E058** and **E317** reach the name slots of `matched_irfs`, `matched_irfs_weights`, `irf_calibration`, and `moment_calibration`.
- **E239** and **E240** reach the trailing name lists of `forecast`, `rplot`, `dynasave`, `dynatype`, and the shock-decomposition commands. `rplot` allows an endogenous or an exogenous.

Other changes:
- `dynare_list_options` knows `matched_moments`, `matched_irfs`, `matched_irfs_weights`, and `moment_calibration`.
- A variable may be named after a block opener (`shocks`, `matched_irfs`) or after a command whose only lexer rule is at the start of a statement (`steady`, `dynatype`). `forecast`, `identification`, `simul`, `stoch_simul`, and `varobs` are still not names.
- `model = 0.2;` and `steady = 0.9;` at the start of a statement are syntax errors, as they are in Dynare. Inside a block, `end` is the block closer, not a name.

## v0.5.5

Adds the refusals the Dynare preprocessor prints only at its last stage, when it writes the MATLAB files — after its check and transform stages have already accepted the file. The file you edit is enough to decide each of them. Codes `E380`, `E381` and `W205`.

Diagnostics implemented in this release:
- **E380** — a `load_params_and_steady_state` file names something the loader cannot take: an `epilogue` helper, an `external_function` name (the `name=` value or a value named by `first_deriv_provided` / `second_deriv_provided`), or a trend variable. The four slots it accepts are parameter, endogenous, `varexo` and `varexo_det`. Dynare refuses only when writing: `Unsupported variable type for A in load_params_and_steady_state`.
- **E381** — a `steady_state(…)` expression calls an `external_function`: `The expression inside a steady_state operator cannot contain external functions`. The operand walk descends, so a call nested under an operator is caught too.
- **W205** — two rows of the same `shock_groups` block reuse a label: `shock group label 'g1' has been reused. Only using the last definition.` The comparison is within one block, as Dynare's is: two separate `shock_groups` blocks may share a label silently.

Other changes:
- **`W204` narrows**: the unsupported kinds above now error `E380` instead, and `W204` keeps the genuinely unknown name. All three kinds are positional, as Dynare's own reading is — a name declared only *after* the `load_params_and_steady_state` statement is still unknown to the loader, warns `Unknown symbol`, and stays `W204`.
- **Writer-stage honesty**: the test harness can now spawn a run that reaches the MATLAB writer (bare `nopreprocessoroutput`, without `onlyjson`, which is what skips the writer), and cleans the `+<name>/` package directory such a run leaves beside the `.mod`.
- The two writer-stage messages that no `.mod` shape can be matched against — the more-than-32-nested-parentheses warning (whose trigger is Dynare's generated text) and the excluded-name-still-in-`initval` refusal (which aborts Dynare with no message) — are documented as deliberately silent (`W187`, `E191`).

## v0.5.4

Adds the MS-SBVAR family: the `ms_*` commands, `sbvar`, `svar`, `markov_switching`, the `svar_identification` and `conditional_forecast_paths` blocks, `conditional_forecast`, `plot_conditional_forecast`, the `data` statement and the dotted `prior` statement are read instead of skipped, and the refuses Dynare makes on them are reported. Codes `E338`–`E379`.

Diagnostics implemented in this release:
- **The family's own refusals** (one code per distinct Dynare message): the `data` statement's file-or-series rule, its both-at-once rule and its `nobs` bound (`E338`–`E340`); `ms_estimation`'s `datafile` / `initial_year` gate (`E341`); `conditional_forecast`'s `parameter_set` (`E342`), and `conditional_forecast_paths`' mismatched period and value counts and its repeated `var` name (`E343`, `E344`); `markov_switching`'s required options, its chain and regime-count values, its chain order, its `parameters` types, its `restrictions` row shape, its regime bound, its repeated regime pair, its transition probabilities and their sums (`E345`–`E355`); `svar_identification`'s one-block and one-cholesky rules, its repeated lag, its repeated equation, its equation-number bound, its repeated name and its Qi-or-Ri restriction (`E356`–`E362`); `svar`'s choice of one of `coefficients` / `variances` / `constants` with its chain and equation values (`E363`–`E367`); the `ms_*` commands' mutually exclusive regime and filtered-probability options (`E368`–`E371`); and the `prior` statement's `shape`, `mean` / `mode`, `stdev` / `variance` and `domain` rules, the joint head's name count, a head that is not a parameter, and a `corr` head whose two names differ in type (`E372`–`E379`).
- **`E227` reads the `data` statement in file order**: `estimation; data(file='x.csv');` is refused, as Dynare refuses it, while a `data` statement written before the `estimation` silences the refusal on both sides.
- **The parsed family reaches the shared name checks**: an undeclared name in the `svar_identification` body, in a `conditional_forecast_paths` `var` row or in a `prior` head reports `E058`; a `var` row naming an exogenous reports `E317`; a `prior` head naming a parameter reports `E059`; and the trailing name lists of `ms_irf` and `plot_conditional_forecast` report `E239` / `E240` with Dynare's own text.
- **Malformed family shapes** are `E001`, pointed at the token Dynare's parser stops on: a missing or empty option list, an option name the command does not take, a value written in a shape the grammar has no production for, an empty or malformed block body, and the statements `dsample(10, 10);`, `rplot(periods=10);`, `smoother2histval(periods=10);`, `var_remove(alpha);`, `y(1) = 2;` and the dotted heads whose body is not `prior`, `options` or `subsamples`.
- **`E378` also fires on a top-level assignment** (`y = 3;`), where Dynare's `y is not a parameter` is the same sentence its `prior` head prints.

Other changes:
- The `data` statement is parsed: it replaces the presence-only record 0.5.2 kept for the `datafile` gate.
- A line the Dynare lexer reads as native MATLAB text is no longer reported as a parameter assignment with a missing semicolon; `aaaa = 1` followed by `bbbb = 2;` is accepted here, as Dynare accepts it.
- `sbvar`, `svar_identification`, `svar_global_identification_check`, `conditional_forecast_paths`, `plot_conditional_forecast` and the dotted `prior` are in `dynare_list_options`, in option hover and completion, and in command-name completion.

Three shapes stay silent on purpose, because Dynare 7.1 prints no message for them: it aborts on `markov_switching(…, parameters=[<undeclared>])`, on a `restriction` whose expression is not a `coeff(…)` term, and on a `duration` written as a vector.

## v0.5.3

Adds the equation surgery family: the `model_remove(TAGS);` statement and the `model_replace(TAGS); … end;` block now change the model before the checks run, and the refuses they can cause are reported. Codes `E335`–`E337`.

Diagnostics implemented in this release:
- **Equation surgery**: a tag set that matches no equation of the model (`E335`), an excluded equation whose left side is not one endogenous variable and which carries no `endogenous` tag (`E336`, `model_remove` only), and the same endogenous excluded twice by one statement (`E337`). Each refusal points at the equation or the statement in the file you edit.
- **`E256`** (a tag key used twice) also fires on the tag list of a surgery statement, where the preprocessor refuses the list while parsing it.
- **Double-quoted strings**: a tag, a `shock_groups` name, a `bvar_*` string, or an option value written with double quotes is now `E001` — the preprocessor refuses `"…"` with `character unrecognized by lexer` in every one of those positions. Macro directives keep their double quotes, and single quotes are unaffected.
- **Names read as of the statement that names them**: a symbol a later `model_remove` takes out of the model was still endogenous when an earlier statement used it, so `optim_weights` (`E317`) and `ramsey_constraints` (`E321`) written before the removal no longer report a false Error whichever way the symbol left, and `planner_objective` (`E251`) no longer does when the symbol is dropped. A statement written after the removal refuses the name, as before. A constraint's bound is read as of its own row, so a bound a removal re-typed still refuses (`E320`), with the preprocessor's own sentence. The same rule keeps `initval` / `endval` / `histval` / `varobs` entries for a symbol a removal drops from reading as undeclared, `filter_initial_state` reports the timing refusal (`E314`) instead of the undeclared one, and a removed equation's body joins the undeclared-name walk (`E020`).

Other changes:
- The equation list, the counts, and the `[static]` / `[dynamic]` check (`E208`) are the post-removal model, the way 7.1 sees it.
- `model_remove` and `model_replace` are in `dynare_list_options` and in command-name completion.

`exclude_eqs` / `include_eqs` are not covered: they are preprocessor invocation options rather than `.mod` syntax, and the editor never sees them.

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
