# Higher-order perturbation (`order=2` or `order=3`)

Read this when the question is about risk itself: risk premia, asset pricing, precautionary saving,
uncertainty (volatility) shocks, the second-order term in welfare, Epstein–Zin preferences, the
stochastic steady state, or a generalized impulse response function (GIRF). The command is still
`stoch_simul` or `method_of_moments`, with `order=2` or `order=3`. This file says when a higher order
is required and how to use pruning, GIRFs and the stochastic steady state. Option details and the
`oo_.dr` tensors are in [stochastic-simulation.md](stochastic-simulation.md).

A first-order approximation has certainty equivalence: shock variances do not enter the decision rule,
IRFs are symmetric, risk premia are zero, and a volatility shock has no effect. A question about how
risk changes means and decisions needs at least second order.

## 1. What each order adds

| Need | Lowest order | Why |
|---|---|---|
| IRFs, moments, estimation | 1 | Certainty equivalence is enough, and it is the most stable |
| Risk premia, precautionary saving, the risk correction to the **mean** (`ghs2`), second-order welfare | 2 | A constant risk correction and quadratic terms; the mean leaves the deterministic steady state |
| A **time-varying** risk premium, stochastic volatility, third-order welfare | 3 | The risk correction depends on the state, so a volatility shock has a nontrivial path |

Stay at order 1 when it answers the question. Higher order is slower, can explode, and needs pruning.

## 2. `order` and `pruning`

```dynare
stoch_simul(order=2, pruning, irf=0) y c rp_ann;
```

- `order=2` or `order=3` is the order of the Taylor approximation.
- Turn **`pruning` on from order 2.** Higher-order simulation produces explosive paths from the higher
  powers (Kim et al. 2008; Andreasen et al. 2018). Pruning drops those terms order by order.
  Theoretical moments then use the pruned state space. Without pruning, the second moments are only
  the linear approximation. See [stochastic-simulation.md](stochastic-simulation.md).
- `conditional_variance_decomposition` is not available at the orders and pruning settings Dynare
  refuses. Check `dynare_list_options` for `stoch_simul` before you write the option.
- Skewed innovations at higher order: programming library `Andreasen_2012`, `Born_Pfeifer_2014`.

## 3. Objects that exist only at higher order

### Stochastic steady state and ergodic mean

These are not manual terms. Define them when you report them.

From order 2, the center of the model is not the deterministic steady state.

- **Stochastic steady state** (risky steady state): the point where the agent stays when the current
  shock is zero but future risk is taken into account.
- **Ergodic mean** (also called EMAS in this literature): the mean of a long simulation.

Both differ from the deterministic steady state by the risk correction (`oo_.dr.ghs2`). Say which
baseline an IRF or a mean uses. A GIRF is usually relative to the ergodic mean.

### Generalized impulse response function (GIRF)

Not a manual term. At higher order an IRF depends on the initial state and on the sign and size of the
shock. The path from `irf=` is one path relative to the deterministic steady state. A GIRF is the
average response to a shock starting from the ergodic mean: simulate shocked paths, average them, and
subtract the no-shock baseline. The programming-library file `Basu_Bundick_2017` is the worked pattern,
including uncertainty shocks (a volatility shock does nothing at order 1; use order 3 and a GIRF).

## 4. Asset pricing

A risk premium is an expected excess return. It comes from the covariance of the return with the
stochastic discount factor, which is a second moment. Certainty equivalence sets that covariance to
zero, so the premium is zero at order 1. Any risk-premium question needs at least `order=2`. A
time-varying premium needs `order=3`. The course file `Jermann1998.mod` runs the same asset-pricing
model at orders 1, 2 and 3. Habit formation and capital adjustment costs are often used to enlarge the
premium.

## 5. Epstein–Zin preferences

Epstein–Zin preferences separate the elasticity of intertemporal substitution from risk aversion.
Write the utility and the certainty-equivalent operator with **helper variables** (for example `U` and
`E_t[U(+1)^(1-gamma)]^(1/(1-gamma))`). Do not call these Dynare auxiliary variables: those are the
`AUX_*` names Dynare creates. Then use `order=2` or `order=3` with `pruning`. Programming library:
`Caldara_et_al_2012`.

## 6. Plot the decision rule

The point of a higher order is the curvature of the decision rule. The course file `plot_policy_fun.m`
rebuilds the rule from the `oo_.dr` tensors on a grid of the state and plots one control against one
state: order 1 is a straight line; orders 2 and 3 bend. `AES_example.m` compares the true solution of
a toy expectational equation with a simultaneous perturbation and a sequential perturbation, including
the Jensen term from `exp`.

## 7. Local course examples

Folder: `references/examples-code/Dynare_Course/Chapter_08_Higher_order/`.
Search: `grep -iE "order = 2|order=2|higher|risk premium|asset pricing" references/catalog-code.csv`.

| File | What it shows |
|---|---|
| `Jermann1998.mod` | The same model at orders 1, 2 and 3: risk premium, risk-free rate, stochastic discount factor, annualized return; habit and capital adjustment costs |
| `Jermann_1998.mod` | DSGE_mod copy: `stoch_simul(order=2) rp_ann` |
| `plot_policy_fun.m` | Decision-rule plot from `oo_.dr` |
| `AES_example.m` | True solution against two perturbation schemes on a toy expectational equation |

Read the order comparison in `Jermann1998.mod` first.

## 8. Programming library (`catalog-code.csv`)

- `SGU_2004`: second-order approximation, decision rule, pruning, first-order against second-order welfare.
- `Basu_Bundick_2017`: uncertainty shocks, GIRF at the ergodic mean, a steady-state file, order 3.
- `Caldara_et_al_2012`: Epstein–Zin preferences, stochastic volatility, `plot_policy_fun.m`.
- `Born_Pfeifer_2018_welfare`, `RBC_baseline_welfare`: second-order welfare and consumption equivalents.

## 9. Pitfalls

| Pitfall | Class | What to do |
|---|---|---|
| No `pruning` | Compute advice | Explosive paths or NaN at order 2 or 3. Turn `pruning` on. |
| `max`, `min`, `abs` or a comparison on an endogenous variable | Tool coverage | Derivatives at the kink are wrong (R6). Dygnosis W200. Use OccBin or perfect foresight. |
| Reading a higher-order IRF as a deviation from the deterministic steady state | Compute advice | Say whether the baseline is the ergodic mean or the stochastic steady state. |
| `order=2` in GMM or SMM by default | Compute advice | Slower, and not always better identified. Confirm that the question needs the risk term ([moments-method.md](moments-method.md)). |

Nonsmooth operators in `model(linear)` are E210 and E211. Dygnosis does not compute risk premia, GIRFs
or welfare.
