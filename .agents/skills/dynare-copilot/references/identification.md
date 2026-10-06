# Identification and sensitivity

Read this when the task asks whether parameters are identified, asks for a check before estimation, or
asks for global sensitivity analysis (GSA), a stability mapping, Morris or Sobol indices, or a prior
restricted by IRFs or moments. Commands: `identification` and `sensitivity`. Blocks:
`irf_calibration`, `moment_calibration`. Run these before a long Metropolis–Hastings chain
([estimation.md](estimation.md)).

If a parameter, or a combination of parameters, does not move the likelihood or the moments on its
own, the posterior stays on the prior and the chain drifts. Dynare has two tools.
`identification` is local and analytical (Iskrev; Ratto–Iskrev). `sensitivity` is global (Ratto:
Monte Carlo filtering and variance decomposition).

The results are numerical. Dygnosis does not compute identification strength. It does check the
written form: `identification` order not in 1..3 is E236; `max_dim_cova_group` of 0 is E237.

## 1. `identification`: local identification

```dynare
identification(
   parameter_set = prior_mean,   // calibration | prior_mean (default) | prior_mode |
                                 // posterior_mean | posterior_mode
   prior_mc = 2000,              // draws over the prior; a value above 1 runs the Monte Carlo
   advanced = 1,                 // collinearity, pairs, weakest direction
   order = 1,
   no_identification_strength    // skip the slower strength calculation
);
```

Read `oo_.identification`:

- **Identification strength:** how far the data moves each parameter. Near 0 means weak or not
  identified.
- **Collinearity**, including pairwise: parameters whose effects cannot be separated.
- Parameters diagnosed as not identified, and the weakest direction (a combination).
- With `advanced=1`, several Jacobian checks (moments, spectrum, minimal system).

Look at `prior_mean` first. Then set `prior_mc` so the check is not only at one point.

## 2. `sensitivity`: global sensitivity analysis

Sample the prior or the parameter space and map parameters to stability, moments, IRFs or the
likelihood. The option selects the mapping:

```dynare
// (a) Stability mapping: Blanchard-Kahn regions
sensitivity(stab = 1, redform = 0, Nsam = 2048, pprior = 1);

// (b) Reduced form: parameters to selected oo_ coefficients or moments
sensitivity(redform = 1, morris = 1);     // morris=1 screens; morris=0 with Sobol decomposes variance

// (c) Fit to data
sensitivity(rmse = 1, datafile = 'data.csv');

// (d) With identification
sensitivity(identification = 1, morris = 2);
```

- `pprior=1` draws from the prior. `pprior=0` draws from the posterior or a given range.
- `Nsam` is the sample size.
- `stab`, `redform`, `rmse` and `identification` select the analysis.
- `morris` screens; Sobol decomposes variance. `ppost` uses posterior draws.
- Figures (scatter, CDF, stability map, Sobol indices) go in the `GSA/` folder.

`dynare_sensitivity` is the old name of `sensitivity`. Write `sensitivity`.

## 3. `irf_calibration` and `moment_calibration`

These blocks set an implicit prior: a draw whose IRFs or moments miss the stated shape has prior
density 0. Sensitivity analysis uses the same blocks to mark an acceptable region.

```dynare
moment_calibration;
   y, y(-1), [0.4 0.7];          // first autocorrelation of y in [0.4, 0.7]
   y, eps_z, 1:4, +;             // variance contribution of eps_z to y in periods 1..4 is positive
end;

irf_calibration;
   y, eps_g, 1:4, [1 3];         // IRF of y to eps_g in periods 1..4 lies in [1, 3]
end;
```

A row is `variable, shock or variable, period or range, interval or sign`. After these blocks the
prior no longer integrates to 1. That changes `model_comparison` and the marginal likelihood. The
manual states this.

## Before a long chain

1. `identification(prior_mc=2000, advanced=1)`. Drop or reparameterize weakly identified parameters.
2. `sensitivity(stab=1)`. Most prior draws should satisfy the Blanchard–Kahn conditions. Otherwise the
   chain rejects often.
3. Optional: `sensitivity(redform=1, morris=1)` to see which parameters move the moments you care about.
4. Then `estimation(…, mh_replic=…)` with a large number of draws.

## Numerical run

Run official Dynare (`SKILL.md` "Numerical runs"). Start with a small `prior_mc` or `Nsam`. `order=2`
can identify more parameters and is slower; look at `order=1` first. Read `oo_.identification`. GSA
figures are in `GSA/`.

## Course examples

Folder: `references/examples-code/Dynare_Course/Chapter_07_Identification/`.
Search: `grep -iE "identification|no_identification|varobs" references/catalog-code.csv`.

| File | What it shows |
|---|---|
| `cochrane_toy.mod` | A small Fisher-equation plus Taylor-rule model that is not identified. Read the diagnostic output. |
| `forward_looking_varobs_x.mod` | The same three-equation model; identification changes with `varobs` |
| `RBC_bayesian.mod` | `identification(advanced=1)` on an RBC model, before and after estimation |

Read `cochrane_toy.mod` before a long chain, and check that your `varobs` can identify the parameters
you estimate (`forward_looking_varobs_x.mod`).

Also: `Smets_Wouters_2007` in the programming library. Manual: "Sensitivity and identification
analysis". Check options with `dynare_list_options`.
