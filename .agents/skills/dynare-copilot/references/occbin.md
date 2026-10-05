# Occasionally binding constraints (OccBin)

Read this when a constraint binds only some of the time: a zero lower bound (ZLB), a collateral or
borrowing constraint, or irreversible investment, and the task wants OccBin's piecewise-linear
solution or estimation under that constraint. For one deterministic constraint, perfect foresight
with `lmmcp` and `⟂` is the lighter route ([perfect-foresight.md](perfect-foresight.md)).

"Zero lower bound (ZLB)" is the manual's term (`lmmcp` option).

Dygnosis checks the written OccBin form (E170–E185). It does not run `occbin.solver` or decide
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

occbin_setup(simul_periods=60, simul_check_ahead_periods=200, simul_maxit=50);
stoch_simul(order=1, irf=0, nograph, nocorr, nomoments);

% MATLAB after stoch_simul: set the shock path and call occbin.solver (below)
```

Order: `occbin_constraints`, `shocks`, `occbin_setup`, `stoch_simul`, then the MATLAB simulation.
`occbin_setup` is a Dynare command. Put it before `stoch_simul`, not inside a later MATLAB fragment.
`occbin_setup` after `stoch_simul` is a written clash Dynare refuses at transform; Dygnosis reports
the incompatible-command cases it covers (E179).

## `occbin_constraints`

```dynare
occbin_constraints;
   name 'ELB'; bind r_unc <= R_lb; relax r_unc > R_lb;
end;
```

- `name 'STRING'` is the name the equation tags use.
- `bind` is required. Dynare evaluates it in the baseline regime to decide whether the constraint
  starts to bind.
- `relax` is the return condition, evaluated in the constrained regime. Write it on its own equation.
  An omitted relax regime is refused (`the regime corresponding to relax='…' is not defined`; E172).
- The expressions use current endogenous variables, in levels, not deviations from the steady state.
  A lead, a lag or an exogenous variable has to be written as a helper variable first. A steady-state
  value uses `STEADY_STATE()`.
- Dynare allows at most two constraints (E171).

## Regime tags

Each constraint needs two equations that share one `name`:

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

Call `occbin.solver` from MATLAB after `stoch_simul`, and set the shock matrix yourself:

```matlab
T_sim = 60;
shk = zeros(T_sim, M_.exo_nbr);
shk(1, find(strcmp(M_.exo_names,'eps_g'))) = 0.10;

options_.occbin.simul.SHOCKS    = shk;
options_.occbin.simul.endo_init = zeros(M_.endo_nbr, 1);

[~, out, ~] = occbin.solver(M_, options_, oo_.dr, oo_.dr.ys, oo_.exo_steady_state, []);
```

`endo_init` has length `M_.endo_nbr` (every endogenous variable), not `M_.nspred`. The wrong length
fails at runtime with a size mismatch on `history(:,1) = init`.

```matlab
% out.piecewise (T x M_.endo_nbr): piecewise-linear path, in levels
% out.linear    (T x M_.endo_nbr): linear path, constraint ignored
idx_y    = find(strcmp(M_.endo_names, 'y'));
idx_rnom = find(strcmp(M_.endo_names, 'R_nom'));
zlb_periods = find(abs(out.piecewise(:, idx_rnom) - R_lb) < 1e-5);
```

**`shocks(surprise)` was reported to write a zero shock matrix under Dynare 7.1**, so the constraint
never bound and Dynare printed no error. `occbin_solver;` reads that block, so it failed the same
way. This was not rechecked under Dynare 7.2. Until it is, set `options_.occbin.simul.SHOCKS` and
call `occbin.solver` as above. Do not treat a 7.1 report as a 7.2 fact.

## Fiscal multiplier at the ZLB: two simulations

The multiplier is ΔY / ΔG. One simulation mixes the demand shock into Y. Run twice and subtract.

```matlab
T_sim = 60;
ix_xi = find(strcmp(M_.exo_names, 'eps_xi'));
ix_g  = find(strcmp(M_.exo_names, 'eps_g'));

shk_A = zeros(T_sim, M_.exo_nbr);
shk_A(1:8, ix_xi) = -0.25;
shk_A(1,   ix_g)  =  0.10;
options_.occbin.simul.SHOCKS    = shk_A;
options_.occbin.simul.endo_init = zeros(M_.endo_nbr, 1);
[~, out_A, ~] = occbin.solver(M_, options_, oo_.dr, oo_.dr.ys, oo_.exo_steady_state, []);

shk_B = zeros(T_sim, M_.exo_nbr);
shk_B(1:8, ix_xi) = -0.25;
options_.occbin.simul.SHOCKS = shk_B;
[~, out_B, ~] = occbin.solver(M_, options_, oo_.dr, oo_.dr.ys, oo_.exo_steady_state, []);

idx_y = find(strcmp(M_.endo_names, 'y'));
idx_g = find(strcmp(M_.endo_names, 'g'));
dG0   = out_A.piecewise(1, idx_g) - oo_.dr.ys(idx_g);
mult_zlb   = (out_A.piecewise(:,idx_y) - out_B.piecewise(:,idx_y)) / dG0;
mult_nozlb = (out_A.linear(:,idx_y)    - out_B.linear(:,idx_y))    / dG0;
```

A one-period demand shock dies out through the AR(1) and often does not keep the ZLB binding. A
rough size, from the decision rule:

```text
|eps_min| > (R_ss - R_lb) / |coefficient of eps_xi in r_unc|
```

Read that coefficient from `oo_.dr.ghu`. To keep the ZLB binding for N periods, apply the surprise
for N periods (Eggertsson–Woodford 2003). Example with β=0.99, φπ=1.5, φy=0.5, R_lb=1: R_ss − R_lb
= 0.0101, coefficient 0.0348, one-period size about −0.29; −0.25 for 8 periods left the ZLB binding
for 12 periods in that exercise. Recompute the coefficient for your model.

## Pitfalls

| Pitfall | What happens | What to write |
|---|---|---|
| `shocks(surprise)` (reported for 7.1, not rechecked in 7.2) | Shock matrix stays zero; the ZLB never binds; no error | Set `options_.occbin.simul.SHOCKS` |
| `endo_init` has length `M_.nspred` | Size mismatch at runtime | `zeros(M_.endo_nbr, 1)` |
| No explicit `relax='ELB'` equation | Preprocessor: regime not defined (E172) | One `bind` equation and one `relax` equation, same `name` |
| One period of the demand shock | ZLB binds for 0 periods | N surprise periods; size as above |
| `occbin_setup` after `stoch_simul` or inside MATLAB | Refused, or the command never runs | Dynare command before `stoch_simul` |
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
| `NK_occbin.mod` | `occbin_constraints`, `bind`/`relax` tags | The tag structure. Its run uses `shocks(surprise)` and `occbin_solver`, the path reported to fail under 7.1. Copy the tags; run with `options_.occbin.simul.SHOCKS` and `occbin.solver` until that report is rechecked. |

Read `NK_det.mod`, then `NK_det_mcp.mod`, then `NK_occbin.mod`.

Official example: `<dynare-root>/examples/occbin/rbc_occbin.mod`. Manual: "Occasionally binding
constraints (OccBin)". Run Dynare from `SKILL.md` "Numerical runs".
