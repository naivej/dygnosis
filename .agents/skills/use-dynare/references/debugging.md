# Running Dynare and debugging

Read this when you run a model in Dynare, when Dynare or MATLAB reports an error, when results look
wrong without an error, or before you deliver a model (final checklist).

This file owns the "Run-and-fix loop". It also holds the final checklist, the error table (error, cause,
fix), the section on wrong numbers without an error, and the diagnostic commands.

Keep two kinds of evidence apart. Dygnosis (tools `dynare_*`, see `references/dygnosis-workflow.md`)
decides what the source alone decides: parse errors, declarations, written timing, equation counts, and
the refusals Dynare prints before MATLAB runs. Only a Dynare run decides steady state, Blanchard-Kahn
conditions, determinacy, IRFs, moments and estimation results. Never infer the second kind from the
first.

## Before you diagnose

Look for an existing fix first, in this order. If one matches, apply it; do not derive it again.

1. `references/known-issues.md`: specific problems with tested workarounds (read-only).
2. The error table below: syntax, equation count, Blanchard-Kahn, steady state, singular Jacobian,
   estimation, MATLAB-level errors.
3. The pitfalls section of the reference for the task, especially `references/heterogeneity.md`,
   `references/steady-state.md`, `references/perfect-foresight.md`, `references/occbin.md` and
   `references/estimation.md`.
4. The final checklist below. Many Blanchard-Kahn and singular-Jacobian failures are a failed item.

If nothing matches, find the cause yourself. Collect the static facts with Dygnosis (`dynare_diagnose`,
`dynare_model_info`, `dynare_equations`, `dynare_expand`), then run the Dynare diagnostic commands
(`resid;`, `check;`, `model_diagnostics;`, `model_info;`). Compare with an official example in
`<dynare-root>/examples/` or with `references/examples-code/`. Change one thing per round.

When you solve a problem that none of these sources covers, put the symptom, the cause and the fix in
the report to the user (SKILL.md "Report"). Do not edit `known-issues.md` or any other skill file.

## Run-and-fix loop

Use this loop for every Dynare run: from Stage 4 (steady state) on, and at Stage 3 (model block) if you
want Dynare's own equation count.

### 1. Static check first

Before the first numerical run, run `dynare_diagnose` on the root file. Pass its includes and companion
files (`dynare_related_files` finds them). Fix every Error: Dynare would refuse the file before MATLAB.
Read the Warnings and decide on each one. Information diagnostics are metadata notes (R1).
`dynare_auto_fix` applies the stored fixes for some codes. Details: `references/dygnosis-workflow.md`.

If Dygnosis is not connected, follow `references/dygnosis-setup.md`. If setup is blocked, say in the
report that the static check was not run, and continue.

### 2. Connect MATLAB and find Dynare (once per session)

Use the [MATLAB Agentic Toolkit](https://github.com/matlab/matlab-agentic-toolkit) to connect the
agent to MATLAB and to run MATLAB code.

| Item | How to find it |
|---|---|
| MATLAB session | MATLAB Agentic Toolkit (MCP tools such as `evaluate_matlab_code` and `run_matlab_file`, plus its MATLAB skills). If those tools are absent, point the user at the toolkit and mark numerical runs as not done. |
| `<dynare-root>`: the Dynare folder that contains `matlab/` and `examples/` | Ask the user, or read an existing `addpath` in the project (`run_*.m`, `startup.m`). Typical install locations: Windows `C:/dynare/<x.y>`, macOS `/Applications/Dynare/<x.y>`, Linux packages `/usr/lib/dynare` (manual, "Installation and configuration"). If several versions are installed, use the one the user or the project names; otherwise use the newest and say which. |
| Dynare version | After `addpath`, run `disp(dynare_version)`. Dygnosis checks against Dynare 7.2; report the version you ran. |
| Working folder | The folder of the `.mod` file. |

Dynare 7.2 supports MATLAB R2020a to R2026a. Do not guess numerical results when MATLAB is unavailable:
say which runs were not done and give the user the Dynare commands of steps 3 and 4.

### 3. Initialize

Run these in the connected MATLAB session:

```matlab
addpath('<dynare-root>/matlab');   % the matlab subfolder only, not its subfolders
cd('<folder of the .mod>');
```

### 4. Loop (round n = 1, at most 5 rounds)

1. **Run** `dynare <model> noclearall` (file name without `.mod`). Add `nograph` while you debug. Add
   `nointeractive` when the session must not wait for input.
2. **Read the output and branch:**
   - Preprocessor `ERROR: <model>.mod: line A, col B: …`: use the error table. Run `dynare_diagnose`
     again; most preprocessor refusals are Dygnosis Errors too.
   - `Blanchard & Kahn conditions are not satisfied: …`: check R2 (timing), signs and R4 first. Run
     `model_info;`.
   - Steady state with NaN, or large residuals: read `references/steady-state.md`. Run `resid;` to find
     the equation.
   - A MATLAB error inside Dynare's code: look in `references/known-issues.md`, then the error table.
   - Clean run: the stage passes. After Stage 5 (experiment), verify (step 5).
3. **Fix one thing per round.** Say what you changed and why. Record round n in the task list.
4. **Stop** after round 5, or when the same error appears in two rounds. Report the blocker, what you
   tried, and what you need (better initial values, the correct calibration target, a data file, the
   missing source). Do not retry without new information.

Options for that `dynare` call:

| Option | Use |
|---|---|
| `noclearall` | Keeps `M_` and `oo_` between runs. Default in this loop. |
| `nograph` | No graphs while debugging. |
| `nointeractive` | The session does not wait for input. |
| `nostrict` | Warn and continue when endogenous variables outnumber equations, an undeclared symbol is assigned in `initval` or `endval`, an undeclared symbol is in the model block, or a declared exogenous variable is unused. |
| `json=compute` | Write JSON under `<model>/model/json/`. In a file comment write `json=compute`, with no spaces around `=`. |
| `nodisplay` | Option of a computing command such as `stoch_simul(nodisplay)`, not of the `dynare` command. |

### 5. Verify (after Stage 5 runs clean)

Read the results and summarize them for the user:

```matlab
oo_.dr.eigval           % eigenvalues computed by check (Blanchard-Kahn conditions)
oo_.steady_state        % steady state in declaration order: no NaN
oo_.mean, oo_.var       % moments after stoch_simul (theoretical without the periods option)
oo_.irfs                % IRFs after stoch_simul: oo_.irfs.<var>_<shock>
oo_.endo_simul          % paths after perfect_foresight_solver
oo_.heterogeneity.dr.G  % sequence-space Jacobians after heterogeneity_solve
M_.endo_names           % order of the endogenous variables
```

Dynare also writes `M_`, `oo_` and `options_` to `<model>/Output/<model>_results.mat` (manual,
"Running Dynare", Output). Load that file when the workspace no longer holds the results.

You can run the diagnostic commands `resid;`, `model_diagnostics;`, `model_info;` and `check;` at any
time in the session. For slow solves and repeated plotting, see `references/matlab-workflow.md`.

## Final checklist

Apply it to each file (rule IDs: SKILL.md "Writing rules"). For each item, record the result, or name
it as not applicable. "Static" names the Dygnosis check; "Numerical" needs a Dynare run; "Review"
means no tool decides it.

1. **Equation count (R4).** Static: `dynare_equations` `count_gap`. A plain model needs `delta` 0.
   With `ramsey_model` or `discretionary_policy`, `delta` equals minus the number of policy
   instruments. Heterogeneous models: E192, W208. Codes: E188, W013.
2. **Timing (R2).** Static: `dynare_model_info` and the `idents` of `dynare_equations`. Under the
   default convention, a stock in `forward_looking` or `mixed` is a timing slip. Review: compare with
   the derivation note.
3. **Exogenous processes (R3).** Static: W211 (exogenous variable with a lead).
4. **Names (R5).** Review by reading. Dygnosis does not report these names.
5. **Parameters assigned before use (R7).** Static: W010 (never assigned), E130 (used before assignment
   in `steady_state_model`).
6. **Shocks block matches the experiment.** Stochastic: `stderr`, `var … = …`, `corr`. Perfect
   foresight: `periods` and `values`. Static: E205 (perfect foresight and stochastic commands in one
   file), W120 (stochastic command without a stochastic exogenous variable), W060 (IRF without a written
   shock size).
7. **Steady state.** Static (presence only): I050, W042, W052. Numerical: the steady-state pass in
   SKILL.md "Numerical runs".
8. **Nonsmooth functions under perturbation (R6).** Static: W200; in `model(linear)` E210 and E211.
9. **Statement syntax (R7).** Static: E001 (parse error), W012 (assignment to an undeclared name),
   W057 (equation outside the model block).
10. **Labels and comments (R1).** Static: I208 (missing equation tag), I209 (missing `long_name`).
    A non-ASCII identifier, equation, or shock statement is E001.
11. **Form (R8).** Static: W140 (nonlinear operator in a linear model).
12. **Experiment set,** one family: `stoch_simul`, `perfect_foresight_setup` with
    `perfect_foresight_solver`, `estimation`, `method_of_moments`, `ramsey_model`,
    `discretionary_policy`, `osr`, or the `heterogeneity_*` commands. Static: `dynare_list_options`
    lists the valid options of a command.
13. **Redundant equation by Walras' law removed** (check every model with several agents). If the model
    has a household budget constraint, goods market clearing and asset market clearing, one of them is a
    linear combination of the others. Mark it as redundant in section 4 of the derivation note and do
    not write it in the model block. Otherwise the count looks right but one independent equation is
    missing: the static Jacobian is singular and the Blanchard-Kahn conditions fail. Review; Dygnosis
    W054 finds only an exact duplicate equation. Numerical: `model_diagnostics;` lists the collinear
    equations.

## Check the equation count at Stage 3

At Stage 3 the file has declarations, parameter values and the model block, but no steady state and no
experiment yet.

- Dygnosis decides the written count without a run: `dynare_equations` `count_gap` (`delta` 0 means
  equal; `unreferenced_endogenous` lists declared variables that no equation uses). E188 means Dynare
  would refuse. W013 is guidance where Dynare's rewrite can change the count (for example an expression
  that makes Dynare add auxiliary variables, or equation surgery). E186 and W020 report an endogenous
  variable that no equation uses.
- Dynare's own refusal, printed before MATLAB: `ERROR: There are N equations but M endogenous
  variables!`. Dynare does not apply this test with `ramsey_model`, `ramsey_policy` or
  `discretionary_policy`.
- Dynare accepts a file with only declarations, parameter values and the model block. Do not add a
  placeholder `initval`. Dygnosis I050 (no `initval` or `steady_state_model`) is expected at this
  stage.
- When the count, syntax, names and timing are correct, go to Stage 4 (steady state).

## Error table (error, cause, fix)

"Static" names the Dygnosis code that reports the problem before a run. "Numerical" means only a Dynare
run shows it.

**Preprocessor `ERROR: <model>.mod: line A, col B: syntax error, unexpected …`** (Static: E001)
- A missing `;` at the end of the previous statement. The parser reports the next line (see "Reading
  preprocessor errors" below).
- A `//` comment inside `verbatim` or native MATLAB code. Dynare passes that text to MATLAB unchanged;
  use `%` there.
- Old Mac line endings (CR only). Dynare's macro processor ends a line only at LF. Convert to LF or
  CRLF.

**Preprocessor `character unrecognized by lexer`** (Static: E001)
- A non-ASCII character in an identifier, an equation or a shock statement (`var café;`). Rename with
  ASCII (R1).
- A double-quoted string in Dynare syntax. Use single quotes.
- A comment or a quoted string is not the cause. What Dynare accepts: "R1 details" in
  `references/workflow-detail.md`.

**`Unknown symbol: <name>`** (Static: E020)
- A name in the model block that is not declared, often a typo. Declare it or correct it.

**`ERROR: There are N equations but M endogenous variables!`** (Static: E188, W013)
- A missing or extra equation, or a missing or extra declaration. See "Check the equation count at
  Stage 3".

**`Namespace-qualified symbol pp.x not allowed in this context`** (Static: E275)
- A MATLAB struct field used as a value in the `.mod`. Fix: `references/known-issues.md`.

**`Blanchard & Kahn conditions are not satisfied: no stable equilibrium.` / `…: indeterminacy.` /
`…: indeterminacy due to rank failure.`** (Numerical)
- `check;` prints "There are N eigenvalue(s) larger than 1 in modulus for M forward-looking
  variable(s)". More explosive eigenvalues than forward-looking variables: no stable solution. Fewer:
  indeterminacy. Equal numbers but `The rank condition is NOT verified.`: rank failure.
- Common causes: a wrong sign or coefficient (for example a Taylor rule with `phi_pi < 1`); a timing
  error that makes a state variable forward-looking, or the reverse; a missing or extra equation.
  **Check timing first.** `dynare_model_info` lists the written timing classes; the Dynare command
  `model_info;` lists the state, forward-looking and static variables as Dynare classifies them.

**`Impossible to find the steady state (the sum of squared residuals of the static equations is …)`**
(Numerical)
- Numerical `initval`: improve the initial values; run `resid;` to see which equation has a large
  residual; try `homotopy_setup`; try another `solve_algo`.
- `steady_state_model`: an algebra error. Check each line by hand, or replace the block temporarily
  with `initval` guesses to find the inconsistent equation. Static part: E130 (use before assignment),
  W131 (variable assigned twice), W042 (endogenous variable missing from the block).
- A model with a true unit root: tag the equations `[static]`/`[dynamic]` and use `steady(nocheck)`
  (manual, "Steady state": `nocheck` option and `[static]` equations).

**Singular static Jacobian** (Numerical)
- Two equations are linearly dependent (for example the resource constraint written twice in two forms,
  or the redundant equation by Walras' law), a variable does not actually appear, or a functional form
  is wrong.
- Run `model_diagnostics;`. It reports endogenous variables missing at the current period, a steady
  state with NaN or Inf, and a singular static Jacobian with the collinear variables and equations
  (named by their `name` tags). Static part: W054 (exact duplicate equation), E186 and W020 (unused
  endogenous variable).

**Perfect foresight: the solver fails, or the stacked Jacobian is singular** (Numerical)
- After substitution, an equation keeps only leads or only lags, often when a Lagrange multiplier or a
  discount factor is written as a helper variable. Rewrite the equation so that it keeps a period-t
  term (`references/perfect-foresight.md`).
- The terminal condition is out of reach: increase `periods`, use the `endval_steady` option of
  `perfect_foresight_setup`, or use homotopy.
- A forward-looking pricing equation that also has `x(-1)`: `references/known-issues.md`.

**Estimation: stochastic singularity** (Numerical; Dygnosis W092 is only a static count)
- Dynare 7.2 stops with `initial_estimation_checks:: Estimation can't take place because there are
  less declared shocks than observed variables!`, or reports `Kalman filter: F is singular in
  stationary period. Stochastic singularity detected.`
- Cause: fewer shocks and measurement errors than observed variables, or observed variables tied by an
  exact identity. Fix: add shocks or measurement errors, or remove observed variables. W092 counts the
  names; it does not evaluate the covariance matrix.

**Estimation: the mode is not found, or the posterior looks wrong** (Numerical)
- Usually the steady state or the observation equations do not match the data: data mapped to the
  wrong model variable, means or trends inconsistent, `loglinear` and `logdata` not used together
  correctly. First simulate the model at the prior mean to check the setup. Static part: E090 (observed
  variable is not endogenous), E227 (no data file).

**MATLAB level: `Undefined function or variable …`, `Unrecognized function or variable …`,
`Error using …`**
- A variable or parameter name that clashes with a MATLAB function (`gamma`, `beta`), typically with a
  user-written steady-state file. Rename (`gam`, `betta`; R5). Dygnosis does not report this.
- Dynare is not on the path: `addpath('<dynare-root>/matlab')`.
- A misspelled name at top level became native MATLAB code (R7). Look at `+<model>/driver.m`. Static:
  W012.

**Specific crashes and traps seen in practice** (a lagged exogenous variable that crashes
`subst_auxvar` or `heterogeneity_solve`, `oo_.irf` instead of `oo_.irfs`, the toolbox function
`range`, the steady state of a pure exogenous variable, and others): `references/known-issues.md`.
This table keeps only general and structural errors.

## Wrong numbers without an error

A model can run clean and still give economically implausible numbers: a balanced-budget government
spending multiplier of about 0.01, IRFs off by a factor of about 100 or with the wrong sign, implausible
steady-state ratios. No tool reports these silent numerical errors. Compare with an analytical
benchmark before you trust a result.

- When a known analytical or limiting result exists (balanced-budget government spending multiplier
  about 1, transfer multiplier 0 under Ricardian equivalence, long-run money neutrality, plausible
  ranges of `C/Y`, `I/Y`, `K/Y`), print it with `fprintf` after the solve and compare.
- An error of an order of magnitude is almost always in post-processing: a normalization without the
  steady-state share `1/g_y`, a percentage IRF used as a ratio of levels, `oo_.irf` used instead of
  `oo_.irfs`, `Scale` divided by 100 once too often, the wrong sign or size of the shock.
- Where benchmarks come from, and why this nearly free step catches scaling and sign errors before you
  plot: `references/matlab-workflow.md`, "Check against analytical benchmarks".
- If timing or the steady state may be the cause (wrong IRFs without a crash), go back to items 2 and 3
  of the final checklist and to `references/steady-state.md`.

## Diagnostic commands

Static, with Dygnosis (no run):

- `dynare_diagnose`: diagnostics with codes.
- `dynare_model_info`: symbol lists, written timing classes, counts.
- `dynare_equations`: equations with tags, identifiers, source locations and `count_gap`.
- `dynare_expand`: the text after `@#include` and macro expansion.

Numerical, Dynare commands in the `.mod` or the session:

- `resid;`: static residuals at the current values (debug steady-state guesses).
- `check;`: eigenvalues and Blanchard-Kahn conditions.
- `model_diagnostics;`: sanity checks of the model and the steady state.
- `model_info;`: state, forward-looking and static variables, and the block structure. This is the
  Dynare command, not the Dygnosis tool `dynare_model_info`.
- `steady;`: prints the steady state. Ratio checks: "Wrong numbers without an error" above.

Report the results as SKILL.md "Report" describes. For a numerically solved steady state, say whether
the steady-state pass in SKILL.md "Numerical runs" held.

---

# Dynare 7.2 manual notes

Sources: `running-dynare.rst` ("Running Dynare", "Understanding Preprocessor Error Messages") and
`the-model-file.rst` ("Variable declarations", `model_info`) of the Dynare 7.2 manual.

## Preprocessor output: where to look

`dynare FILENAME` runs the preprocessor first. By default (without the `use_dll` option) it writes in
`+FILENAME/`:

- `driver.m`: declarations and computing tasks. A line that the parser does not recognize goes here
  unchanged as native MATLAB code. Misspelled variable or parameter names often show up here, so read
  `driver.m` first.
- `dynamic.m`: residuals and Jacobian of the dynamic equations; Dynare may add auxiliary variables and
  equations. The column order is in `M_.lead_lag_incidence`: rows are t-1, t, t+1; columns are the
  endogenous variables in declaration order; 0 means the variable does not appear in that period; a
  nonzero value is the column of that variable in the Jacobian.
- `static.m`: residuals and Jacobian of the static (steady-state) equations.

When an error at the simulation stage names an equation, locate it in these files. To rerun the
computing tasks without the preprocessor, type `FILENAME.driver`. To see the text after macro expansion
without a run, use Dygnosis `dynare_expand` (or the Dynare option `savemacro`).

## Block types of `model_info`

`model_info;` lists the state, forward-looking and purely static variables. With `block_dynamic` or
`block_static` it prints the block decomposition; `incidence` adds the incidence matrices and needs one
of these two options. Block types:

- `EVALUATE FORWARD` and `EVALUATE BACKWARD`: the block can be evaluated directly.
- `SOLVE FORWARD x` and `SOLVE BACKWARD x`.
- `SOLVE TWO BOUNDARIES x`: the block has both leads and lags.

`x` is `SIMPLE` for a block with one equation and `COMPLETE` for several. When the Blanchard-Kahn
conditions fail, use `model_info;` to check which variables Dynare treats as state and forward-looking
variables, and compare with what you intended.

## Reading preprocessor errors (line numbers)

Errors have the form `ERROR: file.mod: line A, col B: <message>` (also `cols B-C`, or
`line A, col B - line C, col D`). The most common misleading case is a missing semicolon:

```
varexo a, b           // the ; is missing here
parameters c, ...;
```

A statement can span several lines, so the parser finds the problem only when it reaches `parameters`
on line 2 and reports `line 2, cols 0-9: syntax error, unexpected PARAMETERS`. The fix is a `;` at the
end of **line 1**. Also remember: code that does not violate Dynare syntax but that the parser does not
recognize is passed to `driver.m` as native MATLAB code.

## Naming advice of the manual (R5)

Apply R5 (SKILL.md "Writing rules"). Checklist item 4 is the review. Dygnosis does not report these
names.
