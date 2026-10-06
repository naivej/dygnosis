# Shock decomposition

Read this when the task asks which structural shocks drove a historical path, a recession or inflation,
or asks for a realtime decomposition or an initial-condition decomposition. Commands:
`shock_decomposition`, `realtime_shock_decomposition`, `plot_shock_decomposition`,
`initial_condition_decomposition`. This file covers those commands, the `shock_groups` block,
`parameter_set` and the plots. You usually need smoothed shocks first, from `estimation`
([estimation.md](estimation.md)) or `calib_smoother`.

A shock decomposition splits each endogenous variable in each period into the contribution of the
initial condition, the cumulative contribution of each structural shock, and a smoothed residual. It
answers which shocks drove that history. The smoother (Kalman smoother) recovers the historical
shocks, so the run needs data and a parameter set: a posterior, a mode, or calibrated values through
`calib_smoother`.

## 1. `shock_decomposition`: a realized sample

```dynare
shock_decomposition(
   parameter_set = posterior_mean,   // calibration | prior_mode | posterior_mode |
                                     // posterior_mean (default) | posterior_median | mle_mode
   datafile = 'data.csv',            // required when estimation was not run first
   first_obs = 1, nobs = 200,
   use_shock_groups = monpol,        // name of a shock_groups block
   colormap = 'jet',
   nograph                           // compute only; plot later
) y pi r;                            // variables to decompose; default: all endogenous
```

- `parameter_set` chooses the parameters for the smoother. After estimation use `posterior_mean` or
  `posterior_mode`. For a calibrated model use `calibration` (run `calib_smoother` first, or let this
  command smooth).
- Results are in `oo_.shock_decomposition` (variable × (number of shocks + 2) × time). The last two
  slices are the initial condition and the smoothed residual or constant.
- With many variables, compute with `nograph` and plot with `plot_shock_decomposition`.

## 2. `shock_groups`: group the shocks

Group shocks by economic meaning (demand, supply, policy) so the figure is readable:

```dynare
shock_groups(name = monpol);        // referenced as use_shock_groups=monpol
   'Monetary policy'  = eps_r;
   'Demand'           = eps_g, eps_b;
   'Supply'           = eps_z, eps_mu;
end;
```

Each line is `'display name' = shock1, shock2, …;`. Shocks you do not list go into a residual group.

## 3. `realtime_shock_decomposition`

This repeats the decomposition at each date using only the data available then, so you can see how the
decomposition is revised as data arrive (forecast, realtime and pointwise views):

```dynare
realtime_shock_decomposition(
   parameter_set = posterior_mean,
   forecast = 8,                 // periods forecast from each vintage
   save_realtime = [60 80 100]   // vintages whose decomposition is saved
) y;
```

Output: `oo_.realtime_shock_decomposition`, `oo_.realtime_forecast_shock_decomposition` and related
fields.

## 4. `initial_condition_decomposition`

This splits the contribution of the initial state across state variables: which initial states drive
the later path.

```dynare
initial_condition_decomposition(
   nograph
) y k;
```

Result: `oo_.initval_decomposition`.

## 5. `plot_shock_decomposition`

Compute first with `shock_decomposition(nograph)`, then plot:

```dynare
plot_shock_decomposition(
   use_shock_groups = monpol,
   type = qoq,             // aoa | yoy | qoq
   detail_plot,            // one panel per shock; default is a stacked group chart
   fig_name = 'crisis'
) y pi;
```

Also used: `realtime` and `vintage` (with a realtime decomposition), and `steadystate` (overlay the
steady state).

## Not a variance decomposition

- A **shock decomposition** attributes the realized value in a sample to shocks. It needs data and a
  smoother.
- A **variance decomposition** (`conditional_variance_decomposition` of `stoch_simul`, or
  `oo_.variance_decomposition`) is the share of each shock in unconditional or conditional variance. It
  is not about one history. The two answers are different.

## Numerical run

Run official Dynare (`SKILL.md` "Numerical runs"). Have a parameter set first: a posterior after
estimation, or `calib_smoother(datafile=…)` for a calibrated model. Data column names must match
`varobs`. Check that the contributions in `oo_.shock_decomposition` add up (shocks plus the initial
condition are close to the realized value).

`dynare_list_options` lists the options of these commands. An observed variable that is not endogenous
is E090. More observed variables than shock sources is W092, a static count, not a smoother result.
Dygnosis does not compute the decomposition.

Reference implementations: `Smets_Wouters_2007` in the programming library
(`references/catalog-code.csv`) for a historical and a realtime decomposition after estimation.
Manual: "Shock Decomposition".
