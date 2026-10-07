# Markov-switching SBVAR (MS-SBVAR)

Read this when the task uses regime switching, a structural BVAR (Sims–Waggoner–Zha), or time-varying
volatility or coefficients, with `markov_switching`, `svar`, `sbvar` or the `ms_*` commands. This file
covers how that command family is organized, what each `ms_*` command does, and the declaration order.
It is a structural BVAR, not a DSGE in a `model` block. Dynare's Markov-switching commands are for
SBVAR. A Markov-switching DSGE needs an external toolbox such as RISE.

MS-SBVAR estimates a structural BVAR whose intercept, coefficients or shock variances can switch with a
Markov chain (Sims–Waggoner–Zha). The commands are a separate workflow from `stoch_simul` and
`estimation`. A typical file:

```dynare
var y pi r;                 // endogenous variables of the VAR
varexo e_y e_pi e_r;        // shock names; identification is structural

// 1) Structural identification — before sbvar / ms_sbvar
svar_identification;
   exclusion lag 0;
   restriction ...;         // exclusions or linear restrictions by equation and lag
end;

// 2) Markov chains (several chains may control coefficients and variances separately)
markov_switching(chain=1, number_of_regimes=2, duration=2.5,
                 restrictions=[[1,2,0.1],[2,1,0.1]]);
markov_switching(chain=2, number_of_regimes=3, duration=[2,4,4]);

// 3) Which part of the SBVAR each chain controls
svar(coefficients, chain=1, equations=[1:3]);   // chain 1 switches coefficients
svar(variances,    chain=2);                     // chain 2 switches shock variances

// 4) Estimate, then diagnostics, IRFs and forecasts
ms_estimation(file_tag='m1', mh_replic=10000, drop=1000);
ms_compute_probabilities(file_tag='m1');
ms_compute_mdd(file_tag='m1');
ms_irf(file_tag='m1', horizon=24);
ms_forecast(file_tag='m1', forecast=12);
ms_variance_decomposition(file_tag='m1', horizon=24);
```

## `markov_switching`: declare one chain

```dynare
markov_switching(chain = 1,
                 number_of_regimes = 3,
                 duration = 2.5,                       // scalar, or one duration per regime
                 restrictions = [[1,3,0],[3,1,0]]);    // transition restrictions
```

- `chain`: chain number. Several chains are independent (coefficients, variances, constants).
- `number_of_regimes` and `duration` (mean duration; Dynare turns it into a prior on transition
  probabilities).
- `restrictions=[[current regime, next regime, probability], …]` fixes some transition probabilities.
  If you give every transition out of a regime, they must sum to 1. A partial list must sum to less
  than 1.
- `parameter` and `number_of_lags` belong to Markov-switching DSGE setups. Do not use them for SBVAR.

Dygnosis reports malformed `markov_switching` and `svar` statements (E345–E371). Run
`dynare_diagnose` before a numerical run. The estimate, the marginal data density and the regime
probabilities are numerical results. Dygnosis does not compute them.

## `svar` and `sbvar`: attach a chain

`svar(...)` says which chain switches which part, and on which equations:

- `svar(coefficients, chain=k[, equations=…])`: lag coefficients follow chain k.
- `svar(variances, chain=k[, equations=…])`: structural shock variances follow chain k.
- `svar(constants, chain=k[, equations=…])`: the intercept follows chain k.

The `svar_identification` block (SWZ identification: exclusions and linear restrictions by lag and
equation) must be declared **before** `sbvar` or `ms_sbvar`.

## `ms_*` commands

Use one `file_tag` (and `output_file_tag` when you name the output) across the commands of one run.

| Command | Role | Main options |
|---|---|---|
| `ms_estimation` | Estimate the SBVAR and the regimes (MCMC) | `file_tag`, `output_file_tag`, `mh_replic` (default 10000), `drop` (burn-in), `thinning_factor`, `adaptive_mh_draws` (default 30000 tuning draws), `save_draws` |
| `ms_simulation` | Draw from the posterior | Same `file_tag` |
| `ms_compute_mdd` | Marginal data density | Writes Müller and bridged log marginal data densities into `oo_.ms` |
| `ms_compute_probabilities` | Filtered and smoothed regime probabilities | Probability of each regime in each period |
| `ms_irf` | Regime-dependent IRFs | `horizon`; one of `filtered_probabilities`, `regime` or `regimes`; `percentiles` (default `[.16 .5 .84]`); `draws`; `parameter_uncertainty` |
| `ms_forecast` | Forecast | `forecast` (horizon, default 12); one of `regime`, `regimes` or `filtered_probabilities`; `percentiles` |
| `ms_variance_decomposition` | Variance decomposition | `horizon`; `filtered_probabilities` or `regime`; `percentiles` |

- `.eps` figures go under `<tag>/Output/`. Data files go under `<tag>/`.
- For IRFs, forecasts and variance decompositions, choose one regime view: `filtered_probabilities`
  (filtered probabilities at the end of the sample), `regime` (one regime) or `regimes` (every regime).
- `save_draws` writes the A0, Aplus, Q and Zeta draws to `draws_<tag>.out`. Load them in MATLAB with
  `load_flat_file.m`.

## Not a DSGE

- These commands do not read the equations of a `model` block. `var` and `varexo` only name the VAR
  variables and set the dimension.
- Dynare does not estimate a Markov-switching DSGE. Use RISE (Junior Maih) for that.
- Declare `svar_identification` before `sbvar` or `ms_sbvar`.

## Numerical run

Run official Dynare under MATLAB or Octave (`SKILL.md` "Numerical runs"). MS-SBVAR uses the
`switch_dw` mex: the Dynare install must include it, and the data file must be in place. Keep one
`file_tag` from estimation through probabilities, the marginal data density, IRFs and the forecast.
Start with a small `mh_replic` to check the setup, then raise it. Read `oo_.ms`.

Examples: `<dynare-root>/examples/` and Dynare's `tests/ms-sbvar`. Manual: "Markov-switching SBVAR".
Check option names with `dynare_list_options` and that manual section. Do not invent an option.
