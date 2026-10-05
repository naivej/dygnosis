# Stochastic simulation (`stoch_simul`)

Read this when the task asks for IRFs, moments, variance decompositions or decision rules (policy
functions) of a rational-expectations model solved by perturbation with `stoch_simul`. This file
covers the stochastic `shocks` block, the `stoch_simul` options, where Dynare stores the results, and
nonstationary or mixed-shock setups. For `order=2` or `3` (risk, asset pricing, GIRFs), also read
[higher-order.md](higher-order.md).

`stoch_simul` takes a Taylor approximation of the model around the **deterministic steady state**,
solves for the decision rules, and reports the decision rules, theoretical or simulated moments,
variance decompositions and impulse response functions (IRFs). It is the main command for RBC, New
Keynesian and other DSGE models under rational expectations.

One file is one context. A file with `stoch_simul` cannot also contain `perfect_foresight_solver`,
`perfect_foresight_with_expectation_errors_solver` or `simul`: Dynare refuses the mix (Dygnosis
E205). For deterministic paths, see [perfect-foresight.md](perfect-foresight.md).

## Shocks block (stochastic context)

In a stochastic context, the `shocks` block sets the **covariance matrix of the exogenous
innovations**, not a time path:

```dynare
shocks;
    var eps_a;            stderr 0.0072;     // standard deviation
    var eps_g          =  0.0052^2;          // variance (= standard deviation squared)
    corr eps_a, eps_g  =  0.3;               // correlation
end;
```

- `stderr` sets the standard deviation. `var x = …;` sets the **variance**. `var x, y = …;` sets a
  covariance.
- `skew x = …;` sets a skewness coefficient (`skew u, v, w = …;` a coskewness). The shocks follow a
  skew normal distribution; with no `skew` entry, all skewness is zero (Gaussian).
- **Nonlinear model (R8 default): write the standard deviation as a fraction.** A 1% shock is
  `stderr 0.01`; do not multiply by 100. Write `stderr 1` for 1% only in a linearized model whose
  variables are percent deviations. A wrong scale gives wrong uncertainty correction terms in
  higher-order approximations.
- A variance of 0 removes a shock from the moments and IRFs. The variable stays in the model.
- With correlated shocks, the variance decomposition uses a Cholesky decomposition. The result
  **depends on the declaration order in `varexo`**.
- IRFs are computed only for shocks with a nonzero variance. Dygnosis W060 reports an IRF request
  with no written shock size. W120 reports a stochastic command when no stochastic exogenous
  variable is declared. Run `dynare_diagnose` ([dygnosis-workflow.md](dygnosis-workflow.md)).

## `stoch_simul` options

The list below gives the options that this skill uses, with the Dynare 7.2 defaults. To list all
options of a command, use `dynare_list_options`.

- `order = 1|2|3`: order of the Taylor approximation. Default 2 (after `estimation`, the order of
  the estimation). `order>=3` implies `k_order_solver`. At `order=3`, theoretical moments need
  `pruning`; without `pruning`, set `periods>0` for simulated moments.
- `irf = INTEGER`: number of IRF periods. Default 40. `irf=0` turns IRFs off (no `oo_.irfs`).
- `irf_shocks = (eps_a, eps_g)`: IRFs only for the listed shocks.
- `relative_irf`: at first order, the response to a shock of size 1 (not one standard deviation),
  multiplied by 100. For a loglinearized model in percent, this is the response to a 1% shock. At
  `order>1`, the impulse is 0.01 instead.
- `irf_plot_threshold = DOUBLE`: IRFs with a maximum absolute deviation below this value are not
  plotted. Default `1e-10`.
- `periods = INTEGER`: if larger than 0, Dynare simulates the model for this number of periods and
  reports **simulated moments** instead of theoretical moments. Default 0. The simulation starts
  from the `initval` values (recomputed by `steady` if the file has `steady`), or from `histval`.
- `drop = INTEGER`: burn-in periods dropped before simulated moments are computed. Default 100.
- `replic = INTEGER`: number of simulated series for IRFs. Default 1 at `order=1` (where it is not
  used), 50 at `order>1`. `simul_replic = INTEGER`: number of simulated series for `periods>0`.
  Default 1.
- `pruning`: removes higher-order terms that accumulate in simulations. Strongly recommended from
  `order=2`, to prevent explosive paths. With `pruning`, theoretical moments use the pruned state
  space (Andreasen et al. 2018). Without it, second-order moments are a second-order accurate
  approximation that uses the linear terms (Kim et al. 2008).
- `hp_filter = 1600`, `one_sided_hp_filter = DOUBLE`, `bandpass_filter` or
  `bandpass_filter = [6 32]`: filter before moments are computed. Use only one filter (Dygnosis
  E238). `one_sided_hp_filter` works only with simulated moments.
- `ar = INTEGER`: order of the autocorrelations. Default 5.
- `nograph`, `nodisplay`, `graph_format = eps|pdf|fig|none`: graph control. The default format is
  `eps`. `graph_format=(pdf, fig)` writes more than one format.
- `nomoments`, `nocorr`, `nodecomposition`, `nofunctions`, `noprint`: suppress printed output.
- `loglinear`: takes the log of **all** variables (all steady-state values must be strictly
  positive). Moments, IRFs and decision rules are then for the log variables.
- `conditional_variance_decomposition = [1 4 8]`: conditional variance decomposition at the listed
  horizons. Only with theoretical moments (`periods=0`), `order<3` and no `pruning`.
- `contemporaneous_correlation` (stores `oo_.contemporaneous_correlation`), `spectral_density`
  (stores `oo_.SpectralDensity`).
- `partial_information`: solution under partial information. Agents observe only the variables
  listed in `varobs`.
- `dr = default|cycle_reduction|logarithmic_reduction|aim`: algorithm for the decision rules.
  `cycle_reduction` is faster for large models. `aim` is valid only at first order.
- `solve_algo`, `qz_criterium` (default `1.000001`), `k_order_solver`, `tex`: advanced control.

## Results

- `oo_.dr`: decision rules (structure below).
- `oo_.mean`, `oo_.var`, `oo_.autocorr`, `oo_.gamma_y`: moments. They are theoretical with
  `periods=0` and simulated with `periods>0`. Simulated moments also give `oo_.skewness` and
  `oo_.kurtosis`.
- `oo_.irfs.<variable>_<shock>`: IRFs (for example `oo_.irfs.y_eps_a`). `get_irf('eps_a', 'y')`
  returns the same series.
- `oo_.endo_simul`, `oo_.exo_simul`: simulated series (when `periods>0`).
- `oo_.variance_decomposition`: unconditional variance decomposition. Dynare computes it when
  theoretical moments are requested and `nodecomposition` is not set. With measurement errors, also
  `oo_.variance_decomposition_ME`.
- `oo_.conditional_variance_decomposition` (and `oo_.conditional_variance_decomposition_ME`).

Read these fields in MATLAB or Octave after the run, through the execution route that the host
offers (MATLAB MCP, `matlab -batch "…"` or `octave --eval "…"`; see
[matlab-workflow.md](matlab-workflow.md)). Do not infer moments, IRFs or the steady state from
static checks. If no route exists, say what you could not run and give the exact commands.

## Decision rule structure `oo_.dr`

First order: `y_t = y^s + A·y^h_{t-1} + B·u_t`, where `y^h = y - y^s` is the deviation from the
steady state.

- `oo_.dr.ys`: steady state (declaration order).
- `oo_.dr.ghx` = A. Rows: all endogenous variables in DR order. Columns: state variables in DR
  order.
- `oo_.dr.ghu` = B. Rows: endogenous variables in DR order. Columns: exogenous variables in
  declaration order.
- Second order adds `ghs2` (the shift Δ² from the variance of future shocks), `ghxx`, `ghuu` and
  `ghxu`.
- At third order, Dynare stores `g_0`, `g_1`, `g_2` and `g_3` in folded form: each unique symmetric
  element is stored once. To decode, multiply each off-diagonal element of `g_2` by 2. In `g_3`,
  multiply an element with three different indices by 6, and an element with exactly two equal
  indices by 3.

## Variable types and orderings (Blanchard-Kahn diagnosis)

- Four types: purely backward `M_.npred`, purely forward `M_.nfwrd`, mixed `M_.nboth`, static
  `M_.nstatic`. Their sum is `M_.endo_nbr`. Dynare counts them after it adds its auxiliary variables
  (`AUX_*`). State variables are the purely backward and the mixed variables: `M_.nspred`.
- Two orderings: the **declaration order** (`M_.endo_names`) and the **DR order** (static, then
  purely backward, then mixed, then purely forward; declaration order inside each group). All
  decision-rule elements use the DR order. `oo_.dr.order_var` maps DR order to declaration order;
  `oo_.dr.inv_order_var` is the inverse.
- Blanchard-Kahn conditions: the number of eigenvalues larger than one in modulus must equal the
  number of forward-looking variables, and a rank condition must hold. `check` stores the
  eigenvalues in `oo_.dr.eigval`.
- When the conditions fail, compare the timing classes before you run more code. Dygnosis
  `dynare_model_info` reports the classes of the written model (before Dynare adds `AUX_*`
  variables). The Dynare command `model_info;` reports Dynare's own classification at run time.
  Timing rules: SKILL.md "Writing rules" R2. Fix steps: debugging.md "Run-and-fix loop".

## Canonical example: basic RBC (Pfeifer style)

Points to copy: the AR process is an endogenous variable (`z` endogenous, `eps_z` exogenous; R3);
closed-form `steady_state_model`; `[name=…]` equation tags, `long_name` and TeX names (R1); `log_*`
helper variables; the sequence `resid; steady; check;`. The full reusable skeleton is in
[templates.md](templates.md).

```dynare
var y ${y}$ (long_name='output')  c ${c}$ (long_name='consumption')
    k ${k}$ (long_name='capital')  l ${l}$ (long_name='labor')
    invest ${i}$ (long_name='investment')  z ${z}$ (long_name='TFP')
    log_y ${\log y}$ (long_name='log output')  log_c ${\log c}$ (long_name='log consumption')
    log_k ${\log k}$ (long_name='log capital')  log_l ${\log l}$ (long_name='log labor');
varexo eps_z ${\varepsilon_z}$ (long_name='TFP shock');
parameters betta ${\beta}$ (long_name='discount factor')  alppha ${\alpha}$ (long_name='capital share')
    delta ${\delta}$ (long_name='depreciation rate')  rhoz ${\rho_z}$ (long_name='TFP persistence')
    psi ${\psi}$ (long_name='labor disutility weight')  sigma ${\sigma}$ (long_name='inverse IES');

alppha = 0.33;  betta = 0.99;  delta = 0.025;  rhoz = 0.95;  sigma = 1;

model;
[name='Euler equation']
c^(-sigma) = betta*c(+1)^(-sigma)*(alppha*exp(z(+1))*k^(alppha-1)*l(+1)^(1-alppha) + 1 - delta);
[name='labor FOC']
psi*c^sigma/(1-l) = (1-alppha)*exp(z)*k(-1)^alppha*l^(-alppha);
[name='law of motion of capital']
k = invest + (1-delta)*k(-1);
[name='resource constraint']
y = c + invest;
[name='production function']
y = exp(z)*k(-1)^alppha*l^(1-alppha);
[name='TFP process']
z = rhoz*z(-1) + eps_z;
[name='log output'] log_y = log(y);
[name='log consumption'] log_c = log(c);
[name='log capital'] log_k = log(k);
[name='log labor'] log_l = log(l);
end;

steady_state_model;
    z = 0;
    // Fix steady-state labor at l = 1/3 and solve for psi (a calibration target solved in the steady state)
    l = 1/3;
    kl = ((1/betta - 1 + delta)/alppha)^(1/(alppha-1));  // capital-labor ratio (temporary variable)
    k = kl*l;
    invest = delta*k;
    y = kl^alppha*l;
    c = y - invest;
    psi = (1-alppha)*kl^alppha*(1-l)/c^sigma;
    log_y = log(y); log_c = log(c); log_k = log(k); log_l = log(l);
end;

resid;
steady;
check;

shocks;
    var eps_z; stderr 0.007;
end;

stoch_simul(order=1, irf=40, hp_filter=1600) log_y log_c log_k log_l z;
```

## Nonlinear by default; report log deviations (R8)

By default, write the original nonlinear equations (SKILL.md "Writing rules" R8) and let
`stoch_simul` do the perturbation. To show IRFs and moments as **log deviations**, choose one of
these:

- Preferred: add helper variables such as `log_y = log(y);` and list the `log_*` variables after
  `stoch_simul` (the template already does this).
- `stoch_simul(loglinear)`: takes the log of **all** variables (all steady-state values must be
  strictly positive).
- Declare `var(log) y;`: Dynare creates the auxiliary variable `LOG_y` (`LOG_y = log(y)`) and
  replaces `y` with `exp(LOG_y)` in the model.

Write an AR process in logs as `log(A) = rho_a*log(A(-1)) + eps_a;`.

Write a linearized model only when an R8 exception applies (the user asks for it, or the source
gives only the linearized system). Declare it with `model(linear);`.

## Nonstationary models (trends)

```dynare
trend_var(growth_factor=gA) A;          // multiplicative trend variable (declare it before any var that uses it)
var(deflator=A) y invest;               // y and invest follow trend A; Dynare stationarizes the model
// for an additive trend, use log_trend_var(log_growth_factor=…) and var(log_deflator=…)
var(log) c;                             // creates LOG_c = log(c); the model uses exp(LOG_c) for c
```

- It is usually simpler to write the model directly in detrended (stationary) form.
- Worked example: DSGE_mod `Aguiar_Gopinath_2007` (trend growth; recovers the nonstationary
  variables from the detrended variables), local copy in
  `references/examples-code/Aguiar_Gopinath_2007/`.

## Other cases

- **Deterministic and stochastic shocks together** (for example, an announced future change):
  declare the deterministic exogenous variable with `varexo_det`, give its path in the `shocks`
  block with `periods` and `values`, then write `stoch_simul(irf=0); forecast;`. `varexo_det` is
  refused in a perfect foresight file (Dygnosis E026).
- **`histval` as the start of a simulation**: with `periods>0`, a `histval` block sets the initial
  state of the simulation. It does not change the IRFs.
- **Third order with asymmetric innovations**: the worked examples are DSGE_mod `Andreasen_2012` and
  `Born_Pfeifer_2014`. They are in the public repository
  [DSGE_mod](https://github.com/JohannesPfeifer/DSGE_mod), not in the local
  `references/examples-code/`.

## Course examples (Pfeifer Dynare Course, local, preferred reference)

The files are in `references/examples-code/Dynare_Course/Chapter_04_stoch_simul/`, and the
introduction file is `Chapter_01_Dynare/NK_linear.mod`. To find more, search
`references/catalog-code.csv` for `stoch_simul`, `hp_filter` or
`conditional_variance_decomposition`.

| File | What it teaches |
| ---- | --------------- |
| `Chapter_01_Dynare/NK_linear.mod` | **Minimal `.mod` file**: three-equation linear New Keynesian model, `model(linear)`, `stoch_simul(order=1,irf,tex,irf_plot_threshold=0)`, `long_name` and TeX names, `write_latex_steady_state_model`. The first file to read for a beginner. |
| `Chapter_04_stoch_simul/RBC_IRF.mod` | Nonlinear RBC (TFP shock only): `stoch_simul(order=1,irf=40,hp_filter=1600,TeX)`. It declares variables with `var(log)`, so Dynare's `LOG_*` auxiliary variables report the IRFs as log (percent) deviations. The AR(1) shock is fitted on detrended data. |
| `Chapter_04_stoch_simul/RBC_baseline.mod` | RBC with TFP and government spending shocks: `conditional_variance_decomposition=[4]`, `hp_filter=1600`, variance decomposition. |

`RBC_IRF.mod` uses the `var(log)` route of the previous section to get IRFs in percent deviations.
`RBC_baseline.mod` is the same RBC model that chapters 5 to 10 reuse (filtering, estimation,
identification, method of moments, forecasting). Read it once as the base model of the course.
