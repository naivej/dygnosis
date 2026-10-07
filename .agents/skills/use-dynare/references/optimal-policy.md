# Optimal policy (Ramsey, discretion, OSR)

Read this when the task involves Ramsey policy, commitment, discretion, welfare or optimal simple rules,
or uses `ramsey_model`, `discretionary_policy`, `osr`, `planner_objective` or
`evaluate_planner_objective`. It covers how to choose among the three experiments, the planner
objective, the equation count, and the OSR statements.

Three different experiments; choose by the question (manual, "Optimal policy"):

- **Optimal policy under commitment (Ramsey)**: the planner commits once to a state-contingent plan. Use
  `ramsey_model` followed by `stoch_simul` (or another computing command). `ramsey_policy` is the
  deprecated all-in-one form.
- **Optimal policy under discretion**: the planner re-optimizes every period and cannot commit. Use
  `discretionary_policy`.
- **Optimal simple rules (OSR)**: find the coefficients of a **given** simple rule (for example a Taylor
  rule) that optimize the planner's criterion. Use `osr` with `osr_params`. OSR also implies commitment.

For Ramsey and discretion, the model block holds the **private-sector equilibrium conditions** and a
separate `planner_objective` gives the planner's goal. Do **not** write the rule for the policy
instrument: Dynare derives the optimal policy. For OSR you write the rule with free coefficients.

## Planner objective

```dynare
planner_objective pi^2 + vartheta*x^2;        // one-period loss (minimized)
// or a welfare / utility expression (maximized), for example:
planner_objective log(c) - chi*n^(1+phi)/(1+phi);
```

- Give the **one-period** objective, not the discounted lifetime sum. The discount factor comes from the
  `planner_discount` option of `ramsey_model`, `discretionary_policy` or `osr`.
- The objective can contain only current endogenous variables, no exogenous variables. To use an
  exogenous variable, define a helper variable for it in the model block.
- With `ramsey_model` any nonlinear expression is allowed. With `discretionary_policy` the objective
  must be quadratic.
- `planner_objective` and the optimal-policy commands go together (Dygnosis E100). With Ramsey, only one
  `planner_objective` is allowed (E104).

## Equation count (R4)

With `ramsey_model`, `ramsey_policy` or `discretionary_policy`, the model block has **one equation
fewer than endogenous variables per policy instrument**. Dynare adds the planner's first-order
conditions and one Lagrange multiplier per model equation, named `MULT_1`, `MULT_2`, … in the order of
the equations in the model block (manual, "Auxiliary variables"). The missing equation is the instrument
rule that Dynare solves for. With `osr`, the rule is written, so the count is equal.

Check the written count with Dygnosis `dynare_equations` (`count_gap`). W013 expects a gap of -N when
`instruments=` lists N names, and still warns on a square file (see `references/dygnosis-workflow.md`).

## Optimal policy under commitment (Ramsey)

```dynare
planner_objective pi^2 + vartheta*x^2;
ramsey_model(planner_discount=betta, instruments=(r));
stoch_simul(order=1, irf=20) x pi r;
evaluate_planner_objective;     // conditional and unconditional welfare
```

- `ramsey_model` only builds the expanded model (private-sector conditions + planner first-order
  conditions). Follow it with a computing command: `steady`, `stoch_simul` at any order, `estimation`,
  or the perfect-foresight commands.
- **Do not** write a Taylor rule for the instrument (R4).
- `planner_discount` sets the planner's discount factor (default `1.0`); usually the household's
  `betta`. `planner_discount_latex_name` sets its TeX name.
- `instruments=(…)` declares the instruments **for the steady-state computation under optimal policy**.
  It requires a `steady_state_model` block or a `<model>_steadystate.m` file that computes the steady
  state **conditional on the instrument value**. Set the instrument's initial value in `initval`. Put
  `steady` and `resid` after `ramsey_model` and the `initval` block. Example of a conditional
  steady-state file: `<dynare-root>/examples/optimal_policy/nk_ramsey_steady_file.mod` with
  `nk_ramsey_steady_file_steadystate.m` (it reads the instrument names from `options_.instruments`).
- `ramsey_policy(...)` is deprecated and equivalent to `ramsey_model; stoch_simul;
  evaluate_planner_objective;` (Dygnosis W150). Use the split form.
- `ramsey_constraints; r > 0; end;` adds bounds on variables (variable, `>` or `<`, constant). It
  requires `ramsey_model` or `ramsey_policy` (E203).
- Lagrange multipliers can be listed after `stoch_simul` (case-sensitive, e.g. `MULT_1`) to get their
  IRFs.
- `[static]` / `[dynamic]` equation tags are refused with Ramsey or discretion (E209).
- Manual warning (timeless perspective): a forward-looking helper variable (the manual says "auxiliary
  variable") changes the planner's first-period problem, because the planner takes period-0 values of
  predetermined variables as given. Write such definitions as model-local variables:
  `1/C=beta*1/C(+1)*(R_cap); #R_cap=R(+1)+(1-delta);`, not as a separate equation
  `R_cap=R(+1)+(1-delta);`.

## Optimal policy under discretion

```dynare
planner_objective pi^2 + vartheta*x^2;
discretionary_policy(planner_discount=betta, instruments=(r), order=1) x pi r;
```

The setup is the same as Ramsey (objective + private-sector conditions, no instrument rule), but the
solution is time-consistent (no commitment). The algorithm is an LQ solver (Dennis 2007). Requirements
(manual, "Optimal policy under discretion"):

- The objective must be quadratic.
- The model must **either** be linear (then set the `linear` option of `model` or `model_options`)
  **or** be solved at first order with an analytical steady state provided (`steady_state_model` or a
  steady-state file). `discretionary_policy` does not by itself require `model(linear)` (R8): the course
  file `Ramsey_Example_discretionary.mod` is a nonlinear `model;` with `steady_state_model`; the
  `Gali_2015_chapter_5_discretion` files use `model(linear)`.
- `instruments=(…)` is required (Dygnosis E215). An order greater than 1 is refused (E235).
  `discretionary_policy` cannot be combined with `ramsey_model` or `ramsey_policy` (E202).
- It accepts the options of `ramsey_policy` plus `discretionary_tol` (default `1e-7`) and `maxit`
  (default `3000`).
- `estimation` can follow `discretionary_policy` (estimation under discretion), and
  `evaluate_planner_objective` computes welfare.

## Optimal simple rules (OSR)

Here you **do** write the rule with free coefficients and let Dynare optimize them:

```dynare
parameters ... gpi gy grho;      // rule coefficients to optimize

model;
   ...
[name='simple interest-rate rule (coeffs optimized)']
   r = grho*r(-1) + (1-grho)*(gpi*pi + gy*x);
end;

// parameters Dynare will optimize:
osr_params gpi gy grho;

// loss weights to minimize:
optim_weights;
   pi 1;
   x  0.5;
   r  0.1;        // weights on covariances are also allowed: pi, x  W;
end;

// optional: bounds for the search:
osr_params_bounds;
   gpi, 1, 5;
   gy,  0, 3;
   grho, 0, 0.95;
end;

osr(opt_algo=9) x pi r;          // minimizes the weighted variance loss
```

`osr` supports two objectives (manual, "Optimal Simple Rules (OSR)"):

- **With `planner_objective`**: maximizes the expected discounted sum of the one-period planner
  objective (discount from `planner_discount`). Works at higher order; the course example uses
  `osr(opt_algo=9,order=2,planner_discount=beta)`.
- **Legacy, with `optim_weights`**: minimizes a weighted sum of unconditional (co)variances of
  demeaned endogenous variables, as in the code above. Only `order=1`, and the weighted variables must
  not have unit roots.
- Use one or the other: both together are refused (E204). `osr` requires `osr_params` and one of the two
  (E103).

Statements:

- `osr_params` lists the parameters to optimize. The search starts from their calibrated values (the
  course file sets `alpha = 1.5;` as the starting value).
- `optim_weights` gives the weight matrix: a diagonal element is `VARIABLE_NAME EXPRESSION;`, an
  off-diagonal element is `VARIABLE_NAME, VARIABLE_NAME EXPRESSION;`.
- `osr_params_bounds` (optional) gives `PARAMETER_NAME, LOWER_BOUND, UPPER_BOUND;` per line. It sets
  bounds only, not starting values. It needs a constrained optimizer: the manual lists `opt_algo` 1, 2,
  5 or 9, and `osr` does not accept 5, so use 1, 2 or 9.
- `osr` then runs `stoch_simul` and accepts its options. After `osr`, the rule parameters keep their
  optimal values, so a later `stoch_simul` uses them. Results: `oo_.osr.objective_function`,
  `oo_.osr.optim_params.<PARAMETER_NAME>`, `M_.osr.param_names`, `M_.osr.param_indices`,
  `M_.osr.param_bounds`.

## Welfare: `evaluate_planner_objective`

- Computes, displays and stores the planner objective under Ramsey or discretion in
  `oo_.planner_objective_value.unconditional` and `oo_.planner_objective_value.conditional`. In a
  stochastic Ramsey context, `conditional` has two subfields: `steady_initial_multiplier` (initial
  Lagrange multipliers at their steady state) and `zero_initial_multiplier` (planner implements the
  policy for the first time). You cannot set the initial multiplier values yourself.
- Conditional welfare is conditional on the period-1 information set:
  - Predetermined states inherited from period 0, endogenous and lagged exogenous, come from `histval`;
    without `histval` they are at the steady state.
  - Period-1 values of the exogenous shocks use the perfect-foresight syntax of `shocks`:
    `var u; periods 1; values 1;`. A lagged exogenous state (`u(-1)` in an equation) takes its period-0
    value from `histval`: `u(0)=1;`.
- Stochastic context: `pruning` is not supported. At `order=2` the result uses the second-order variance
  in `oo_.var`. Options `periods` (default 10000) and `drop` (default 1000) control the simulation for
  unconditional welfare at higher order.

## Notes

- The optimal-policy steady state is often non-trivial because of the multipliers. The manual says a
  numerical `initval` guess alone is in general very difficult; prefer a closed-form steady state
  conditional on the instruments (`steady_state_model` or a steady-state file). If that is not possible,
  give good numerical `initval` values and use `steady`.
- In stochastic experiments, keep R6: no `max`, `min`, `abs` or comparison operators near a kink.

Local examples (`references/examples-code/`): `Gali_2015/Gali_2015_chapter_5_commitment.mod`
(`model(linear)` with the deprecated `ramsey_policy`), `Gali_2015/Gali_2015_chapter_5_discretion.mod`
(`model(linear)` with `discretionary_policy`), and their `_ZLB` variants. Official 7.2 examples:
`<dynare-root>/examples/optimal_policy/nk_ramsey_osr.mod` (fixed Taylor rule, OSR or Ramsey,
selected with macro switches) and `nk_ramsey_steady_file.mod`.

## Course examples (Pfeifer Dynare course, run locally, first reference)

Path: `references/examples-code/Dynare_Course/Chapter_13_optimal_policy/`. One NK model with Rotemberg
pricing (Christiano, Motto and Rostagno 2007, "Notes on Ramsey-Optimal Monetary Policy", Section 2),
written once per policy regime so the optimal-policy commands can be compared side by side. Search the
code catalog with `rg -i "ramsey|discretionary_policy|osr|planner_objective" references/catalog-code.csv`.

| File | Policy regime | Key commands |
| ---- | ------------- | ------------ |
| `Ramsey_Example_commitment.mod` | Ramsey (commitment) | `ramsey_model(instruments=(r),planner_discount=beta,planner_discount_latex_name=$\beta$)` + `planner_objective log(C)-chi/2*h^2` + `stoch_simul(order=2,irf=20,periods=0,irf_plot_threshold=0)` + `evaluate_planner_objective`; instrument start value in `initval` |
| `Ramsey_Example_discretionary.mod` | discretion | `discretionary_policy(instruments=(r),planner_discount=beta,planner_discount_latex_name=$\beta$)`; nonlinear `model;` (not `model(linear)`) with `steady_state_model`; quadratic `planner_objective pi_hat^2*theta/((theta-1)/phi) + y_gap^2` |
| `Ramsey_Example_OSR.mod` | optimal simple rule | `osr(opt_algo=9,order=2,planner_discount=beta,irf_plot_threshold=0)` + `osr_params alpha` + `osr_params_bounds` + `planner_objective log(C)-chi/2*h^2` + `evaluate_planner_objective` |
| `Ramsey_Example_macroexp.mod` | macro-expanded copy | output of `savemacro` (`<file>_macroexp.mod`); contains only the OSR branch, with no `@#` directives left. Its header comments name the switches `Optimal_policy`, `Ramsey`, `Efficent_steady_state` and `Estimation_under_Ramsey`; the unexpanded original with those switches is `<dynare-root>/examples/optimal_policy/nk_ramsey_osr.mod` |

The main value of this set is the **comparison**: same model and calibration, only the optimal-policy
command changes. `ramsey_model` uses commitment, `MULT_` multipliers and one equation fewer than
endogenous variables; `discretionary_policy` needs a quadratic objective and a linear model or a
first-order solution with an analytical steady state; `osr` only optimizes the rule coefficients
(here against the planner objective; with `optim_weights`, against weighted unconditional variances).
`nk_ramsey_osr.mod` shows how macro switches keep several experiments in one file (macro-processor.md).
