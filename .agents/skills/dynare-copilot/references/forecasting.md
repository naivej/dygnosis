# Forecasting

Read this when the task asks for a future path, an unconditional or conditional forecast, the path of
other variables given a path for one variable, or a fan chart. Commands: `forecast`,
`conditional_forecast`, `conditional_forecast_paths`, `plot_conditional_forecast`. A forecast needs a
stochastic solution first ([stochastic-simulation.md](stochastic-simulation.md)) or an estimation
([estimation.md](estimation.md)).

- **Unconditional forecast** (`forecast`): from the current state, follow the model. Report the mean
  path and a band.
- **Conditional forecast** (`conditional_forecast`): constrain some endogenous variables to a path you
  give (for example a path for the interest rate). Dynare solves for the shocks that hit that path and
  reports the other variables.

## 1. `forecast`: unconditional

Run `stoch_simul` or `estimation` first. The starting point is the steady state from `stoch_simul`,
or a `histval` block, or the last smoothed state from estimation.

```dynare
stoch_simul(irf=0);
forecast(
   periods = 20,
   conf_sig = 0.9              // coverage of the band; default 0.9
) y pi r;                      // default: all endogenous variables
```

- With `varexo_det` and a perfect-foresight path in `shocks`, `forecast` includes that known future
  path. See [stochastic-simulation.md](stochastic-simulation.md).
- Output is `oo_.forecast`: `Mean`, `HPDinf`, `HPDsup`, `Steady`.
- After estimation, `estimation(…, forecast=20)` also writes a posterior forecast.

## 2. Conditional forecast, in three steps

### (a) `conditional_forecast_paths`: the constrained path

```dynare
conditional_forecast_paths;
   var r;
   periods 1:4;
   values 0.01 0.0125 0.015 0.015;

   var pi;
   periods 1:2;
   values 0.02 0.02;
end;
```

One `var` / `periods` / `values` group per constrained endogenous variable. The number of constrained
endogenous variables must not exceed the number of shocks used to hit the path.

### (b) `conditional_forecast`: solve for the shocks

```dynare
conditional_forecast(
   parameter_set = calibration,   // calibration, or posterior_mean after estimation
   controlled_varexo = (eps_r, eps_pi),
   periods = 20,
   conf_sig = 0.8
) y c;
```

- `controlled_varexo`: the shocks Dynare is free to set so that the constrained path is hit. There
  must be at least as many as there are constrained endogenous variables.
- `parameter_set` uses the same names as shock decomposition: `calibration`, `posterior_mode`,
  `posterior_mean`, and the other documented values.
- Result: `oo_.conditional_forecast` (`.cond` and `.uncond`).
- A `conditional_forecast` without `parameter_set` is a Dynare refusal. Dygnosis reports it as E342.

### (c) `plot_conditional_forecast`: the fan chart

```dynare
plot_conditional_forecast(periods = 20) y c;
```

"Fan chart" is not a manual term. It means this plot: the conditional path and its band.

## Where the forecast starts

- The default start is the steady state. For a chosen historical state, set the state variables in
  `histval` ([steady-state.md](steady-state.md)).
- After estimation, the usual start is the last smoothed state. `estimation` connects that itself.
- In a `loglinear` model, `histval` still holds the **level**, not the log.

## Which command

| Situation | Command |
|---|---|
| Extrapolation and a band | `forecast` |
| A constrained path for some endogenous variables | `conditional_forecast` and `conditional_forecast_paths` |
| A deterministic path that agents foresee | `perfect_foresight_*`, or `varexo_det` with `forecast` ([perfect-foresight.md](perfect-foresight.md)) |

## Numerical run

Run official Dynare (`SKILL.md` "Numerical runs"). If `controlled_varexo` has fewer names than the
constrained endogenous variables, the inversion is underdetermined. After estimation, `parameter_set`
must match the posterior you loaded. Read `oo_.forecast` or `oo_.conditional_forecast`. Dygnosis does
not compute the forecast.

## Course examples

Folder: `references/examples-code/Dynare_Course/Chapter_10_forecasting/`.
Search: `grep -iE "forecast|conditional_forecast|smoother2histval" references/catalog-code.csv`.

| File | What it shows |
|---|---|
| `forecast_test_AR1.mod` | `forecast(periods, conf_sig)` on an AR(1): mean reversion and the band |
| `rbc_basic_forecast.mod` | Unconditional forecast; `histval` sets the starting state |
| `rbc_basic_forecast_varexo_det.mod` | `varexo_det` plus `forecast`: a known future deterministic path |
| `rbc_basic_calib_smoother.mod` | `calib_smoother`, then `smoother2histval`, then `forecast` |
| `rbc_basic_ML_forecast.mod` | `estimation(order=1, forecast=8)` |
| `rbc_basic_posterior_forecast.mod` | Posterior forecast, including parameter uncertainty in the band |
| `rbc_basic_recursive_estimation.mod` | Recursive forecast: re-estimate on an expanding window |
| `rbc_basic_pf_controlled_paths.mod` | A controlled future path with `perfect_foresight_*` |
| `NK_linear_controlled_forecast.mod` | Full conditional forecast: paths, `controlled_varexo`, plot |

Two patterns that are easy to miss:

1. **Start from the last smoothed state.** After `calib_smoother` (or estimation), `smoother2histval;`
   writes that state into `histval`. The forecast then continues the data instead of jumping from the
   steady state.
2. **Recursive forecast.** In MATLAB, loop `estimation(…, forecast=h)` on an expanding window. See
   `rbc_basic_recursive_estimation.mod`.

Also: `Smets_Wouters_2007` in the programming library (posterior forecast). Manual: "Forecasting".
Check options with `dynare_list_options`.
