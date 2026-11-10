# Occasionally binding constraints (OccBin)

Read this when a constraint binds only some of the time: a zero lower bound (ZLB), a collateral or
borrowing constraint, or irreversible investment, and the task wants OccBin's piecewise-linear
solution or estimation under that constraint. For one deterministic constraint, perfect foresight
with `lmmcp` and `⟂` is the lighter route ([perfect-foresight.md](perfect-foresight.md)).

"Zero lower bound (ZLB)" is the manual's term (`lmmcp` option).

Dygnosis checks the written OccBin form (E170–E185). It does not run `occbin_solver` or decide
whether the constraint binds. E178 is `shocks(surprise)` without an `occbin_constraints` block.
E172 is a `bind` or `relax` regime that is not defined. E175 is a constraint with no equation.

## File order

Order is part of the file. A ZLB example:

```dynare
var y c pi R_nom r_unc ...;
varexo eps_g eps_xi;
parameters ...;

model;
  // equations with no regime tag
  ...

  // shadow rate: no regime tag; it holds in both regimes
  [name='shadow Taylor rate']
  r_unc = (1/betta) * pi^phi_pi * y^phi_y;

  // policy: two equations, one name, one relax and one bind
  [name='monetary policy', relax='ELB']
  R_nom = r_unc;

  [name='monetary policy', bind='ELB']
  R_nom = R_lb;
end;

steady_state_model; ... end;
steady; check;

occbin_constraints;
    name 'ELB'; bind r_unc <= R_lb; relax r_unc > R_lb;
end;

shocks;
    var eps_g;  stderr 0.01;
    var eps_xi; stderr 0.01;
end;

shocks(surprise);
    var eps_xi;
    periods 1:8;
    values -0.25;
end;

occbin_setup;
occbin_solver(simul_periods=60, simul_check_ahead_periods=200, simul_maxit=50);
```

Place `occbin_constraints` and `shocks(surprise)` before `occbin_setup`, then call `occbin_solver`.
`occbin_setup` is a Dynare command. If the file also uses `stoch_simul`, put `occbin_setup` first.
Dygnosis reports the incompatible-command cases it covers (E179).

## `occbin_constraints`

```dynare
occbin_constraints;
   name 'ELB'; bind r_unc <= R_lb; relax r_unc > R_lb;
end;
```

- `name 'STRING'` is the name the equation tags use.
- `bind` is required. Dynare evaluates it in the baseline regime to decide whether the constraint
  starts to bind.
- The `relax` expression is optional. Dynare evaluates it in the constrained regime to decide when
  to return. If omitted, Dynare tests whether `bind` is false in that regime. An equation tagged
  `relax='ELB'` is a separate regime equation and is required for the example above (E172).
- The expressions use current endogenous variables, in levels, not deviations from the steady state.
  A lead, a lag or an exogenous variable has to be written as a helper variable first. A steady-state
  value uses `STEADY_STATE()`. Write expressions consistent with the piecewise-linear solution because
  Dynare evaluates them as written.
- Dynare allows at most two constraints (E171).

## Regime tags

Give all regime versions of an equation the same `name`. With one constraint, write a `relax`
version and a `bind` version.

```dynare
[name='monetary policy', relax='ELB']
R_nom = r_unc;

[name='monetary policy', bind='ELB']
R_nom = R_lb;
```

The shadow-rate equation has no regime tag. It holds in both regimes.

Two constraints:

```dynare
[name='foo', bind='IRR,INEG']
[name='foo', relax='IRR']
[name='foo', bind='IRR', relax='INEG']
```

## Run the simulation

After `occbin_solver` completes, check the simulation status and inspect the shocks and paths.

```matlab
sim = oo_.occbin.simul;
assert(sim.error_flag == 0, 'OccBin simulation failed');
shock_names = M_.exo_names(sim.exo_pos);
shock_path = sim.shocks_sequence;
rate = strcmp(M_.endo_names, 'R_nom');
rate_floor = M_.params(strcmp(M_.param_names, 'R_lb'));
floor_periods = find(abs(sim.piecewise(:, rate) - rate_floor) < 1e-5);
```

`sim.piecewise` uses the constrained solution. `sim.linear` ignores the constraint. Their rows
are periods and their columns follow `M_.endo_names`. Use `sim.regime_history` to confirm when
the constraint binds.

## Fiscal multiplier at the ZLB: two simulations

Run two `.mod` scenarios with the same model, calibration, initial state, demand shocks and solver
settings. Add the fiscal shock only in scenario A. Save `oo_.occbin.simul` and `M_.endo_names` after
each successful run. Use this surprise block in A and omit the `eps_g` entry in B.

```dynare
shocks(surprise);
    var eps_xi;
    periods 1:8;
    values -0.25;
    var eps_g;
    periods 1;
    values 0.10;
end;
occbin_setup;
occbin_solver(simul_periods=60, simul_check_ahead_periods=200);
```

With matching variable order, form the response from A minus B. The impact multiplier divides the
period-one output difference by the period-one spending difference, provided that denominator is
nonzero and both variables are in comparable level units. A path divided by that impact spending
difference is a response normalized by impact spending. A cumulative multiplier instead divides
the sum of output differences by the sum of spending differences over a stated horizon, provided
that spending sum is nonzero. For a linear comparison use each run's `linear` field and its own
spending difference. Convert log or percentage deviations to the intended units before computing a
fiscal multiplier. Check each run's regime history because the fiscal shock can change when the
bound binds.

## Pitfalls

| Pitfall | What happens | What to write |
|---|---|---|
| Unexpected paths or no binding | The applied shocks or constraint conditions may differ from the intended experiment | Inspect the shock path and regime history as shown above |
| No explicit `relax='ELB'` equation | Preprocessor: regime not defined (E172) | One `bind` equation and one `relax` equation, same `name` |
| `occbin_setup` after `stoch_simul` | Dynare refuses the command order | Put `occbin_setup` first |
| ZLB shade drawn before the series | Axis stretches; the lines overlap | `plot` first, read `ylim`, then `patch`, then `uistack` |

## Estimation under the constraint

```dynare
occbin_setup(likelihood_inversion_filter, smoother_inversion_filter);
estimation(smoother, heteroskedastic_filter, ...);
```

- `likelihood_inversion_filter` assumes the system starts at the steady state. The order of `varexo`
  must match the order of `varobs`.
- The piecewise Kalman filter (the default) is incompatible with the univariate Kalman filter
  (`kalman_algo=2` or `4`).
- If a shock drops out while the ZLB binds, set that observation to NaN. The piecewise Kalman filter
  also needs `heteroskedastic_shocks` to set that shock's standard deviation to 0.
- A unit root needs `diffuse_filter`.

## OccBin or `lmmcp`

| | OccBin | Perfect foresight with `lmmcp` |
|---|---|---|
| Use | Stochastic simulation, expectations, estimation | A deterministic path |
| How many constraints | At most two | Not limited to two |
| What you write | A regime equation for each case | A complementarity condition with `⟂` |

`Guerrieri_Iacoviello_2015` is the OccBin reference in Pfeifer's DSGE_mod on GitHub. It is not in
the local programming library. The local course files below are the copies to read.

## Course examples

Folder: `references/examples-code/Dynare_Course/Chapter_12_OccBin/`.
Search: `grep -iE "occbin|lmmcp|ELB" references/catalog-code.csv`.

| File | How it treats the bound | When to copy it |
|---|---|---|
| `NK_det.mod` | A kinked Taylor rule under perfect foresight | One deterministic path |
| `NK_det_mcp.mod` | An `[mcp]` tag and `perfect_foresight_solver(lmmcp)` | The idea of a deterministic bound. The `[mcp]` tag is obsolete (Dygnosis W170). Write `⟂` instead. |
| `NK_occbin.mod` | `occbin_constraints`, `bind`/`relax` tags | The tag structure and standard `shocks(surprise)`, `occbin_setup`, `occbin_solver` workflow |

Read `NK_det.mod`, then `NK_det_mcp.mod`, then `NK_occbin.mod`.

Official example: `<dynare-root>/examples/occbin/rbc_occbin.mod`. Manual: "Occasionally binding
constraints (OccBin)". Run Dynare from `SKILL.md` "Numerical runs".
