# Run script `run_<model>.m`

Read this when you deliver a script that reruns the model in one step, as Stage 5c (run script) of a
new model or a replication, or when the user asks how to run the model, wants a script that runs it,
or asks how to get the figures automatically.

## What the run script does

At delivery the project folder usually holds several parts: the model `<model>.mod`, perhaps a
hand-written steady-state file `<model>_steadystate.m`, and the plot functions `plot_irfs_pub.m` /
`plot_series_pub.m`. None of them runs by itself. The most common gap is a plot function that was
copied but that no line calls. The user then runs `dynare`, sees only Dynare's own graphs, and the
paper figures never appear.

The run script has one job: call these parts in the correct order. One run then goes from the
solution to the figures and numbers, and the user needs to change at most the Dynare path.

Test: when the run script finishes, do all the outputs the user expects (figures, numbers) exist?
Each missing output is a part that is not connected.

## When to write one

Stage 5c (run script) is the default for a new model or a replication that produces IRFs or
simulated paths. Do not write one for a small edit or a pure check, or when the user declines. If
the project already has a main script, extend it instead (see "Project variants").

## Adapt it to the project

This is guidance, not a fixed template. A single-file RBC model needs a few lines. A model with a
hand-written steady-state file needs that file next to the `.mod`. A comparison of several models
solves and saves each model in turn. Build the script from the five steps below. Choose the outputs
by experiment type. Use the project's own names and paths, not the example names.

To find the parts, use Dygnosis (see dygnosis-workflow.md). `dynare_related_files` lists the includes
and the steady-state and helper files of a `.mod`. `dynare_model_info` gives the exact variable and
shock names for the plot calls.

## The five steps

```matlab
% =====================================================================
%  run_<model>.m  -  <one line: this script solves model X and produces Y>
%  Usage: run the file (F5, run('run_<model>.m'), or the MATLAB Agentic Toolkit).
%  No manual path setup or change of folder is needed.
% =====================================================================
clear; close all; clc;
isOctave = exist('OCTAVE_VERSION', 'builtin') ~= 0;

% ---- 1. Environment: self-contained ----
if isempty(which('dynare'))
    addpath('<dynare-root>/matlab');   % Dynare install path on this machine
end
here = fileparts(mfilename('fullpath'));   % folder of this script
if ~isempty(here), cd(here); end

% ---- 2. Solve: dynare calls <model>_steadystate.m if it exists ----
dynare <model> noclearall nointeractive

% ---- 3. Check key numbers before you trust figures (see matlab-workflow.md) ----
ys = oo_.steady_state;                                  % set by steady; (else initval values)
if isfield(oo_, 'dr') && isfield(oo_.dr, 'ys') && ~isempty(oo_.dr.ys)
    ys = oo_.dr.ys;                                     % steady state used by stoch_simul
end
ss = @(v) ys(strcmp(M_.endo_names, v));                 % steady-state value by name
fprintf('\n===== steady-state check =====\n');
fprintf('C/Y = %.4f   I/Y = %.4f\n', ss('c')/ss('y'), ss('invest')/ss('y'));

% ---- 4. Outputs: choose by experiment type (table below) and call the code ----
if ~isOctave   % the plot functions need MATLAB
    plot_irfs_pub({'y','c','invest','l'}, 'eps_z', 'Save', 'fig_irf');
end

% ---- 5. Save the results for analyze_<model>.m, so figures change without a new solve ----
save('<model>_oo.mat', 'oo_', 'M_', 'options_');
```

Steps 1, 2 and 5 fit almost every task. Fill steps 3 and 4 for the model and the experiment.

- **Step 1, Dynare path.** Do not copy a fixed install path from another machine. Find the path on
  the machine that runs the script and write it into the placeholder `<dynare-root>/matlab`. For
  example, run `which dynare` in the MATLAB session, or find the folder that contains
  `matlab/dynare.m`. Say in the report which path you wrote. If Dynare is already on the path, the
  `addpath` line is skipped.
- **Step 1, folder.** `mfilename('fullpath')` is empty when the code is pasted into a session instead
  of run as a file. In that case, `cd` to the project folder first.
- **Step 2, options.** With `noclearall`, `dynare` does not delete the global variables and the
  functions that use persistent variables before the run. With `nointeractive`, Dynare does not
  request user input (running-dynare.rst).
- **Step 3.** A figure can hide an error of scale or sign, so print key numbers first. Where an
  analytical benchmark exists, print it next to the result (matlab-workflow.md). Read the steady
  state from the right field. `steady;` fills `oo_.steady_state`. `stoch_simul` without `steady;`
  puts the steady state in `oo_.dr.ys` and leaves `oo_.steady_state` at the `initval` values (zero
  for variables without `initval`), so a ratio such as `C/Y` prints `NaN` (checked with Dynare 7.2).
- **Step 4.** The plot functions need MATLAB (publication-plots.md, "Compatibility and cleanup").
  The `isOctave` guard lets the other steps run under Octave. Under Octave, report that no figures
  were made.
- **Step 5.** Dynare already saves `M_`, `oo_` and `options_` in
  `<model>/Output/<model>_results.mat` (running-dynare.rst, *Output*). The next run of the same
  `.mod` overwrites that file. The named copy keeps the results of this run when one `.mod` runs
  several experiments or calibrations, and it is the file that `analyze_<model>.m` loads.
- **Language (R1).** Comments and printed messages follow the user's language. Identifiers, file
  names and labels in figures stay English ASCII. The same rule holds for the `.mod` and the `.m`
  files.

## Step 4: choose the output by experiment type

Run scripts are easy to hard-code as "plot the IRFs". But the experiment types of this skill put
their output in different places and need different presentations. Look at the experiment command of
the task first, then put the matching output code into step 4.

| Experiment (command) | Output in | Step 4 code |
|---|---|---|
| Stochastic simulation, IRFs (`stoch_simul(irf=N)`) | `oo_.irfs.<var>_<shock>` | `plot_irfs_pub({...}, 'eps_x', 'Save','fig_irf');` (publication-plots.md) |
| Stochastic simulation, simulated series (`stoch_simul(periods=T)`) | `oo_.endo_simul` | `plot_series_pub({'y','c'}, 'Save','fig_sim');` (add `'Center','ss'` for deviations) |
| Stochastic simulation, theoretical moments (`periods=0`, the default) | `oo_.mean`, `oo_.var`, `oo_.autocorr` | no figure; print a table of key moments (means, standard deviations, autocorrelations, key correlations) with `fprintf` |
| Perfect foresight transition path (`perfect_foresight_solver`) | `oo_.endo_simul` (all periods, with initial and terminal columns) | `plot_series_pub({'y','c','k'}, 'Save','fig_transition');` (perfect-foresight.md) |
| Estimation, posterior IRFs (`estimation(..., bayesian_irf)`) | `oo_.PosteriorIRF.dsge.<Mean, Median, HPDinf, HPDsup, …>.<var>_<shock>` | `plot_irfs_pub` with the posterior mean as `oo_` and the HPD interval as `Bands` (publication-plots.md, "Posterior IRFs") |
| Forecasting (`forecast`) | `oo_.forecast` | fan chart: Dynare's own forecast graphs, or your own bands (forecasting.md) |
| Shock decomposition (`shock_decomposition`) | `oo_.shock_decomposition` | stacked bar chart (shock-decomposition.md) |
| Heterogeneous-agent model (`heterogeneity_simulate`) | `oo_.irfs`, and `oo_.endo_simul` when `periods > 0` | the same plot functions (heterogeneity.md) |

Step 4 must call the plot function and check that the files were written. This is the step that was
missed most often. Each plot function covers one kind of output. Call it; do not write inline `plot`
code:

- `plot_irfs_pub.m` plots IRFs (publication-plots.md).
- `plot_series_pub.m` plots time series in `oo_.endo_simul`: simulated series and perfect foresight
  transition paths. Same interface and style.

Copy both into the project folder first. They are deliverables, so do not delete them during
cleanup. If one task produces several outputs (for example IRFs and a simulated series), call each
in turn in step 4. Short `plot_series_pub` use: `plot_series_pub({'y','c'}, 'Save','fig_sim')`. Add
`'Center','ss'` for deviations from the steady state (with `'Scale',100` for percent deviations of
log variables). Use `'Scenarios',{ooA,ooB}` to compare scenarios.

## Project variants

1. **The project has a main script.** Do not create a separate `run_<model>.m`. Put steps 2 and 4
   (solve and plot) at the matching place in the existing main script, and keep its path and folder
   conventions. In the report, say which step of the main script now runs the model.
2. **Hand-written steady-state file.** `dynare <model>` finds and calls `<model>_steadystate.m`
   itself. The manual (Steady state) requires the name `FILENAME_steadystate.m` for `FILENAME.mod`.
   Do not `run` the file from the run script. Keep it next to the `.mod`, with exactly that name.
   A `steady_state_model` block makes Dynare generate `+<model>/steadystate.m` instead
   (steady-state.md).
3. **Several models or scenarios.** Solve and save each model separately. Then load the saved
   results and plot the comparison. Do not put several `dynare` calls in the comparison step: each
   change to a figure would solve all models again. Full skeleton: matlab-workflow.md (comparison of
   several models).
4. **Expensive solve (heterogeneous-agent models, estimation, third order).** Split the work into
   `run_<model>.m` (solve and save only) and `analyze_<model>.m` (load the `.mat` and plot), so that
   the expensive solve runs once. Reasons and skeleton: matlab-workflow.md.

## Run it

Run the script through the [MATLAB Agentic Toolkit](https://github.com/matlab/matlab-agentic-toolkit)
as SKILL.md "Numerical runs" describes. If MATLAB is unavailable, say what was not run and give the
user the Dynare commands to run the script.

Only a run gives the steady state, the Blanchard-Kahn conditions and the IRFs. Do not infer them from
static checks.

## Related files

- Plot function options, scenarios, bands: publication-plots.md. This file only covers how the run
  script calls them.
- Separate solve and analysis, cached `oo_`, several models, analytical benchmarks:
  matlab-workflow.md.
- Getting the `.mod` to run and fixing errors: debugging.md "Run-and-fix loop".
- Static checks before the first run (`dynare_diagnose`): dygnosis-workflow.md.
- This file: how to write the `.m` that connects the parts, reruns in one step and produces the
  outputs.
