# Method of moments (GMM, SMM, IRF matching)

Read this when the task estimates parameters by matching moments or impulse responses: GMM, SMM
(simulated method of moments), or IRF matching, with `method_of_moments`. This file covers the
`matched_moments` and `matched_irfs` blocks, the option groups, the weighting matrix, and the choice
among the three methods. The `estimated_params` row syntax is in [estimation.md](estimation.md). The
steady state is in [steady-state.md](steady-state.md).

`method_of_moments` matches model moments to data moments, or model IRFs to empirical IRFs. It does
not use the likelihood. `mom_method` selects the method:

- **GMM:** analytical perturbation moments. Fast. The `order` must be high enough for those moments.
- **SMM:** moments from a simulated sample. Use it when the analytical moments are hard or the model
  is nonlinear.
- **IRF matching:** model IRFs against empirical IRFs, often from a structural VAR.

Order of the file: for GMM or SMM, `matched_moments`, then `estimated_params`, then
`method_of_moments`. For IRF matching, `matched_irfs` (and optionally `matched_irfs_weights`), then
`estimated_params`, then `method_of_moments(mom_method=irf_matching)`.

The optimizer recomputes the steady state at every draw. Write a `steady_state_model` block
([steady-state.md](steady-state.md)).

Dygnosis checks the written form, not the estimate. `method_of_moments` without `mom_method` is E382.
Without a data file, when one is required, is E383. A matched moment with an unsupported shape is
E386. A repeated `matched_irfs` pair is E388. Period and value counts that differ are E390.

## `matched_moments` (GMM and SMM)

Only product moments. Each row is variables, with optional lags.

```dynare
matched_moments;
   c;            // E[c]
   y y;          // E[y^2]
   c y;          // E[c*y]
   y y(-1);      // E[y_t * y_{t-1}]
   gy gy gy;     // E[gy^3]   (SMM; GMM only if order is high enough)
end;
```

- You cannot match a correlation directly. Match the two variances and the covariance.
- The number of moments must be at least the number of estimated parameters. Extra moments use the
  optimal weighting matrix.

## `estimated_params`

A method-of-moments row can be the maximum-likelihood form (no prior):
`name, initial [, lower, upper];`. A Bayesian-style row adds a prior. That prior is an extra penalty
(penalized method of moments):

```dynare
estimated_params;
   rho,          0.8,   0,  0.999;
   stderr eps_z, 0.01,  0,  ;
   // alppha,    0.3,  , , beta_pdf, 0.30, 0.05;
end;
```

## `method_of_moments`

The manual groups the options as required, common, SMM only, GMM only, IRF matching only, general,
data, optimization, Bayesian, and numerical algorithm.

```dynare
method_of_moments(
   mom_method = SMM,            // required: GMM | SMM | IRF_MATCHING
   datafile   = 'data.csv',     // required for GMM and SMM
   order      = 2,
   weighting_matrix = ['DIAGONAL','OPTIMAL'],
   weighting_matrix_scaling_factor = 1,
   bartlett_kernel_lag = 20,    // HAC (Newey-West) lag for the optimal weight
   simulation_multiple = 5,     // SMM: simulated length = data length times this
   burnin = 500,
   mode_compute = 13,           // 13 (lsqnonlin), 4 or 5 are common here
   additional_optimizer_steps = [4],
   nodisplay, graph_format = eps
);
```

- `mom_method` is required. GMM and SMM need `datafile`. IRF matching uses `matched_irfs` instead.
- Under GMM, `order` is the order of the analytical moments. Moments of order 3 and above usually need
  SMM. Under SMM, `order` is the perturbation order of the simulation.
- `weighting_matrix`: `IDENTITY`, `DIAGONAL`, `OPTIMAL`, or a `.mat` file name. A list runs iterated
  GMM (diagonal first, then optimal).
- `bartlett_kernel_lag` is the lag of the HAC kernel for the long-run variance.
- SMM also uses `seed` and `bounded_shock_support`.
- Optimizer and data options are shared with `estimation` (`mode_compute`,
  `additional_optimizer_steps`, `optim`, `first_obs`, `nobs`, `prefilter`, `logdata`). Use `logdata`
  together with `model(loglinear)` only when the whole model is logged. Prefer helper variables
  (`log_y = log(y);`) to logging every equation.
- Check every option with `dynare_list_options` before you write it. Do not invent one.

## IRF matching

```dynare
matched_irfs;
   var y;   varexo eps_r;  periods 1:8;  values (empirical_irf_y);
   var pi;  varexo eps_r;  periods 1:8;  values (empirical_irf_pi);
end;
matched_irfs_weights;
   y, eps_r, (W_y);
end;
method_of_moments(mom_method = irf_matching, order = 1, mode_compute = 13);
```

`values` is a column vector of the empirical IRF. Weights default to one.

## Output

Results are in `oo_.mom`: estimates, standard errors under the optimal weight, the J statistic and
the overidentification test, model moments against data moments, and the weighting matrix. Read
`oo_.mom` after the run. Dygnosis does not fill it.

## Which method

| Method | Moments from | Use when | Cost |
|---|---|---|---|
| GMM | Analytical perturbation | First and second moments; speed | Higher moments need a higher `order` |
| SMM | A simulated sample | Higher moments or strong nonlinearity | Slow; a larger `simulation_multiple` is more stable |
| IRF matching | Model IRFs against empirical IRFs | A credible structural-VAR response exists | Estimate that response and its weights first |

## Numerical run

Run official Dynare (`SKILL.md` "Numerical runs"). The data file must be on the path Dynare searches,
and column names must match, as for `estimation`. For SMM, start with a small `simulation_multiple`
and few optimizer steps. For iterated GMM use `weighting_matrix=['DIAGONAL','OPTIMAL']`.

Programming library: `Born_Pfeifer_2014` (GMM and SMM, with a recalibration switch),
`RBC_state_dependent_GIRF`. Official examples under `<dynare-root>/examples/` include `AnScho_GMM`
and `RBC_MoM` where that install has them.

## Course examples

Folder: `references/examples-code/Dynare_Course/Chapter_09_Method_of_Moments/`.
Search: `grep -iE "mom_method|method_of_moments|irf_matching" references/catalog-code.csv`.
Shared pieces: `RBC_MoM_common.inc` (pulled in with `@#include`), `RBC_MoM_steady_helper.m`,
`RBC_Data_2.mat`.

| File | Method | What to copy |
|---|---|---|
| `RBC_MoM_GMM_order2.mod` | GMM | `mom_method=GMM`, `order=2`, `mode_compute=4`, `additional_optimizer_steps=[13]`, `matched_moments` |
| `RBC_MoM_SMM_order2.mod` | SMM | Same moments; only `mom_method` changes |
| `rbc_irf_matching.mod` | IRF matching | Empirical IRFs as the target; diagonal weights are inverse IRF variances; an AR(2) reparameterized by its roots so the draw stays stable; `estimated_params(overwrite)` |

Diff the GMM and SMM files: they share `RBC_MoM_common.inc` and the moments. `rbc_irf_matching.mod`
shows two patterns the manual does not walk through: empirical IRFs as data, and estimating AR roots
instead of AR coefficients.

Manual: "Estimation based on moments".
