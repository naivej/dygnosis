# Publication plots (`plot_irfs_pub`, `plot_series_pub`)

Read this when a task produces IRFs or simulated paths and needs figures: Stage 5b (plots) of a new
model or a replication, or a user request for paper-quality, comparison or exported figures.

The skill ships two MATLAB functions in `references/`:

- `plot_irfs_pub.m` plots IRFs from `oo_.irfs.<var>_<shock>`.
- `plot_series_pub.m` plots paths from `oo_.endo_simul`: stochastic simulation (`stoch_simul` with
  `periods`) and perfect foresight transition paths.

Both draw one panel per variable and one line per scenario (or per shock), with a zero line, no box,
and vector PDF export. Color and line style change together, so lines stay distinct in
black-and-white print, a common journal requirement. The default style follows figures in AER, JME
and Econometrica. Dynare's own `stoch_simul` graphs (`<model>/graphs/<model>_IRF_<shock>.eps`) show one
model at a time and cannot overlay scenarios.

## When to plot

Stage 5b (plots) is the default for a new model or a replication that produces IRFs or simulated
paths: do it without waiting for the user to ask. Do not add plots to a small edit or a pure check,
or when the user declines.

## Set up

1. Copy the function you need from `references/` into the folder of the `.mod` (or a folder on the
   MATLAB path). The copy is a deliverable: do not delete it during cleanup.
2. Turn off Dynare's own graphs to save time: add `nograph`, for example
   `stoch_simul(order=1, irf=40, nograph) y c invest l;`.
3. Make sure the variables you plot have IRFs:
   - If the `stoch_simul` command lists variables, Dynare stores IRFs only for those. With no list,
     it stores them for all declared endogenous variables.
   - Dynare stores no IRF for a shock with zero variance. Dygnosis W060 warns when the shocks
     selected for IRFs have no written variance or standard error.
   - Take exact variable and shock names from `dynare_model_info` (see dygnosis-workflow.md).
4. For percent deviations, plot log variables: helper variables such as `log_y = log(y);`, or the
   `loglinear` option (see stochastic-simulation.md). Then keep the default `Scale` 100.
5. Call the function after the `dynare` run, in the run script (run-script.md) or in the session.
   Run it through the MATLAB route the host offers (MATLAB MCP server or `matlab -batch`). Check that
   the figure files exist.

## `plot_irfs_pub` calls

```matlab
% Simplest: oo_/M_ are in the workspace
plot_irfs_pub({'y','c','invest','l'}, 'eps_z');

% Save a PDF (paper figure)
plot_irfs_pub({'y','c','invest','l'}, 'eps_z', 'Save','fig_irf_tech');

% Compare scenarios (same variables, same shock, distinct in black and white)
plot_irfs_pub({'y','pi','r'}, 'eps_a', ...
    'Scenarios',     {oo_base, oo_alt}, ...
    'ScenarioNames', {'Baseline','Sticky wages'}, ...
    'Save','fig_irf_compare');

% Several shocks in one figure
plot_irfs_pub({'y','c'}, {'eps_a','eps_g'}, 'OverlayShocks', true);

% Two bands (for example 90% and 68%): wide band first, light grey; narrow band second, darker
plot_irfs_pub({'y','pi'}, 'eps_a', 'Bands', {{lo90,hi90},{lo68,hi68}});

% Demo figure from synthetic data, to check the style first
plot_irfs_pub
```

To get two `oo_` structures for a comparison: after model A runs, `ooA = oo_;`; after model B runs,
`ooB = oo_;`; then `plot_irfs_pub(..., 'Scenarios', {ooA, ooB})`. Or load each model's results:
Dynare saves `M_`, `oo_` and `options_` in `<model>/Output/<model>_results.mat`, for example
`A = load('modelA/Output/modelA_results.mat'); ooA = A.oo_;`.

### Posterior IRFs

`estimation(..., bayesian_irf)` stores `oo_.PosteriorIRF.dsge.<MOMENT>.<var>_<shock>`, with moments
`Mean`, `Median`, `HPDinf` and `HPDsup` (among others). The HPD interval has the probability set by
`mh_conf_sig` (default 0.9). These structs have the same field names as `oo_.irfs`, so pass them
directly:

```matlab
pirf = oo_.PosteriorIRF.dsge;
plot_irfs_pub({'y','pi'}, 'eps_a', ...
    'oo_',   struct('irfs', pirf.Mean), ...        % posterior mean as the line
    'Bands', {pirf.HPDinf, pirf.HPDsup}, ...       % one band at mh_conf_sig
    'Save',  'fig_posterior_irf');
```

A second band needs a second interval that you compute, for example from an `estimation` run with
another `mh_conf_sig`. Give band data as structs with fields `<var>_<shock>`. A numeric vector is
drawn unchanged in every panel, and an H×nv matrix is not split by column. Bands are drawn only with
one scenario and without `OverlayShocks`.

## `plot_irfs_pub` options

| Option | Effect | Default |
|---|---|---|
| `oo_` / `M_` | Dynare structures | read from the base workspace |
| `Scenarios` / `ScenarioNames` | one line per `oo_` (replaces `oo_`) / legend names | none / `Scenario 1`, `Scenario 2`, … |
| `Scale` | IRF multiplier: 100 turns a log deviation into percent; use 1 for variables in levels | 100 |
| `Horizon` | number of periods to plot | full length |
| `Layout` | `[nr nc]` panel grid | near-square |
| `Titles` | custom panel titles | TeX name (see "Titles") |
| `Interpreter` | title interpreter: `auto`, `latex`, `tex`, `none` | `auto` |
| `YLabel` / `XLabel` | axis titles | `Percent dev. from SS` / `Quarters` |
| `Save` / `Formats` | file name without extension / `{'pdf','eps','png'}` | no file / `{'pdf'}` |
| `Font` / `FontSize` | font | `Helvetica` / 9 |
| `Grid` / `ZeroLine` | grid / zero line | false / true |
| `OverlayShocks` | all shocks in one figure (not with `Scenarios`) | false |
| `Bands` | `{lo,hi}` or two bands `{{lo1,hi1},{lo2,hi2}}` | none |
| `Colors` / `LineStyles` / `LineWidth` | override the style | 6 colors / `- -- : -.` / 1.6 |
| `FigSize` | `[width height]` in centimeters | estimated from `Layout` |

With several shocks and no `OverlayShocks`, the function draws one figure per shock, appends
`_<shock>` to the `Save` name, and returns the last figure handle.

The default `YLabel` is correct only when the plotted variables are logs. For variables in levels,
set `'Scale', 1` and a matching `'YLabel'`.

## Titles

Panel titles use the TeX name from the declaration (`var y $Y$ (long_name='Output');` gives `$Y$`),
rendered with the LaTeX interpreter. Dynare fills a TeX name for every variable (default: the
variable name), so give each declaration a TeX name, as R1 asks. With several shocks in separate
figures, each figure is titled with the `long_name` of its shock. To title panels with `long_name`,
pass `Titles`:

```matlab
vars = {'y','c'};
ttl  = cellfun(@(v) M_.endo_names_long{strcmp(M_.endo_names, v)}, vars, 'UniformOutput', false);
plot_irfs_pub(vars, 'eps_z', 'Titles', ttl);
```

## Time series, simulations and transition paths: `plot_series_pub`

Not every task produces IRFs. A stochastic simulation (`stoch_simul(periods=T)`) and a perfect
foresight transition path (`perfect_foresight_solver`) both go to `oo_.endo_simul`. Plot them with
`references/plot_series_pub.m`, which has the same interface and style as `plot_irfs_pub`. In a run
script, call it. Do not write inline `plot` code.

```matlab
% Simplest: oo_/M_ in the workspace, plot these paths from endo_simul
plot_series_pub({'y','c','k'}, 'Save','fig_sim');

% Deviation from the steady state (subtract oo_.steady_state; Scale=100 for log variables)
plot_series_pub({'y','c'}, 'Center','ss', 'Scale',100, 'Save','fig_dev');

% Transition paths of several scenarios (for example baseline and reform)
plot_series_pub({'y','c'}, 'Scenarios',{ooA,ooB}, ...
    'ScenarioNames',{'Baseline','Reform'}, 'Save','fig_transition');

% Label periods so that the first simulated period is 1
plot_series_pub({'y','c'}, 'Time', (1:size(oo_.endo_simul,2)) - M_.maximum_lag);

% Demo figure from synthetic data, to check the style first
plot_series_pub
```

Options specific to `plot_series_pub`:

| Option | Effect | Default |
|---|---|---|
| `Source` | field of `oo_` that holds the [endo × T] path | `endo_simul` |
| `Center` | `none` (levels), `ss` (subtract `oo_.steady_state`), `mean` (subtract the sample mean) | `none` |
| `Scale` | multiplier | 1 |
| `Time` | x-axis values; give at least as many as plotted periods | `1:T` |
| `YLabel` | axis title | from `Center`: `Level`, `Dev. from SS` (`Percent dev. from SS` when `Scale` is 100), `Dev. from mean` |
| `XLabel` | axis title | `Period` |
| `ZeroLine` | zero line | on when centered |

`Horizon`, `Layout`, `Titles`, `Interpreter`, `Save`, `Formats`, `Font`, `FontSize`, `Grid`,
`Colors`, `LineStyles`, `LineWidth` and `FigSize` work as in `plot_irfs_pub`. Titles follow the same
rules.

Facts about the data:

- `oo_.endo_simul` also holds the initial and terminal conditions. The first simulated period is in
  column `1+M_.maximum_lag`, and there are `M_.maximum_lag+periods+M_.maximum_lead` columns (manual,
  `oo_.endo_simul`). Use `Time` or `Horizon` if the paper shows only the simulated periods.
- `Center='ss'` subtracts `oo_.steady_state`. The `steady` command fills that field. After
  `stoch_simul` without `steady;`, it still holds the `initval` values (zero for variables without
  `initval`), and the steady state is only in `oo_.dr.ys` (checked with Dynare 7.2). Put `steady;`
  before the experiment command when you center on the steady state.
- With `Scale=100`, the centered result is a percent deviation only for log variables. For a
  variable in levels, it is 100 times the deviation in levels, although the default label says
  `Percent dev. from SS`. Set `YLabel` in that case.

## Heterogeneous-agent models

After `heterogeneity_simulate`, Dynare 7.2 stores IRFs in `oo_.irfs` and simulated paths in
`oo_.endo_simul` (manual, `heterogeneity_simulate`, *Output*), so both functions apply. See
heterogeneity.md for the rest of the heterogeneous-agent workflow.

## Compatibility and cleanup

- Both functions need MATLAB. The full style needs R2020a or later (`tiledlayout`,
  `exportgraphics`, `yline`). Older releases fall back to `subplot`, `print` and a drawn zero line
  (plainer style, still usable). The functions are not tested under Octave: they call `gobjects` and
  set graphics properties with dot notation (`yl.Color = …`, `pa.Annotation…`), which can fail there.
  Under Octave, skip the plots and report it.
- During cleanup, keep `plot_irfs_pub.m`, `plot_series_pub.m` and the exported `fig_*.pdf/.eps/.png`:
  they are deliverables. Dynare's own outputs (`<model>/graphs/<model>_IRF_*`) can be removed. See
  the cleanup lists in workflow-detail.md and SKILL.md "Report".
