# Steady state

Read this for almost every task: the linearization point and the initial and terminal conditions depend
on the steady state. It covers the choice between a closed-form `steady_state_model` (preferred) and a
numerical `initval` guess, calibration targets solved inside the steady state, homotopy, `[static]` /
`[dynamic]` tags, and the commands that verify the result.

Almost every `.mod` file needs a steady state: `stoch_simul` and `estimation` approximate the model
around it, and perfect foresight uses it for initial and terminal conditions. A correct steady state
separates a correct result from a silent error. **Default: if you can solve the steady state by hand,
write the closed-form (analytical) solution in a `steady_state_model` block.** Use a numerical `initval`
guess only when there is no closed form (SKILL.md "New model", "Stage 4 (steady state)" in
workflow-detail.md).

Solving the steady state is numerical work that only Dynare does (`steady`, `resid`, run under MATLAB).
Dygnosis checks the written blocks statically; it does not compute residuals or prove that a
steady state exists. Static checks that apply:

- W042: an endogenous variable has no assignment in `steady_state_model` (Dynare warns and falls back to
  the `initval` value or zero).
- E130: a variable is used before it is assigned in `steady_state_model` (Dynare refuses). Dygnosis does
  not report E130 when `ramsey_model` or `ramsey_policy` is present, because a conditional steady state
  can read the instrument value.
- W131: a name is assigned twice in `steady_state_model` (Dynare accepts and warns).
- I050: neither `initval` nor `steady_state_model` nor a sibling `<model>_steadystate.m` exists.
- W052: an endogenous variable is missing from `initval` (Dynare assumes zero).
- E065: an exogenous variable inside the `steady_state()` operator (Dynare refuses).
- E208 / E209: `[static]` and `[dynamic]` counts differ, or the tags are used with Ramsey or discretion.
- `dynare_related_files` shows whether a `<model>_steadystate.m` companion file exists and resolves.

See `references/dygnosis-workflow.md` for the tools.

## Option A (preferred): closed-form `steady_state_model`

When you can solve the steady state on paper, give the formulas to Dynare. It can then recompute the
steady state cheaply and reliably at every parameter point (essential for estimation) instead of running
a Newton-type solver every time.

```dynare
steady_state_model;
    z = 0;                                  // the shock is zero in the steady state
    r = 1/betta - 1 + delta;                // build intermediate values step by step...
    kl = (alppha/r)^(1/(1-alppha));         // temporary variable (capital-labor ratio)
    w  = (1-alppha)*kl^alppha;
    // ...derive each endogenous variable row by row from parameters, steady-state exogenous values and variables assigned above
    l  = ...;
    k  = kl*l;
    invest = delta*k;
    y  = kl^alppha*l;
    c  = y - invest;
end;
steady;        // runs the block and checks that it solves the static model
```

Rules:

- Each row assigns **one** expression to one variable (endogenous, temporary or parameter). The
  expression may use parameters, steady-state values of exogenous variables, and variables **assigned in
  earlier rows**. **Order matters**: the block is computed row by row, not solved as a system
  (E130 when a row reads a later assignment).
- Temporary variables need no declaration.
- When the right-hand side is a MATLAB/Octave function with several outputs, assign them together:
  `[W, e] = my_function(l, n);`.
- Dynare generates `+FILENAME/steadystate.m` from the block.
- The block also works for deterministic models: put `steady` after each `initval` and `endval` block
  that sets exogenous levels, to run it.
- If the values deliberately do not solve the static model exactly (for example a unit-root model, where
  the steady state is not unique or does not exist), use `steady(nocheck)`.

## Calibration targets solved in the steady state (Pfeifer's key technique)

A `steady_state_model` block can **update parameters**: solve a calibration target for the parameter
value. Example: fix steady-state labor `l=0.33` and solve for the labor disutility `psi`; solve for
`delta` and `betta` from the great ratios:

```dynare
steady_state_model;
    // calibration first: solve for parameters from targets such as investment/output i_y and capital/output k_y
    gammax = (1+n)*(1+x);
    delta  = i_y/k_y - x - n - n*x;
    betta  = (1+x)*(1+n)/(alppha/k_y + (1-delta));
    l = 0.33;                               // target: steady-state labor = 1/3
    k = ((1/betta*(1+n)*(1+x) - (1-delta))/alppha)^(1/(alppha-1))*l;
    invest = (x+n+delta+n*x)*k;
    y = k^alppha*l^(1-alppha);
    c = (1-gshare)*y - invest;
    psi = (1-alppha)*(k/l)^alppha*(1-l)/c^sigma;   // psi solved so that l = 0.33
    w = (1-alppha)*y/l;
    r = 4*alppha*y/k;
    z = 0; ghat = 0;
end;
```

Do **not** also assign these parameters (`psi`, `delta`, `betta`) in the calibration section before the
model; this block would overwrite the values anyway. Dygnosis W010 (parameter not assigned) is expected
for them until the `steady_state_model` block assigns them.

**A parameter solved this way can also appear in a dynamic equation of the model block.** Typical: solve
the steady-state spending parameter `gss` from the target `G/Y = gy_share` and use it as the mean of the
exogenous process:

```dynare
parameters gss;                              // not assigned in the calibration section; solved below
model;
   [name='gov spending process']
   log(g) = (1-rho_g)*log(gss) + rho_g*log(g(-1)) + eps_g;
end;
steady_state_model;
   // ...solve for y first...
   gss = gy_share*y;                         // solved to hit G/Y = gy_share
   g   = gss;
end;
```

Why this is safe: in estimation and simulation, `steady_state_model` runs at each parameter point and
updates `gss` before Dynare approximates the model, so the dynamic equation sees the updated value. Use
this pattern for every quantity that is both a calibration target and part of an equation: trend growth,
steady-state inflation, government spending or debt ratios.

## Option B: numerical `initval` guess

Without a closed form, give the Newton-type solver a good starting point:

```dynare
initval;
   c = 1;  k = 10;  l = 0.33;  y = 1;  invest = 0.25;  z = 0;
end;
steady;        // Dynare solves the static model from this guess
```

- Give a value to **every** endogenous variable. An endogenous or exogenous variable left out of
  `initval` is set to **0**, which often makes the solver fail (for example a TFP level of 0). W052 lists
  the omitted endogenous variables.
- A good guess is the hard part. Build complex models step by step and use economically sensible values
  (great ratios, labor about 1/3).
- Check with `resid;` before solving (near 0 once solved). If `steady` fails, adjust
  `steady(solve_algo=..., maxit=..., tolf=...)`; the algorithms are listed below.

## Option C: hand-written steady-state file

For maximum flexibility (loops, conditions), write `FILENAME_steadystate.m`, where `FILENAME.mod` is the
model file. It is more powerful but easier to get wrong; `steady_state_model` is usually enough.

- Signature, as in the official examples:
  `function [ys,params,check] = FILENAME_steadystate(ys,exo,M_,options_)`.
- Examples: `<dynare-root>/examples/stochastic_simulations/nk_baseline_steadystate.m` (the manual
  writes `NK_baseline_steadystate.m`; it calibrates labor disutility inside the file) and
  `<dynare-root>/examples/optimal_policy/nk_ramsey_steady_file_steadystate.m` (a steady state
  conditional on the Ramsey instrument; see optimal-policy.md).
- Names (R5): the file runs as MATLAB code, so do not name parameters or variables after MATLAB functions
  such as `alpha`, `beta`, `gamma`; write `alppha`, `betta`, `gam`. The preprocessor accepts these names
  and Dygnosis does not report them, so a clean check does not clear them.

## Homotopy: when a good guess still does not converge

Solve an easy parameterization first, then move step by step to the hard one:

```dynare
homotopy_setup;
   gam, 0.5, 2;     // gam moves from 0.5 to 2
   x,   2;          // x moves from its initval value to 2
end;
steady(homotopy_mode=1, homotopy_steps=50);
```

- Each line is `NAME, START, END;` or `NAME, END;` for a parameter or exogenous variable; with one value,
  the start comes from the preceding `initval` block (or `endval`, if one comes before
  `homotopy_setup`).
- Dynare must solve the starting point without help (from the `initval` or `endval` guesses).
- `homotopy_mode`: `0` no homotopy (default); `1` all parameters move together, the distance divided
  into `homotopy_steps` intervals; `2` one parameter at a time; `3` try the end values first and halve
  the interval after each failure (double it after each success), with `homotopy_steps` as the maximum
  number of attempts. `homotopy_steps` defaults to 10; if the homotopy fails, increase it.
- `homotopy_force_continue=1`: when the homotopy fails, `steady` keeps the last successful step and
  continues. **Dangerous**: parameters or exogenous variables are then not at the values you asked for.
- With a permanent shock and an already solved initial steady state, put
  `homotopy_setup(from_initval_to_endval);` after the `endval` block (usually with an empty body): every
  exogenous variable moves from its `initval` value to its `endval` value in the next `steady`.

## `[static]` / `[dynamic]` equation tags

Sometimes the static (steady-state) version of an equation should differ from the dynamic one; the
typical case is a unit-root model with a continuum of steady states, or a partial closed form. An
equation tagged `[static]` is used only for the steady state; an equation tagged `[dynamic]` is used for
everything else:

```dynare
model;
   c + k - aa*x*k(-1)^alph - (1-delt)*k(-1);
   [dynamic] c^(-gam) - (1+bet)^(-1)*(aa*alph*x(+1)*k^(alph-1) + 1 - delt)*c(+1)^(-gam);
   [static]  k = ((delt+bet)/(x*aa*alph))^(1/(alph-1));
end;
```

Every `[static]` equation needs a `[dynamic]` partner (E208). The tags cannot be used with
`ramsey_model`, `ramsey_policy` or `discretionary_policy` (E209). For the equation count (R4) a pair
counts once.

## Verify the steady state (Dynare)

Run these in Dynare under MATLAB with the [MATLAB Agentic Toolkit](https://github.com/matlab/matlab-agentic-toolkit)
(SKILL.md "Numerical runs"); if MATLAB is unavailable, give the commands to the user
(debugging.md "Run-and-fix loop"). Do not infer a steady state or the Blanchard-Kahn conditions from
static checks.

- `steady;` prints the steady-state values; with a steady-state file it also checks that they solve the
  static model.
- `resid;` (or `resid(non_zero);`, which prints only nonzero residuals) prints the static residuals at the
  values of the last `initval` / `endval` block, or of the steady-state file. Use it before `steady` to
  find the wrong equation.
- `check;` reports the eigenvalues and whether the Blanchard-Kahn conditions hold
  (`references/debugging.md`).
- After the run, read `oo_.steady_state`: no NaN, economically plausible values.
- Heterogeneous-agent models are different: Dynare refuses `steady` and `check` there (E474); use the
  `heterogeneity_*` steady-state commands (heterogeneity.md).

## `steady` options

`solve_algo` (manual, "Steady state"):

| Value | Method | Note |
| ----- | ------ | ---- |
| 0 | `fsolve` | under MATLAB needs the Optimization Toolbox; always available under Octave |
| 1 | Newton-like algorithm with line search | |
| 2 | splits the model into recursive blocks, solves each with algorithm 1 | |
| 3 | Chris Sims' solver | |
| **4** | **splits the model into recursive blocks, solves each with a trust-region solver with autoscaling (default)** | first choice |
| 5 | Newton with sparse Gaussian elimination (SPE) | needs the `bytecode` option; tune with `markowitz` (default 0.5) |
| 6 | Newton with sparse LU | |
| 7 / 8 | Newton with GMRES / BiCGStab | |
| 9 | trust-region with autoscaling on the whole model (algorithm 4 without splitting) | |
| 10 | Levenberg-Marquardt mixed complementarity problem solver (LMMCP) | complementarity conditions written with `⟂` (see the `lmmcp` option, R6) |
| 11 | PATH mixed complementarity problem solver | download PATH yourself and put it on the MATLAB path |
| 12 | block decomposition by the preprocessor, Newton-type solver on each block | more efficient than 2; typical for purely backward, forward or static models and semi-structural models; do not combine with the `block` option of `model` |
| 14 | as 12, with a trust-region solver on the blocks | |

Other options: `maxit` (default 50), `tolf` (default `eps^(1/3)`), `tolx` (default `eps^(2/3)`),
`nocheck`, `noprint` (useful in loops), `non_zero`, `fsolve_options` (only with `solve_algo=0`),
`homotopy_mode`, `homotopy_steps`, `homotopy_force_continue`. Dygnosis `dynare_list_options` lists the
valid options.

## Two forms of a steady-state file

The manual gives two forms (manual, "Providing the steady state to Dynare"):

- **`steady_state_model` block**: Dynare generates **`+FILENAME/steadystate.m`** inside the package
  folder.
- **Written by the user**: must be named **`FILENAME_steadystate.m`**. More flexible (loops,
  conditions) at the cost of more programming and lower efficiency.

Older Dynare versions (4.x/5.x) named the generated file `<m>_steadystate2.m`; not verified against
the manual. With `steady_state_model`, current Dynare writes `+<m>/steadystate.m`, which cleanup removes
with the `+<m>/` folder. **Never delete** a user-written `<m>_steadystate.m` (no `2`). See "Cleanup" in
workflow-detail.md.

Both forms can **update parameters** at each call: for example set the labor disutility so that
steady-state labor is 0.2, or, in estimation, update a parameter that is a function of an estimated one
so that a ratio stays fixed. This is how calibration targets are solved in the steady state. Do not
overwrite parameters by accident.

## Related commands and outputs

- `resid(non_zero);` shows only nonzero residuals, which finds the wrong equation faster.
- `get_mean('c','k')` returns steady-state values from `oo_.steady_state`; if the steady state is not yet
  computed, it computes it first.
- The steady state is in `oo_.steady_state` (declaration order of `var`, as in `M_.endo_names`); the
  exogenous steady state is in `oo_.exo_steady_state` (declaration order of `varexo`). With a permanent
  shock, the initial steady state is also stored in `oo_.initial_steady_state` and
  `oo_.initial_exo_steady_state`.
