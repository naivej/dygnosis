---
name: use-dynare
description: Dynare .mod files: write, review, replicate, edit, debug, and run experiments (stochastic simulation, higher-order perturbation, perfect foresight, estimation, method of moments, shock decomposition, forecasting, identification, optimal policy, OccBin, heterogeneity, Markov-switching), and produce publication figures. Use when the user works on a .mod or a DSGE, RBC, NK, or HANK model, or when the Dygnosis MCP tools are missing. Numerical runs go through the MATLAB Agentic Toolkit.
---
# Dynare models with Dygnosis

A `.mod` file is written in the language of the Dynare preprocessor, not in MATLAB. Two mistakes give
wrong results **without an error**: a wrong timing convention and an inconsistent steady state. Every
check in this skill aims at these two.

Keep two kinds of evidence apart:

- **Static evidence** comes from Dygnosis, an MCP server whose tools start with `dynare_`. It reads the
  source: parse errors, declarations, written timing, equation counts, includes, and the checks Dynare
  refuses before MATLAB runs.
- **Numerical evidence** comes from official Dynare under MATLAB: steady state, residuals,
  Blanchard-Kahn conditions, IRFs, moments, estimation. Connect and run MATLAB with the
  [MATLAB Agentic Toolkit](https://github.com/matlab/matlab-agentic-toolkit).

A clean Dygnosis result does not show that a steady state exists, that the model is determinate, or that
the economics is right. Never report one kind of evidence as the other.

Read the references for the selected task only. Do not load all of them.

## Workflow

1. **Choose the task** with "Route the task".
2. **Establish tool access** with "Tool access".
3. **Inspect or write**: "Existing model" or "New model".
4. **Check** with Dygnosis after each edit to a declaration, an equation, or a command.
5. **Run** the requested experiment in Dynare when the task needs numbers ("Numerical runs").
6. **Report** the evidence ("Report").

For multi-stage work, keep a visible task list (use the host's todo tool if it has one) and update it
after each stage.

## Tool access

- **Tools are callable:** read the runtime schemas of the `dynare_*` tools and use them. Do not reinstall.
- **Tools are configured but fail, or are absent:** follow `references/dygnosis-setup.md`. Setup ends
  only when a real model query succeeds.
- **Setup is blocked** (a trust prompt, a reload, no executable): give the user the remaining action,
  mark the Dygnosis checks as not run, and continue the work that does not need them. No command-line
  program replaces the MCP tools.

Which tool to use for each need, how to pass includes, and how to read the results:
`references/dygnosis-workflow.md`.

## Route the task

| Signal | Task | Dynare commands | Read |
|---|---|---|---|
| IRFs, moments, variance decomposition, "simulate this DSGE/RBC/NK model" | Stochastic simulation | `stoch_simul` | `references/stochastic-simulation.md` |
| Risk premia, asset pricing, uncertainty (volatility) shocks, precautionary saving, second- or third-order welfare, Epstein-Zin preferences, GIRF, stochastic steady state, `order=2`/`order=3`, pruning | Higher-order perturbation | `stoch_simul(order=2)` or `order=3` | `references/higher-order.md` |
| Transition paths, permanent shocks, deterministic simulation | Perfect foresight | `perfect_foresight_setup`, `perfect_foresight_solver` | `references/perfect-foresight.md` |
| Estimation, Bayesian, priors, MCMC, maximum likelihood, data, `varobs` | Estimation | `estimation` | `references/estimation.md`, `references/steady-state.md` |
| Method of moments, GMM, SMM, simulated moments, IRF matching | Method of moments | `method_of_moments` | `references/moments-method.md`, `references/estimation.md` |
| Shock decomposition, which shock drives a series, realtime decomposition, initial condition decomposition | Shock decomposition | `shock_decomposition`, `realtime_shock_decomposition`, `plot_shock_decomposition`, `initial_condition_decomposition` | `references/shock-decomposition.md` |
| Forecasts, conditional forecasts, a fixed future path for a variable, fan charts | Forecasting | `forecast`, `conditional_forecast`, `conditional_forecast_paths` | `references/forecasting.md` |
| Identification, sensitivity analysis, GSA, prior calibration to IRFs or moments | Identification and sensitivity | `identification`, `sensitivity` | `references/identification.md` |
| Regime switching, Markov switching, structural BVAR, SBVAR, time-varying volatility or coefficients | MS-SBVAR | `markov_switching`, `svar`, `sbvar`, `ms_*` | `references/ms-sbvar.md` |
| Heterogeneous agents, HANK, Krusell-Smith, distributions of households, sequence-space Jacobian | Heterogeneity | `heterogeneity_dimension`, `heterogeneity_*` | `references/heterogeneity.md` |
| Optimal policy, Ramsey, discretion, welfare, simple rules | Optimal policy | `ramsey_model`, `discretionary_policy`, `osr` | `references/optimal-policy.md` |
| Zero lower bound, collateral or borrowing constraints, irreversible investment | Occasionally binding constraints | `occbin_setup`, `occbin_solver`, or perfect foresight with `lmmcp` and `⟂` | `references/occbin.md`; one deterministic constraint: `references/perfect-foresight.md` |
| Almost every model | Steady state | `steady_state_model`, `initval`, `steady` | `references/steady-state.md` |
| Multi-country or multi-sector models, model variants, `@#`, equations generated in loops | Macro processor | `@#define`, `@#if`, `@#for`, `@#include` | `references/macro-processor.md` |
| Errors, a model that does not run, Blanchard-Kahn conditions not met, no steady state, results that look wrong without an error | Debugging | diagnostic commands | `references/debugging.md` |
| A problem that someone already solved | Known problems (read-only) | — | `references/known-issues.md` |
| Change or extend an existing `.mod` | Existing model | — | "Existing model" below |
| IRFs or simulated paths in the deliverable, publication figures, PDF export, scenario comparisons | Publication plots | `plot_irfs_pub.m`, `plot_series_pub.m` | `references/publication-plots.md` |
| Replicate a paper, "a model with feature X", an implementation may already exist | Catalog lookup | — | `references/catalog-lookup.md` |
| A skeleton to start from | Templates | — | `references/templates.md` |
| Details of a stage in "New model" | Stage details | — | `references/workflow-detail.md` |
| Slow solves, repeated plotting or normalization, cached `oo_`, comparing models or scenarios | MATLAB-side workflow | — | `references/matlab-workflow.md` |
| A script that reruns the whole deliverable | Run script | `run_<model>.m` | `references/run-script.md` |
| Structure and formula style of the derivation note | Derivation style | — | `references/derivation-style.md` |
| Optimization problems and first-order conditions of each agent, structural variants | Modeling blocks | — | `references/modeling-blocks.md` |
| Dygnosis tools are missing or fail | Dygnosis setup | — | `references/dygnosis-setup.md` |
| Which Dygnosis tool to use and how | Dygnosis workflow | — | `references/dygnosis-workflow.md` |

## Sources

Three local sources answer different questions. Use them together. Details:
`references/catalog-lookup.md`.

| Question | Source | Take | Do not copy |
|---|---|---|---|
| How to model the economics: first-order conditions, mechanisms, calibration, timing | Model reference library `references/catalog.csv` (MMB replications in `references/examples/<ModelID>.mod`) | Equation logic, parameter values, timing | Code form, especially of linearized versions |
| How to write a Dynare block or command | Programming library `references/catalog-code.csv` (Pfeifer DSGE_mod and the `Dynare_Course/` chapters in `references/examples-code/<Folder>/<CodeID>.mod`) | Options, block structure, interfaces | Equations and calibration |
| Details of a built-in command in the installed version | Official examples in `<dynare-root>/examples/` | Syntax, version compatibility | Equations and calibration |

If neither library matches, search `references/model-archive-catalog.csv` (column `Status`: `runnable`
models have a `.mod`; `derivation-only (needs_review)` entries have only a derivation note, which is an
unverified first pass). Then search the web for the paper. Check that a file exists before you read it.

## Existing model

Use this path when the user has a `.mod` file and wants a change, an extension, a review or a fix.

1. **Baseline.** Read the file. Find its includes and companion files with `dynare_related_files`, and
   pass them in the `files` map. Run `dynare_diagnose` and `dynare_model_info`. Record the timing
   convention, the counts and the diagnostics that exist before your change.
2. **Locate.** Find the equations with `dynare_equations` (prefer the `name` filter) or, for
   macro-generated text, `dynare_expand`. Edit the written source at the returned location.
3. **Classify the change.**
   - Modification (parameters, equations, calibration): edit directly.
   - Extension (a mechanism, a shock): keep R4. If the extension changes the structure (for example a
     representative household becomes heterogeneous), derive the new first-order conditions first.
     The full derivation note is not necessary.
   - A new experiment (estimation, Ramsey, OccBin, …): read the matching reference and add the commands
     to the existing model.
4. **Edit in small steps.** Rename symbols with `dynare_find_references` and `dynare_rename`. After each
   change, run `dynare_diagnose` again. Use `dynare_compare_models` to review the structural effect.
5. **Steady state.** If the change affects the steady state (a new utility function, new variables),
   update it with `references/steady-state.md` and run `steady; resid; check;` in Dynare. The pass is
   the steady-state pass in "Numerical runs".
6. **Self-check** with the final checklist in `references/debugging.md`.
7. **Report** ("Report"). Separate problems that existed before from problems that your change caused.

## New model

Use this path for a new model, a replication, or a substantial extension. Details of each step:
`references/workflow-detail.md`.

Build in increments. Do not write the whole file and then run it. Check each stage before you write the
next one.

- **Sources.** Look up the local libraries first ("Sources"). Tell the user which close models you found.
- **Decisions.** Structural choices change the equations: capital or no capital, labor supply form,
  market structure and price rigidity, household heterogeneity, fiscal and financial blocks, the shocks.
  If the user named a self-contained paper, gave the full equations, or settled every choice, continue.
  Otherwise ask once, with a recommendation for each open choice. Do not ask about form (R1, R5, R8,
  helper variables, tags); apply the rules.
- **Missing source.** A paper can cite another paper for a whole block ("the financial sector follows
  BGG (1999)") without the equations. Do not rebuild that block from memory. Tell the user which block
  comes from which source, and name any local candidate. Ask the user to upload the source, choose a
  local version, or authorize a reconstruction. Without an answer: leave a core mechanism block as a
  marked stub; reconstruct a peripheral standard block only with the label
  "reconstructed, not checked against the source".
- **Stage 1 (derivation note).** Write `<model>_derivation.md` with the eight sections of
  `references/derivation-style.md`. This is the default for replications, non-standard mechanisms and
  user equations that need a consistency check. Skip it for a textbook RBC or three-equation NK model
  (say in one sentence where the steady state comes from), and for small edits. If the user says to
  skip it, skip it and state the risk once. Two checks are mandatory: the redundant equation by Walras'
  law (section 4) and an equation for each variable (section 8). Share the note and continue, unless
  the user wants to review it first or a decision is still open.
- **Stage 2 (declarations).** Write the file header and `var`, `varexo`, `parameters` with metadata
  (R1, R3, R5).
- **Stage 3 (model block).** Translate the derivation note equation by equation, with `[name='…']` tags,
  and assign the parameters. Check: `dynare_diagnose` shows no Error; `dynare_equations` `count_gap` is
  zero (R4); `dynare_model_info` shows the intended timing (R2).
- **Stage 4 (steady state).** Write `steady_state_model` from section 6 of the derivation note (use
  `initval` guesses only without a closed form), then `steady; resid; check;`. Numerical pass: the
  steady-state pass in "Numerical runs".
- **Stage 5 (experiment).** Write `shocks` and the experiment command, for example
  `stoch_simul(order=1, irf=20, nograph);`. Numerical pass: Blanchard-Kahn conditions satisfied, IRFs
  finite.
- **Stage 5b (plots).** When the deliverable has IRFs or simulated paths, produce figures with
  `references/publication-plots.md`. Skip when there is nothing to plot or the user declines.
- **Stage 5c (run script).** For a new model or replication with a numerical experiment, write
  `run_<model>.m` with `references/run-script.md`. Skip when the user declines.
- **Final checks.** Apply the final checklist in `references/debugging.md`.

## Writing rules

- **R1 Labels and comments.** Write comments, derivation notes and messages in the user's language.
  Keep identifiers, `long_name` values, equation tags and TeX names in English ASCII. What Dynare
  accepts is in "R1 details" (`references/workflow-detail.md`). For a new model or a substantial
  extension, tag every equation and give every declaration a TeX name and `long_name`:
  `var c $C$ (long_name='Consumption');` and `[name='euler'] …`. Dygnosis Information I208 and I209
  find missing tags and missing `long_name`. The conventions of an existing file, small edits and
  explicit user preferences come first. Missing metadata is not a Dynare error.
- **R2 Timing.** The timing of a variable reflects when it is decided. Default "stock at the end of the
  period" convention: `y = k(-1)^alppha*…;` and `k = invest + (1-delta)*k(-1);`. Alternative:
  `predetermined_variables k;` with `y = k^alppha*…;` and `k(+1) = invest + (1-delta)*k;`. Use one
  convention per file. Check the result with `dynare_model_info` (`predetermined`, `forward_looking`) and
  the `idents` of `dynare_equations`. How to read `timing` and `dynare_timing`:
  `references/dygnosis-workflow.md`. Longer notes: "R2 details" in `references/workflow-detail.md`.
- **R3 Exogenous processes.** For stochastic commands (`stoch_simul`, `estimation`, …), declare the
  innovations in `varexo` and write persistent processes as endogenous variables:
  `var z; varexo eps_z; z = rhoz*z(-1) + eps_z;`. In perfect foresight an exogenous variable can carry
  the path itself (`shocks` with `periods` and `values`, or `endval`). For `varexo(heterogeneity=…)`
  follow `references/heterogeneity.md`. Dygnosis W211 reports an exogenous variable with a lead.
- **R4 Equation count.** A plain model has as many equations as endogenous variables. With
  `ramsey_model` or `discretionary_policy` the model block holds the private-sector equilibrium
  conditions: one equation fewer than endogenous variables for each policy instrument. Heterogeneous
  models count each heterogeneity dimension separately. Check with `dynare_equations` `count_gap`;
  Dygnosis reports a mismatch as E188 or W013 (heterogeneous: E192, W208). These counts describe the
  written model, before Dynare adds its `AUX_*` auxiliary variables.
- **R5 Names.** Follow the manual: no symbol named after a Dynare command or built-in function (case is
  ignored; for example `Ln`, `shocks`); no `i` and no `inv` (write investment as `invest`); with a
  user-written steady-state file, no MATLAB function names such as `alpha`, `beta`, `gamma` (write
  `alppha`, `betta`, `gam`). House style also avoids `e` and `E`. The 7.2 preprocessor accepts these
  names and Dygnosis does not report them, so a clean check does not clear them.
- **R6 Nonsmooth functions.** Under perturbation (`stoch_simul`, `estimation`, …) keep `max`, `min`,
  `abs`, `sign` and comparison operators off endogenous variables: the derivatives at the kink are
  wrong. Use OccBin, or perfect foresight with `lmmcp` and a complementarity condition (`⟂`, ASCII
  `_|_`; the form is in `references/perfect-foresight.md`). The older `mcp` equation tag is obsolete
  (Dygnosis W170). Dygnosis reports W200, and E210/E211 in `model(linear)`.
- **R7 Statement syntax.** End each statement with `;` and each block with `end;`. Write one statement
  per line. Assign each parameter before it is used. The preprocessor passes an unrecognized top-level
  line to MATLAB unchanged, so a typo can become MATLAB code. Dygnosis reports parse errors (E001),
  unassigned parameters (W010), assignments to undeclared names (W012) and use before assignment in
  `steady_state_model` (E130). `dynare_auto_fix` applies the stored fixes.
- **R8 Nonlinear by default.** Write the original nonlinear equations (first-order conditions,
  constraints, exogenous processes) and let Dynare approximate them. Do not linearize by hand. Use
  `model(linear);` only when the user asks for a linear model or the source gives only a linearized
  system. `discretionary_policy` needs a quadratic objective and either a linear model or a first-order
  solution with an analytical steady state.

## Numerical runs

Numerical evidence comes from official Dynare under MATLAB.

1. Connect to MATLAB with the [MATLAB Agentic Toolkit](https://github.com/matlab/matlab-agentic-toolkit)
   (MATLAB MCP tools and MATLAB skills).
2. Put Dynare on the MATLAB path and work in the model folder (`references/debugging.md`).
3. Fix the Dygnosis Errors before the first run.
4. Follow the "Run-and-fix loop" in `references/debugging.md`: at most five rounds; stop and report when
   the same error appears twice.
5. On an error, look in `references/known-issues.md` and the error table of `references/debugging.md`
   before you diagnose it yourself.
6. If MATLAB is unavailable, say what was not run and give the user the Dynare commands to run.

**Steady-state pass.** `resid(non_zero);` prints `All residuals are zero`, and `check;` prints
`The order and rank conditions are verified.`

## Report

Account for every item below. Write the item, or name it as not applicable.

1. The delivered files: `.mod`, derivation note, steady-state file, run script, figures.
2. The model and the experiment in one or two sentences, and the timing convention of stock variables.
3. Static evidence: the Dygnosis tools you ran, their inputs (root file, includes), the codes reported,
   and the completeness of the expansion. Name what was not checked.
4. Numerical evidence: Dynare version, commands and outcome (steady-state residuals, rank condition,
   key numbers), or the execution limit and the commands for the user. For a numerical steady state,
   say whether the steady-state pass in "Numerical runs" held.
5. Sources: the paper, appendix or reference implementation you used, as in the file header. Mark each
   reconstructed block as "reconstructed, not checked against the source", and name each missing source.
6. Each problem you solved that `references/known-issues.md` does not cover: symptom, cause, fix.
7. Cleanup: which generated files you removed and which you kept (lists in
   `references/workflow-detail.md`). Never delete user files.

Do not write into the installed skill. Save a model to an archive only when the user asks, in a location
the user chooses (`references/model-archive.md` describes a layout).

## Credits

This skill builds on [EconSolider/dynare-copilot](https://github.com/EconSolider/dynare-copilot)
(MIT License). See `LICENSE`.
