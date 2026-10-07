# Estimation (Bayesian and maximum likelihood)

Read this when the task estimates a model: Bayesian estimation, priors, Metropolis–Hastings, maximum
likelihood, or data, with `estimation`. The steady state is in [steady-state.md](steady-state.md).
Run [identification.md](identification.md) before a long chain.

Dynare estimates parameters from data on observed variables with the Kalman filter: maximum
likelihood, or Bayesian estimation with Metropolis–Hastings (MCMC).

**Stochastic singularity:** the number of observed variables must not exceed the number of shocks
plus measurement errors, or the likelihood is degenerate. Dygnosis W092 is only that static count.
The singularity itself appears when Dynare runs.

Order in the file: `varobs`, optional `observation_trends`, `estimated_params`, `estimation(…)`.
Dynare recomputes the steady state at every parameter draw. Write a `steady_state_model` block.

Dygnosis checks the written form. It does not compute a posterior. Relevant codes: E227 (no data
file), E090 (an observed variable is not endogenous), E258 (more than one `varobs`), E244–E248
(duplicates and values used inside `estimated_params`). Check option names with `dynare_list_options`.

## Observed variables and observation equations

```dynare
varobs y_obs pi_obs r_obs;   // names must match the data columns
```

- One `varobs` in the file. Each name is an endogenous variable and matches a data column.
- An **observation equation** maps a model variable to the data. This is the usual source of a wrong
  estimate: the mean, the trend, per-capita scaling and the growth rate must match on both sides
  (Pfeifer's observation-equation notes).

1. Prefer a **growth rate**. Its steady state is 0, and it does not depend on how you detrend:

   ```dynare
   [name='output growth observation eq.']
   y_obs = log_y - log_y(-1);     // helper variable: log_y = log(y);
   ```

   In `steady_state_model`, `y_obs = 0;`. `log_y` is a helper variable, not a Dynare `AUX_*`
   auxiliary variable.

2. Match definitions. If the data are per capita, quarterly and real, the model variable is the same
   object. Put a constant such as steady-state growth in the equation:
   `y_obs = log_y - log_y(-1) + gamma;`.

3. `prefilter=1` demeans each series. Use it for levels whose steady state is not zero. A growth-rate
   observation equation usually does not need it.

4. `loglinear` and `logdata` go together. `loglinear` logs the model and the data (the data are then
   levels, and the steady state is strictly positive). If the data are already logs, add `logdata`.

## `estimated_params` and priors

```dynare
estimated_params;
// name, initial, lower, upper, prior shape, prior mean, prior std [, p3, p4];
   alppha,        0.30,           ,        ,  beta_pdf,      0.30,    0.05;
   rhoz,          0.80,    0,      0.999,     beta_pdf,      0.80,    0.10;
   stderr eps_z,  0.01,    0,      ,          inv_gamma_pdf, 0.01,    inf;
   corr eps_z, eps_g,  0,  -1, 1,             normal_pdf,    0,       0.2;
end;
```

The first token selects what is estimated:

- A parameter name.
- `stderr name`: the standard deviation of an exogenous shock, or the measurement error of an
  observed endogenous variable.
- `corr name1, name2`: a correlation. The bounds are [-1, 1].
- `skew name`: skewness (the shock is skew normal).

After the name:

- Maximum likelihood or method of moments: `name, initial [, lower, upper];`
- Bayesian MCMC: `name [, initial [, lower, upper]], prior shape, prior mean, prior std [, p3 [, p4 [, scale]]];`
- `initial` is the optimizer start. If you omit it, Dynare uses the prior mean.
- Under Bayesian estimation, `lower` and `upper` constrain the mode finder only. They do not change
  the prior, unless `p3` and `p4` shift the support.

Prior shapes: `beta_pdf`, `gamma_pdf`, `normal_pdf`, `uniform_pdf`, `inv_gamma_pdf`
(`inv_gamma1_pdf`), `inv_gamma2_pdf`, `weibull_pdf`.

Use `beta_pdf` for parameters in (0, 1) (autocorrelations, shares). Use `inv_gamma_pdf` for standard
deviations. Use `normal_pdf` or `gamma_pdf` for unbounded or positive structural parameters. For a
generalized beta on a correlation over [-1, 1], set `p3=-1` and `p4=1`.

Related blocks, same row syntax:

- `estimated_params_init;` overrides the optimizer start. Put it after `estimated_params`.
  `use_calibration` seeds it from the calibration. Together with `mode_file` this is E228.
- `estimated_params_bounds;` sets bounds for maximum likelihood.
- `estimated_params_remove;` drops a parameter (modular files).
- A later `estimated_params(overwrite);` replaces the whole list.

To estimate a transformation, keep the parameter in `parameters`, define the transformation with a
model-local variable (`#`), and put the parameter in `estimated_params`.

## `estimation`

```dynare
estimation(
   datafile = 'mydata.csv',   // .m .mat .csv .xls .xlsx; basename must differ from the .mod name
   first_obs = 1,
   nobs = 200,
   mode_compute = 5,
   mh_replic = 20000,         // 0: mode only, no MCMC
   mh_nblocks = 2,            // at least 2 enables the Brooks-Gelman diagnostic
   mh_jscale = 0.3,           // tune so the acceptance rate is about one quarter to one third
   mh_drop = 0.5,
   order = 1
) y_obs pi_obs r_obs;
```

Confirm each number with `dynare_list_options` for `estimation`. The groups:

- Data: `datafile`, `first_obs`, `nobs`, `xls_sheet`, `xls_range`, `presample`, `prefilter`,
  `loglinear`, `logdata`.
- Mode finding: `mode_compute`, `mode_file` (`mode_compute=0` loads a mode file and skips the
  optimizer), `mode_check`.
- MCMC: `mh_replic`, `mh_nblocks`, `mh_jscale`, `mh_drop`, `mh_init_scale`, `load_mh_file`.
- Output: `bayesian_irf`, `moments_varendo`, `conditional_variance_decomposition`, `forecast`,
  `smoother`, `filtered_vars`.

The default chain is deterministic (seed 0). For a different proposal sequence across runs, put
`set_dynare_seed('clock');` before `estimation`.

`mh_jscale` scales the proposal. The course files show the direction:

| `mh_jscale` | Step | Acceptance rate | What you see |
|---|---|---|---|
| Too small (0.2) | Short | Too high | The chain moves slowly; autocorrelation is high |
| In range (about 1.5 in that example) | Medium | About 25–35% | The chain mixes |
| Too large (3.5) | Long | Too low | Draws are rejected; the chain sticks |

Read `Acceptance ratio` on a short run, then change `mh_jscale` in the opposite direction. Dynare can
tune it with `mh_tune_jscale`. `mh_tune_jscale` together with `mh_jscale` is E229.

## Measurement error

After `varobs`, either in `shocks`:

```dynare
var y_obs; stderr 0.001;
```

or in `estimated_params` as `stderr y_obs`. A `heteroskedastic_shocks` block changes a shock standard
deviation by period (`values`, or `scales` as `std0*scale(t)`). It is incompatible with
`analytic_derivation`.

## Mixed deterministic and stochastic shocks

Declare a deterministic exogenous variable (`varexo_det tau;`), give its path in a perfect-foresight
`shocks` block, then `stoch_simul(irf=0); forecast;`. Agents know that future path from the start.
Do not put `perfect_foresight_solver` in the same file as `stoch_simul` (E205).

## Numerical run

Run official Dynare (`SKILL.md` "Numerical runs"). The data file must be where Dynare looks, and the
column names must match `varobs`. For a first check use `mh_replic=0` or a small `mh_replic`. Read
`oo_.posterior_mode`, `oo_.posterior_mean`, `oo_.posterior_hpdinf`, `oo_.posterior_hpdsup` and
`oo_.MarginalDensity`. Those fields exist only after a successful Dynare run.

The preprocessor option `params_derivs_order=0|1|2` sets the order of derivatives with respect to
parameters (used by identification and estimation).

## File skeleton

```dynare
var ...; varexo ...; parameters ...;
// assign parameters that are not estimated; estimated parameters need a starting value
model;
   ...
   [name='output growth observation eq.'] y_obs = log_y - log_y(-1);
   ...
end;
steady_state_model; ... y_obs = 0; ... end;
steady; check;

varobs y_obs pi_obs r_obs;

estimated_params;
   ...
end;

estimation(datafile='data.csv', mh_replic=20000, mh_nblocks=2, mode_compute=5)
   y_obs pi_obs r_obs;
```

## Course examples

Folders: `references/examples-code/Dynare_Course/Chapter_05_Kalman_ML/` and `Chapter_06_Bayesian/`.
The same RBC model runs through both. Data file: `first_diff.mat` (first-difference observations).
Search: `grep -iE "calib_smoother|estimation|mh_jscale|estimated_params" references/catalog-code.csv`.

| File | What it shows |
|---|---|
| `Chapter_05_Kalman_ML/RBC_smoother.mod` | `calib_smoother` on a calibrated model; `steady_state_model` reverse-solves `betta`, `delta` and `psi` to target ratios |
| `Chapter_06_Bayesian/RBC_Bayesian.mod` | Full Metropolis–Hastings: `estimated_params`, `estimated_params_init(use_calibration)`, `smoother`, `bayesian_irf`, `forecast=8` |
| `Chapter_06_Bayesian/RBC_high_acceptance.mod` (and `medium`, `low`) | The same model; only `mh_jscale` changes (0.2, 1.5, 3.5) |

`calib_smoother` plus `smoother2histval` is also how a forecast starts from the last smoothed state
([forecasting.md](forecasting.md)).

Programming library: `Smets_Wouters_2007` (a medium-scale estimation), `Ireland_2004` (maximum
likelihood), `RBC_baseline_first_diff_bayesian` (a small Bayesian model with growth-rate observation
equations). `GarciaCicco_et_al_2010` is in Pfeifer's DSGE_mod on GitHub; it is not in the local
programming library.

Manual: "Estimation based on likelihood".
