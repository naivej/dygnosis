# MATLAB-side workflow: solve once, analyze many times

Read this when a solve is slow (heterogeneous agents with time iteration, order 2 or 3, estimation,
large models) and you must change plots, normalizations or comparisons many times; when you compare
several models or scenarios (HANK against RANK, baseline against counterfactual); or when you package a
run as a script that can run again.

This file answers one question: how to organize the MATLAB scripts so that the expensive solve
runs once and the cheap analysis can change freely, and how to catch errors with analytical benchmarks.
It complements the "Run-and-fix loop" in `references/debugging.md`: the loop makes the `.mod` run; this
file covers what you do after it runs.

Run the scripts through the [MATLAB Agentic Toolkit](https://github.com/matlab/matlab-agentic-toolkit).
How to find `<dynare-root>` and connect MATLAB: `references/debugging.md`, "Run-and-fix loop". If
MATLAB is unavailable, deliver the scripts and give the user the Dynare commands.

## Principle: the solve is expensive, the analysis is cheap and changes often

Keep them apart and connect them with a `.mat` cache of `oo_`. The most common, and least visible,
waste of time is to put `dynare <model>` and the plotting or analysis code in **one** script. Then
each change of a legend, a normalization coefficient or a label solves the **whole model** again. One
time iteration of a heterogeneous HANK model can take about 30 seconds; ten plot adjustments cost five
minutes of waiting, and each run prints thousands of lines of iteration output.

Write two scripts connected by one `.mat` file:

```matlab
% ===== run_<model>.m: solve only, run once =====
cd('<working folder>');
dynare <model> noclearall nointeractive
save('<model>_oo.mat', 'oo_', 'M_', 'options_');   % freeze the results

% ===== analyze_<model>.m: analysis and plots only; change freely, reruns in seconds =====
cd('<working folder>');
S = load('<model>_oo.mat');           % no new solve: read the frozen results
oo_ = S.oo_;  M_ = S.M_;
% ... change normalizations, subplots, labels and comparisons here; reruns never touch the solve ...
```

Rule: **if you run something a second time that one solve already produced, cache it.** Plots, `Scale`,
the horizon, the compared variables are all analysis; they must not trigger a new solve.

Dynare also saves `M_`, `oo_` and `options_` in `<model>/Output/<model>_results.mat` after each run.
The next run overwrites that file, so keep your own named cache when you compare runs.

## Project folder and run script

How to write the run script `run_<model>.m` (self-contained, output chosen by experiment type, calls
the plotting code, fits into an existing `main`): `references/run-script.md`. Write it for a new model
or a replication with a numerical experiment; skip it for a small edit, a pure check, or when the user
declines. This section covers only how it works with `analyze_*.m` and the `.mat` cache in a project
with several files.

For model comparisons, external steady-state files or repeated plotting, use this layout:

```
<project>/
├── <model>.mod                 # the model
├── <model>_derivation.md       # derivation note (Stage 1)
├── run_<model>.m               # solve driver: cd + dynare + save oo.mat
├── analyze_<model>.m           # analysis and plots: load oo.mat + plot_irfs_pub
├── plot_irfs_pub.m             # plotting function of this skill (copy from references/)
└── <model>_oo.mat              # frozen solve results (intermediate file, can be regenerated)
```

**Minimal core of the solve script (`run_<model>.m`).** In a project with several files it only solves
and freezes; `analyze_*.m` makes the plots. In a project with one file, put the plots in the run script
too (full skeleton: `references/run-script.md`).

```matlab
addpath('<dynare-root>/matlab');         % self-contained: a clean MATLAB finds Dynare
cd(fileparts(mfilename('fullpath')));    % go to the folder of this script; avoids a wrong working folder
dynare <model> noclearall nointeractive  % noclearall keeps oo_; nointeractive does not wait for input
save('<model>_oo.mat', 'oo_', 'M_', 'options_');
```

`<model>_oo.mat` is an intermediate file that can be regenerated; remove it at cleanup (lists in
`references/workflow-detail.md`, see SKILL.md "Report"). `run_*.m` and `analyze_*.m` are deliverables
for you and the user: **do not delete them** (the same as `plot_irfs_pub.m`).

## Comparing several models or scenarios

For HANK against RANK, or baseline against counterfactual, **solve and freeze each model separately,
then read them together in one analysis script**:

```matlab
% run_all.m: solve the two models one after the other, freeze each
cd('<dir>'); dynare rank_model noclearall nointeractive; oo_rank = oo_; save('rank_oo.mat','oo_rank');
cd('<dir>'); dynare hank_model noclearall nointeractive; oo_hank = oo_; save('hank_oo.mat','oo_hank');

% compare.m: comparison plots from the frozen results only; change freely without a new solve
R = load('rank_oo.mat'); H = load('hank_oo.mat');
plot_irfs_pub({'Y','C'}, 'eps_g', ...
    'Scenarios',     {R.oo_rank, H.oo_hank}, ...
    'ScenarioNames', {'RANK','HANK'}, ...
    'Save', 'fig_g_multiplier');     % overlaid scenarios: see publication-plots.md
```

Key point: **run and save each model first, then compare.** Do not put two `dynare` calls in the
comparison script; each new comparison plot would then solve both models again.

### Heterogeneous (HANK) models store IRFs elsewhere

`stoch_simul` stores IRFs in `oo_.irfs.<var>_<shock>`, which `plot_irfs_pub` reads directly.
`heterogeneity_solve` stores sequence-space Jacobians in `oo_.heterogeneity.dr.G.<var>.<shock>`, with a
different shape. In Dynare 7.2, `heterogeneity_simulate` also writes IRFs to `oo_.irfs`. Before you
compare HANK against RANK, **read the IRF section of `references/heterogeneity.md`**; otherwise you
plot the wrong field.

## Check against analytical benchmarks

A plot hides errors of size: a curve with the right shape but **100 times too large or with the wrong
sign** often looks fine. When the model has a **known analytical result**, print it with `fprintf` after
the solve and compare before you trust the plot:

| Case | Analytical benchmark | Error it catches |
|---|---|---|
| RANK balanced-budget government spending | output multiplier = 1 (exact) | wrong normalization or `Scale` (for example divided by 100 once too often) |
| RANK lump-sum transfer | output effect = 0 (Ricardian equivalence) | wrong shock definition, wrong sign |
| Steady-state ratios | `C/Y`, `I/Y`, `K/Y` (debugging.md, "Wrong numbers without an error") | calibration or steady-state algebra error |
| Monetary policy shock | a contraction lowers output and inflation | wrong sign in the Taylor rule, timing error |

Example: the government spending multiplier should be about 1, but the script prints 0.01. This shows
at once that the normalization divides by 100 once too often. **When a benchmark exists, print first,
then trust the plot.** This step costs almost nothing and catches scaling and sign errors before you
make figures. Sources of benchmarks: the analytical steady state, textbook limiting cases (Ricardian
equivalence, long-run neutrality), or the closed-form solution of a RANK baseline with the same
structure.

## Silence iteration output after convergence

Time iteration, HANK calibration, homotopy and other iterative solvers print `||Δpolicy||`, residual
norms and similar at each step, often hundreds or thousands of lines. That trace helps while you debug.
**When the solver converges reliably, turn it off.** The output (and your context) stays readable, and
less text comes back through the MATLAB session. How to silence it depends on the solver and its
options (some have a `verbose` or `quiet` switch or an `options_` field). Do not let iteration logs
hide the steady-state residuals and the Blanchard-Kahn result that you need to read.

## Division of work with other references

- Making the `.mod` run, fixing errors: "Run-and-fix loop" and the error table in
  `references/debugging.md` (not repeated here).
- Parameters, scenarios and confidence bands of `plot_irfs_pub`: `references/publication-plots.md`.
- Heterogeneous models, HANK IRFs: `references/heterogeneity.md`.
- The run script: `references/run-script.md`.
- **This file** only answers how to organize the driver scripts so that the expensive solve runs once
  and the cheap analysis changes freely.
