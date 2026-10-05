# New model: step details

Read this when you run SKILL.md "New model" and need the detail of one step. This file expands
SKILL.md; it does not repeat its summary.

## Sources

Search before you write when any one of these is true:

- The user names a paper or model ("replicate Bianchi 2011") and you are not sure of its equations,
  calibration or timing.
- The model has a mechanism you do not know well (a special friction, constraint, preference or market
  structure), and you cannot write a complete, consistent system from memory.
- The equations the user gave are incomplete and need the original source.
- You "roughly remember" the model but cannot write every first-order condition. Treat "roughly
  remember" as "do not know": search.

Do not search when the model is a textbook standard (basic RBC, three-equation NK) and you are sure of
every equation. Go to Decisions.

Writing an unfamiliar model from vague memory is the most dangerous source of errors. Recalling a paper
in your head is not a search: call the web search tool when the local catalogs have no hit.

```
a. Local catalogs first (no web; details in catalog-lookup.md):
   grep catalog.csv (model structure) and catalog-code.csv (Dynare coding patterns,
   including the DSGE_mod examples). A hit is your template. Fallback: model-archive-catalog.csv.
   DSGE_mod is fully local in references/examples-code/. Do not search the web for it.
b. No local hit: search the paper and its technical appendix on the web.
   - "<author> <year> <title> working paper pdf" (NBER, SSRN, journal, author page)
   - Complete first-order conditions, the steady-state derivation and the calibration table
     are often in the online or technical appendix, not in the main text.
c. Extract four items. All four are required; if one is missing, search more or ask the user:
   1. The complete equilibrium system (every first-order condition, constraint and exogenous process)
   2. The calibration or estimated parameter table, with the data frequency (quarterly or annual)
   3. The timing (capital as a stock at the end or at the beginning of the period?
      information set of each expectation?)
   4. The shocks (which shocks, standard deviations, correlations)
d. Trust order: paper and appendix > author code > known replication (DSGE_mod) > forum > blog or
   lecture notes.
   - A .mod that disagrees with the paper: follow the paper and record the difference in the
     file header.
   - Key information still missing after the search: ask the user (PDF, version). Do not invent
     equations or calibration values to fill the gap.
   - List the sources you actually used in the file header comment.
No web tool: tell the user what is missing and what to provide (PDF, equation system, calibration
table). Do not write the missing part from guesses.
```

The search is visible output. Do not turn the knowledge check into a long internal monologue: decide at
once that you know the model, or search or ask at once.

## Decisions

Before you write any agent block, settle which agents the model has and the key features of each. Do not
silently default to a "standard version": a wrong agent setup often forces a rewrite of a whole block.

Skip this step when the user named a paper (Sources defines the model), gave a complete equation system,
or already described the agents' features in this conversation, and when the task changes an existing
`.mod` (SKILL.md "Existing model").

Ask only about choices that change the structure of the equations and that the user has not settled.
Put all open choices in one message, each with options and your recommended default. Do not ask about
form choices that the writing rules already decide (R8 nonlinear form, R5 names, `log_x` helper
variables, `[name=]` tags). If the user says "your call", or does not answer a choice, state the default
you use ("Default: ...; tell me if you want a change") and continue.

Typical open features per agent:

```
Households:
  - Utility: separable or non-separable labor? GHH (Greenwood-Hercowitz-Huffman) or KPR
    (King-Plosser-Rebelo)? Consumption habit?
  - Heterogeneous (savers vs hand-to-mouth, HANK)? Epstein-Zin preferences?
  - Labor supply: endogenous hours? Wage stickiness (unions)?
Firms:
  - Market structure: perfect or monopolistic competition? Two layers (final good and intermediate goods)?
  - Price stickiness: Calvo or Rotemberg? Indexation? Wage stickiness?
  - Inputs: capital and labor? Variable capital utilization, investment adjustment costs, fixed costs?
Central bank:
  - Rule: Taylor rule (response to inflation, output gap, interest-rate smoothing)? Money growth rule?
    Optimal policy: under commitment (Ramsey) or under discretion?
  - A zero lower bound, or a bound the paper sets away from zero? Inflation targeting or price-level targeting?
Government / fiscal:
  - Government spending shock? Taxes (lump-sum or distortionary: labor, capital, consumption)?
    Debt and a fiscal rule? How is the budget balanced?
Banks / financial:
  - Financial frictions (BGG financial accelerator, Gertler-Karadi bank net worth, collateral
    constraints)? Credit spreads, bank capital constraints, liquidity?
Other: open economy (exchange rate, UIP, trade)? Which exogenous shocks, with what persistence?
       Data frequency (quarterly or annual)?
```

Record the settled features in the file header comment (in the user's language, R1); they are the basis
of the equations. A paper that cites a whole block from another paper ("the financial sector follows BGG
(1999)") is also a Decisions item: see SKILL.md "Decisions".

## Build in stages

Never write the whole file and then run it: later errors tangle with earlier ones and are hard to
locate. Write one stage at a time, and pass its check before you write the next stage. Each stage is one
todo item.

| Stage | Writes | Check before the next stage |
|---|---|---|
| 1 | `<model>_derivation.md` | eight sections; section 8 count; Walras-law check |
| 2 | file header and declarations | static check |
| 3 | calibration and `model` block | static check: diagnostics, `count_gap`, timing classes |
| 4 | steady state and `resid; steady; check;` | static check, then numerical run |
| 5 | `shocks` and the experiment command | static check, then numerical run |
| 5b | calls to the plot scripts | numerical run: the figure files exist |
| 5c | `run_<model>.m` | numerical run: the script runs end to end |

Two kinds of check:

- **Static check (Dygnosis).** After Stages 2 and 3, and again after each later edit: run
  `dynare_diagnose` and fix every Error; read `dynare_equations` `count_gap` (R4); read the written
  timing classes with `dynare_model_info` (R2). Tool use: dygnosis-workflow.md. Connection:
  dygnosis-setup.md. A static check never shows a steady state, the Blanchard-Kahn conditions,
  determinacy or IRFs.
- **Numerical run (Dynare).** From Stage 4: official Dynare under MATLAB or Octave, through the
  execution route the host offers. The loop, the routes and the stop limits are in debugging.md
  "Run-and-fix loop". If no route exists, say what you could not run and give the user the exact
  commands.

Before you diagnose an error yourself, check known-issues.md and debugging.md (read-only references).
If you fix a problem they do not cover, describe the symptom, cause and fix in the report (SKILL.md
"Report").

Form (R8): write the nonlinear equations by default. Use `model(linear);` only when the user asks for a
linear model or the source gives only a linearized system. Details: "R8 details" below.

## Stage 1 (derivation note)

Purpose: get the economics right in mathematics first, then translate it mechanically into Dynare. A
mathematical error is cheapest to catch here, not after the IRFs look wrong.

When: by default for a replication of any paper, for any non-standard mechanism (TANK or HANK, financial
frictions, open economy, optimal policy, OccBin and similar), and when the user's equations need a
consistency check. Skip it for a small edit of an existing file (SKILL.md "Existing model") and for a
textbook model (basic RBC, three-equation NK); then say in one sentence where the closed-form steady
state comes from. If the user says to skip it, skip it and state the risk once: the paper-to-Dynare
timing translation is then not cross-checked, and a mathematical error shows up only as wrong numbers or
a Blanchard-Kahn failure after the run.

Output: a separate markdown file `<model>_derivation.md` (not inside the `.mod`). Follow
derivation-style.md for the structure, the LaTeX rules, the FOC numbering and the symbol table. For what
goes into each agent's problem, see modeling-blocks.md. The eight sections:

1. Model overview: which model, source, the experiment in this file, the agents.
2. Optimization problems: per agent, the objective and the budget, technology and resource constraints
   (LaTeX).
3. First-order conditions (FOC): per agent (Euler equation, labor supply, capital, price setting, policy
   rule and so on), numbered (F1), (F2), ...
4. Market clearing and aggregate identities: resource constraint, factor-market clearing, aggregation
   (heterogeneous agents by their shares).
5. Exogenous processes: form and persistence of each AR process or shock.
6. Steady-state solution: the steady-state system and the closed-form or reverse-solve steps, in an
   order that evaluates from top to bottom, ready to copy into `steady_state_model`. A linearized model:
   state that the steady state is zero.
7. Timing and form conventions: stock at the end or at the beginning of the period (R2), logs or
   levels, nonlinear by default (R8).
8. Variable and parameter table (preview of Stage 2): endogenous variables, exogenous variables and
   parameters that appear in the FOCs, with the equation that determines each endogenous variable, so
   that the equation count can be checked against the variable count.

Two required checks. Each prevents a Blanchard-Kahn failure later.

**Check 1: one equation per variable (section 8).** Go row by row through the "Determined by" column. An
empty cell for an endogenous variable means a missing equation: add it to the derivation before Stage 2.
The exceptions follow R4: a policy instrument under `ramsey_model` or `discretionary_policy`, and the
separate count per heterogeneity dimension (heterogeneity.md). A frequent miss: for a CES or
Dixit-Stiglitz bundle, the aggregator definition itself (for example
$`CES_t = [\ldots]^{\varepsilon/(\varepsilon-1)}`$) is a different equation from the demand equations
(F1, F2). List both. With only the demand equations, `ces` is a variable that no equation determines.
After Stage 3, Dygnosis shows the same miss as a count mismatch (`count_gap`, E188 or W013), or as E186
when the variable is in no equation at all.

**Check 2: Walras's law (section 4).** After you write market clearing, check Walras's law explicitly.
In general equilibrium, one of the equilibrium conditions (goods, labor, asset markets, balance of
payments) is redundant.

1. Count all equilibrium conditions.
2. Find the one that is a linear combination of the others. Most common: the household budget
   constraint = goods-market clearing + the net-foreign-asset definition + the UIP or
   balance-of-payments identity.
3. Mark it in the derivation as "redundant by Walras's law; not in the model block", and write the path
   that shows it (for example "household budget constraint").

If the redundant equation goes into the `.mod` unmarked, the equation count equals the variable count
but one independent equation is missing. A static check cannot see this. Dynare shows it only in a
numerical run: a singular Jacobian (`model_diagnostics` reports it), exploding eigenvalues, or a
Blanchard-Kahn failure.

After the note: do not stop and wait for confirmation, unless the user asked to review the derivation
first or a structural choice is still open (then ask, see Decisions). Otherwise continue to Stage 2 and
list the note in the report.

Handoff: in Stages 2 to 4, keep the note open and translate it line by line (variables from section 8,
equations from the FOCs and constraints of sections 2 to 5, the steady state from section 6). Do not
re-derive from memory: translation is faster, more accurate, and keeps the `.mod` consistent with the
note. Tag each model equation `[name='...']` with the FOC it comes from (note number or paper equation
number), so the count (R4) lines up. If the translation shows an error in the derivation, fix the note
first, then continue.

## Stage 2 (declarations)

Get the variables right before any equation. Translate the section 8 table row by row into
declarations; math symbols become ASCII names (ψ → `psi`, β → `betta`; R5).

Write the file header comment and three declarations. Give every name a TeX name and a `long_name`
(R1):

- `var`: endogenous variables, including state variables, control variables and helper variables such
  as `log_y`.
- `varexo`: exogenous variables. With stochastic commands, only innovations such as `eps_z`; AR and other
  persistent processes are endogenous (R3). In a perfect foresight experiment an exogenous variable may
  carry the path itself (R3).
- `parameters`: all parameters, including those that `steady_state_model` solves for.

```dynare
var y $Y$ (long_name='Output')
    c $C$ (long_name='Consumption')
    k $K$ (long_name='Capital')
    z $z$ (long_name='TFP process')
    log_y ${\log Y}$ (long_name='Log output');      // helper variable for reporting
varexo eps_z ${\varepsilon_z}$ (long_name='TFP innovation');
parameters alppha ${\alpha}$ (long_name='Capital share')
           psi ${\psi}$ (long_name='Labor disutility weight');   // solved in steady_state_model
```

Check:

- Every endogenous variable in section 8 is in `var`; `varexo` matches R3; every parameter is declared.
- No name that R5 avoids. Dygnosis does not report R5 names: check them by reading.
- Static check: `dynare_diagnose`. Fix parse errors (E001), duplicate declarations (E030) and missing
  `long_name` (I209). Ignore W010 (parameter without a value) and any report of a declared but unused
  symbol (E021, E186, W022) until Stages 3 and 4 add the values and equations. Do not run Dynare yet: it
  refuses a file whose exogenous variables appear in no equation.

## Stage 3 (model block)

Translate the FOCs, constraints and exogenous processes of sections 2 to 5 into model equations, one by
one: one FOC is one equation, in the note's order. Tag each equation `[name='Fk: ...']` with its source,
and put the comment (user's language) on its own line above the tag (R1).

Write the parameter calibration and the `model` block from the Stage 2 declarations and the note's FOCs.
Do not write the steady state, `shocks` or the experiment yet.

Static check:

- `dynare_diagnose`: no Error (R7 statement syntax, undeclared names E020, and so on). Expected at this
  stage: I050 (no `initval` or `steady_state_model` yet) and W010 for parameters that Stage 4 solves in
  `steady_state_model`. Fix everything else.
- `dynare_equations` `count_gap` (R4): `delta` is equations minus endogenous variables. A plain model
  needs `delta` = 0 and an empty `unreferenced_endogenous`. With `ramsey_model` or
  `discretionary_policy`, `delta` equals minus the number of policy instruments (`expected_delta`).
  Heterogeneous models: count each dimension (heterogeneity.md). A mismatch also shows as E188 or W013
  (heterogeneous: E192 or W208).
- `dynare_model_info` (R2): the written timing classes match section 7 of the note. Under the default
  convention, the stocks are in `predetermined` and the forward-looking variables in `forward_looking`. For a small
  RBC with `y = exp(z)*k(-1)^alppha*n^(1-alppha);` and an Euler equation in `c(+1)` and `y(+1)`, the
  classes are `predetermined` = `k`, `z`; `forward_looking` = `y`, `c`; `static` = `invest`, `n`,
  `log_y`. Under the default convention, a stock in `forward_looking` or `mixed` usually means a timing
  slip (for example `k` instead of `k(-1)` in production makes `k` `mixed`). The classes show the
  timing as written: a variable listed in `predetermined_variables` and written as `k(+1)` shows as
  `forward_looking`.

Count mismatch: compare with the section 8 table row by row (which FOC was not translated, which variable
is declared extra or is missing). Then see debugging.md (equation count, and the error, cause and fix
table).

Without Dygnosis: connect it (dygnosis-setup.md). If that is not possible, the first Dynare run in Stage
4 prints `Found N equation(s).` and refuses a count mismatch (`There are ... equations but ...
endogenous variables!`); check the timing by reading each equation against R2.

## Stage 4 (steady state)

Write the steady-state block and `resid; steady; check;`.

- Preferred: copy the section 6 solution of the note into `steady_state_model`. The note already orders
  the closed-form and reverse-solve steps from top to bottom; calibration targets are reverse-solved
  (steady-state.md).
- No closed form in the note: an `initval` block with an economically sensible guess for every
  endogenous variable (Dygnosis W052 lists the ones you left out). Still no convergence:
  `homotopy_setup` (steady-state.md).

Static check: `dynare_diagnose`. Look for E130 (use before assignment in `steady_state_model`), W042
(endogenous variable missing from `steady_state_model`) and W131 (variable assigned twice in
`steady_state_model`). The Stage 3 W010 for reverse-solved parameters is now gone.

Numerical run (Dynare):

- `resid`: all residuals near zero. With `resid(non_zero);` Dynare prints `All residuals are zero`.
- `steady`: no NaN; plausible ratios (C/Y, I/Y, K/Y).
- `check`: prints `There are N eigenvalue(s) larger than 1 in modulus for M forward-looking
  variable(s).` and then `The order and rank conditions are verified.`

Pass: `steady` succeeds, the residuals are near zero, and `check` verifies the Blanchard-Kahn
conditions. Stage 5 adds only shocks and commands, which do not change the eigenvalues, so fix a
Blanchard-Kahn failure here. With `ramsey_model` or `discretionary_policy`, the optimal-policy command
changes the system: follow optimal-policy.md for where `steady` and `check` go.

Fail: see steady-state.md and debugging.md. The equation with a large residual often exposes a
steady-state algebra error or a typo in the model. If the steady state in the note is wrong, fix the note
too, so the two stay consistent.

## Stage 5 (experiment)

Write the `shocks` block and the experiment command:

- `shocks`: stochastic commands use `stderr`, `var ... = ...;` and `corr`; perfect foresight uses
  `periods` and `values` (or `endval` for a permanent change).
- One command family per file (Dygnosis E205 reports perfect-foresight and stochastic commands
  together): `stoch_simul`; `perfect_foresight_setup` with `perfect_foresight_solver`; `estimation`;
  `ramsey_model` with `stoch_simul` (`ramsey_policy` is deprecated, W150); `discretionary_policy`;
  `osr`. Read the task reference that SKILL.md "Route the task" names.

Static check: `dynare_diagnose` (for example W060 IRF with no written shock size, W120 stochastic command
with no stochastic exogenous variable, E215 or E235 for `discretionary_policy`). `dynare_list_options`
lists the valid options of a command.

Numerical run (Dynare). Pass: the Blanchard-Kahn conditions hold, the command finishes and produces its
output (policy function, moments and IRFs; transition path; posterior), the IRFs have no NaN or Inf, and
the results are plausible. Fail: for a Blanchard-Kahn failure, check timing (R2), signs and the equation
count (R4) first; for everything else, debugging.md "Run-and-fix loop".

## Stage 5b (plots)

Default when the output has IRFs or simulated paths. Skip it for a small edit, a pure check, a task
without figures, or when the user declines; say so in one sentence.

Read publication-plots.md and call the plot script that matches the output. It replaces Dynare's own
figures (add `nograph` to `stoch_simul`):

- IRFs (`stoch_simul` with `irf=`, Bayesian posterior IRFs) → `plot_irfs_pub.m`.
- Simulated series and perfect foresight transition paths (`oo_.endo_simul`) → `plot_series_pub.m`.

Run it and confirm that the figure files exist. An expensive solve with repeated figure changes
(heterogeneity, second or third order, estimation, large models): do not re-solve in the same script as
the plots. Split solve and analysis, cache with `save <model>_oo.mat oo_ M_`, and rerun only the plots
from the cache (matlab-workflow.md, which also covers multi-model comparisons).

## Stage 5c (run script)

Default for a new model or a replication that produces IRFs or simulated paths. Skip it for a small edit,
a pure check, or when the user declines; say so in one sentence.

Read run-script.md. Join the parts (`.mod`, steady-state `.m`, plot script, self-check prints) into a
self-contained run script `run_<model>.m` that reruns everything in one step. Make the run script call
the plot script: this is the step most often missed. If the project already has a `main` script, hook
into it instead of starting a new one. Run it once (debugging.md "Run-and-fix loop") and confirm that it
runs end to end and writes its output files. If no execution route exists, deliver the script and say
that it was not run.

## Final checks

Before the report:

1. Static: run `dynare_diagnose` on the finished file (`dynare_workspace_diagnose` for a project of
   several files). No Error. Fix each Warning, or say in the report why it stays. Information I208,
   I209 and I210 shows missing metadata (R1).
2. Go through the final checklist in debugging.md (the items that apply).
3. A model with several agents: the equation that Walras's law makes redundant is not in the model
   block (Stage 1, check 2).
4. Numerical: the last Dynare run of the finished file was clean (steady state, Blanchard-Kahn
   conditions, experiment output). If you could not run it, the report says what was not run and gives
   the commands.

## File skeleton

The structure after all stages. Dynare is sensitive to the order of blocks. Comments in the file follow
the user's language (R1); the skeleton comments are English examples.

```dynare
/*
 * Header (user's language): what the model is, sources, the experiment in this file,
 * key conventions (timing, logs or levels). Record the agent features settled in
 * Decisions here.
 */

//==================== Declarations ====================
var y $Y$ (long_name='Output')                   // endogenous; short end-of-line comment
    ...
    log_y ${\log Y}$ (long_name='Log output');   // helper variable for reporting
varexo eps_z ${\varepsilon_z}$ (long_name='TFP innovation') ... ;   // innovations only (R3)
parameters alppha ${\alpha}$ (long_name='Capital share') ... ;

//==================== Calibration ====================
alppha = 0.33;
// Parameters solved in steady_state_model are not assigned here

//==================== Model ====================
model;
// Comment on its own line (user's language), above the name tag (R1)
[name='Euler equation']
... ;                       // equations = endogenous variables (R4)
[name='Log output']
log_y = log(y);
end;

//==================== Steady state ====================
steady_state_model;         // preferred; without a closed form use initval; ... end;
   ...
end;

//==================== Checks ====================
resid;     // steady-state residuals
steady;    // solve or verify the steady state
check;     // Blanchard-Kahn conditions

//==================== Shocks ====================
shocks;
   var eps_z; stderr 0.007;    // stochastic: stderr / var = / corr; perfect foresight: periods / values
end;

//==================== Experiment (one family only) ====================
stoch_simul(order=1, irf=40) ... ;
```

## House style (from DSGE_mod)

1. A file header block (user's language): model, sources, experiment, conventions, agent features.
2. Every variable and parameter with a TeX name and an English `long_name`:
   `var c $C$ (long_name='Consumption');`.
3. Every equation with an English `[name='...']` (ideally the paper's equation number) and a comment
   above it (user's language).
4. Calibration targets reverse-solved inside `steady_state_model` (for example `l=0.33` solves for
   `psi`).
5. Helper variables `log_x = log(x);` for reporting and observation equations, each with its own
   `[name='...']` and `long_name`.
6. For estimation, observation equations preferably in growth rates (steady state 0, robust to trends).
7. Variants and repeated structures with the macro processor (macro-processor.md).
8. Fixed order: `resid; steady; check;` before the experiment commands.

## Rule details

### R1 details (labels and comments)

- Comments follow the user's language and say enough: the model, the meaning of each equation, the
  timing choice, the source of each parameter value. Use `//` at the end of a line, `/* */` blocks, and
  the file header.
- Put an equation's comment on its own line above `[name='...']`, not on the same line as the tag.
- A declaration line may end with a short comment (for example `// consumption`).
- Identifiers, `long_name` values, equation tags and TeX names stay English ASCII. Dynare 7.2 accepts
  UTF-8 in comments and quoted strings; ASCII labels keep MATLAB/Octave output, TeX and plots portable.
- New models and substantial extensions: every equation has `[name='...']`, every declaration a TeX
  name and a `long_name`, helper variables such as `log_y` included. Dygnosis Information I208 (equation
  without a name tag), I209 (declaration without `long_name`) and I210 (number written in an equation)
  find the gaps. Missing metadata is not a Dynare refusal.
- The conventions of an existing file, a small edit, or an explicit user preference take precedence.

### R2 details (timing)

- The time index of a variable is the period in which it is decided. Default: the "stock at the end of
  the period" convention.
- A control (forward-looking) variable is decided in the current period: its own definition has no
  lead. It can appear with `(+1)` inside an expectation, for example `c(+1)` in the Euler equation.
- A predetermined (state) variable was decided in the previous period, so the current-period equations
  use its lag: production uses `k(-1)`, the law of motion is `k = invest + (1-delta)*k(-1);`.
- Write leads and lags as `x(+1)`, `x(-2)`. Parameters have no time index (Dygnosis W121 reports a
  parameter with a lead or lag).
- The "stock at the beginning of the period" convention: `predetermined_variables k;`, production uses
  `k`, the law of motion is `k(+1) = invest + (1-delta)*k;`.
- One convention per file. Check the written timing classes with `dynare_model_info` and the
  per-equation `idents` in `dynare_equations` (Stage 3).

### R8 details (nonlinear by default)

Write the original nonlinear equations (first-order conditions, constraints, exogenous processes) and let
Dynare approximate them. Do not linearize by hand. Reasons:

- Hand linearization is a frequent source of hidden errors, and Dynare cannot detect them.
- Only a nonlinear model supports higher-order perturbation (`order=2`, `order=3`: welfare, risk
  premia, uncertainty shocks).
- The steady state has an economic meaning, so you can check the main ratios.
- The same file can switch between first order, second order and perfect foresight.

Write `model(linear);` (variables are percentage or log deviations; the steady state is usually all
zero) only when:

1. the user asks for a linear model, or
2. the source gives only a linearized system.

Otherwise always write the nonlinear model; there is nothing else to decide.

`discretionary_policy` does not by itself require `model(linear)`. The manual ("Optimal policy under
discretion") requires a quadratic objective and either a linear model or a first-order solution with an
analytical steady state. Dygnosis reports `discretionary_policy` without `instruments` (E215) and with an
order greater than 1 (E235). Details: optimal-policy.md. In `model(linear)`, Dygnosis W140 reports a
nonlinear operator and E210/E211 a nonsmooth one.

With a nonlinear model:

- For log-deviation IRFs, add a helper variable `log_y = log(y);`.
- Write shock standard deviations as decimals (1% is `stderr 0.01`, not 1).
- Write an AR process in logs directly: `log(A) = rho_a*log(A(-1)) + eps_a;`.

### Model block notes

- An equation is `lhs = rhs;`. An equation in homogeneous form may give only the left-hand side (Dynare
  reads it as `= 0`).
- `>=`, `<=` and `==` do not make an inequality constraint: Dynare reads `a>=b` as the homogeneous
  equation `(a>=b)=0`. Use OccBin or a complementarity condition (R6).
- A model-local variable `#z = ...;` shares a subexpression between equations. Its scope is the model
  block only.
- `STEADY_STATE(x)` takes the steady-state value (common in Taylor rules and output gaps).
- `EXPECTATION(-1)(x(+1))` takes the expectation with the information set of the previous period.

## Cleanup

Iteration leaves many intermediate files. Before the report, review the working folder. Delete only
files that this task's Dynare runs or your own work created; never delete user files. List the folder
before the first Dynare run, so you can later tell which items the task created. An item of unclear
origin: list it and ask the user. An item on the keep list: never delete it.

Delete list (`<m>` is the `.mod` file name without extension; only when this task created the item):

- Dynare products:
  - `+<m>/`: the package folder of the preprocessor (`driver.m`, `dynamic.m`, `static.m`,
    `set_auxiliary_variables.m`, and `steadystate.m` generated from `steady_state_model`). The
    generated `+<m>/steadystate.m` is not the user-written `<m>_steadystate.m` in the model folder
    (steady-state.md, on the two forms of a steady-state file).
  - `<m>/`: the output folder, with `Output/` (`<m>_results.mat`) and `graphs/` (Dynare's own figures,
    such as `<m>_IRF_*.eps/.pdf/.fig`, unless the user wants them).
  - `<m>.log`, and `<m>_TeX_binder.*` when TeX output was requested.
  - This list matches Dynare's own `clean_current_folder` (`<dynare-root>/matlab/utilities/general/`).
    Do not call that function for cleanup: it acts on every `.mod` in the folder.
- Your own temporary files: debug scripts, crashed or superseded `.mod` versions (`test.mod`,
  `<m>_v1_broken.mod`), placeholder files.

Keep list (never delete; if an item looks like junk, ask the user):

- Every file the user supplied, and data files (`.mat` data, `.xlsx`, `.csv`, observed data and so on).
- The derivation note `<m>_derivation.md`, the final `.mod`, and the reference files of this skill.
- The run scripts `run_<m>.m` and `analyze_<m>.m`, the plot scripts `plot_irfs_pub.m` and
  `plot_series_pub.m`, and the figures they export (`fig_*.pdf/.eps/.png`). They are deliverables:
  without them the user cannot rerun the model or reproduce the figures.
- A user-written steady-state file `<m>_steadystate.m`. It is an external steady-state function, not a
  Dynare product; without it the model has no steady state. Dynare's `clean_current_folder` also keeps
  it.

Procedure: list the working folder, delete the items on the delete list that this task created, list the
items of unclear origin for the user, and say in the report what you deleted and what you kept. Cleanup
needs no MATLAB; file operations are enough.
