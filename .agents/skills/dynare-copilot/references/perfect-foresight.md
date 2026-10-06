# Perfect foresight (deterministic simulation)

Read this when the task is a transition path, a permanent or temporary shock, or a deterministic
simulation, with `perfect_foresight_setup` and `perfect_foresight_solver`. This file covers
`initval`, `endval` and `histval`, the deterministic `shocks` block, complementarity conditions, and
the singular stacked Jacobian.

Under perfect foresight, agents know the whole future path of the exogenous variables. Dynare solves
the nonlinear system over T periods. It does not take a Taylor approximation. Typical uses: return to
the same steady state after a temporary shock, or transition to a new steady state after a permanent
shock.

The two commands, in this order:

```dynare
perfect_foresight_setup(periods=200);
perfect_foresight_solver;
```

A solver command before its setup is E213 (expectation errors: E214). A file cannot also contain
`stoch_simul` or `estimation` (E205). `initval` after `endval` is E217.

After the run, read `oo_.endo_simul` and `oo_.exo_simul`. Dygnosis does not compute those paths.
Setting an exogenous variable in `initval` (the permanent-shock pattern below) is accepted by Dynare.
With a parsed `simul`, `perfect_foresight_setup`, `perfect_foresight_solver`, either
expectation-errors command, or `perfect_foresight_controlled_paths`, Dygnosis keeps W051 quiet. Keep
the assignment: without it the shock would start at zero.

## Initial and terminal conditions

A forward-looking model needs initial conditions (lags of predetermined variables) and a terminal
condition (leads of forward-looking variables).

### Temporary shock: back to the same steady state

```dynare
initval;
   c = 1.2;  k = 12;  x = 1;
end;
steady;                 // steady state at the exogenous value in initval

shocks;
   var x;
   periods 1;
   values 1.2;
end;

perfect_foresight_setup(periods=200);
perfect_foresight_solver;
```

`initval` followed by `steady` computes the steady state at those exogenous values and uses it as the
initial condition, the terminal condition and the solver's initial guess.

### Permanent shock: transition to a new steady state

```dynare
initval;
   x = 1;  k = 12;  c = 1.2;
end;
steady;

endval;
   x = 2;  k = 20;  c = 2;
end;
steady;

perfect_foresight_setup(periods=200);
perfect_foresight_solver;
```

- `initval` comes first. It supplies the history of the predetermined variables. `endval` supplies
  the terminal condition of the forward-looking variables and the solver's guess in each period.
- Without a following `steady`, `endval` is taken literally. It need not be a steady state.
- A variable omitted from `endval` keeps its value from the previous `initval` or `steady`. It is not
  set to zero. A variable omitted from `initval` defaults to 0.
- `perfect_foresight_setup(periods=200, endval_steady)` (or `steady` after `endval`) makes the solver
  compute the terminal steady state. Useful for a large permanent shock.

### `histval`: a chosen history

For lags longer than one, `histval` sets a different value in each pre-sample period. Period 1 is the
first simulation period. Periods 0, −1, … are the history. `initval` then supplies the terminal
condition and the solver's guess.

```dynare
model;
   x = 1.5*x(-1) - 0.6*x(-2) + epsilon;
   log(c) = 0.5*x + 0.5*log(c(+1));
end;
histval;
   x(0)  = -1;
   x(-1) =  0.2;
end;
initval;
   c = 1;  x = 1;
end;
```

`histval` is not followed by `steady`. It does not accept a variable that is not a state. A lag
greater than zero in `histval` is E242.

## Deterministic `shocks` block

A temporary path goes in `shocks`. A permanent change goes in `endval`.

```dynare
shocks;
   var e;
   periods 1;
   values 0.5;

   var v;
   periods 4:5  6  7:9;
   values  1    1.1  0.9;

   var w;
   periods 1 2;
   values (1+p) (exp(z));
end;
```

- A range such as `4:5` with one number repeats that number. A vector must have one entry per period.
- Period 1 is the first endogenous period. Period 0 can clash with `initval` or `endval`. After
  `perfect_foresight_setup`, read `oo_.exo_simul`.
- `mshocks` is the multiplicative form (`1.05` is 5 percent above the baseline). It is useful when
  the exogenous steady state is not zero. With no `endval`, the baseline is the `initval` steady
  state. With `endval`, the baseline is the terminal steady state, unless `relative_to_initval`.

## Solver options

- `stack_solve_algo=0` (default, sparse Newton on the stacked system), `1` (LBJ, less memory), or
  another documented block or iterative solver. Confirm the number with `dynare_list_options`.
- `maxit`, `tolf`, `tolx`.
- `no_homotopy` turns off the automatic homotopy (on failure, Dynare shrinks the shock and steps it
  back up).
- `print` or `noprint`.

Homotopy knobs: `homotopy_initial_step_size`, `homotopy_min_step_size`,
`homotopy_max_completion_share`, `homotopy_linearization_fallback` (linear extrapolation if the
homotopy does not reach 100 percent), `homotopy_marginal_linearization_fallback`,
`homotopy_exclude_varexo`.

`linear_approximation` solves the linearized system (needs a steady state; `stack_solve_algo` in
{0, 7}). `endogenous_terminal_period` drops the tail once it has converged (algorithm 0 only).

## Singular stacked Jacobian

If, after substitution, an equation contains only period-t+1 variables (or only period-t−1
variables), its derivative with respect to period-t variables is zero in the last (or first) period.
The stacked Jacobian is singular. This shows up when a Lagrange multiplier is the only variable
carrying the lead. This fails:

```dynare
Lambda = beta*C(-1)/C;
Lambda(+1)*R(+1) = 1;
```

Substitute the multiplier out:

```dynare
beta*C/C(+1)*R(+1) = 1;
```

`model(differentiate_forward_vars)` creates an auxiliary variable `AUX_DIFF_FWRD` equal to `x-x(-1)`
for each endogenous variable with a lead. Its terminal condition is 0, which helps convergence under
a very persistent or permanent shock. That is Dynare's auxiliary variable, not a helper you write.

## Results

- `oo_.endo_simul`: endogenous paths. Columns are periods. Column 1 is the initial condition. The
  last column is the terminal condition.
- `oo_.exo_simul`: exogenous paths.
- With dates (`first_simulation_period=2000Q1`), the paths are also in `Simulated_time_series`.
- Quick look: `rplot c;` `rplot k;`.

## Controlled endogenous paths

Pin some endogenous variables and let Dynare solve for the same number of exogenous variables. This
is the deterministic form of a conditional forecast.

```dynare
perfect_foresight_controlled_paths;
   exogenize c;
   periods 2 4:5;
   values 1.6 1.7;
   endogenize x;
   exogenize k;
   periods 7:9;
   values 13;
   endogenize z;
end;
perfect_foresight_setup(periods=100);
perfect_foresight_solver;
```

The number of controlled endogenous variables equals the number of freed exogenous variables. The
block can sit next to an ordinary `shocks` block if the two do not clash. It needs
`stack_solve_algo` in {0, 1, 2, 3, 6, 7}. It is incompatible with `block` and `bytecode`. Under
expectation errors, `learnt_in=period` says when agents learn that plan.

## `shock_paths`

One block can replace `shocks`, `mshocks`, `endval` and `perfect_foresight_controlled_paths`. Do not
combine it with those (E113). A name in `values` needs a scope prefix:

| Prefix | Meaning |
|---|---|
| `initval.X` or `init.X` | Value from the `initval` block (the steady state, if `steady` follows `initval`) |
| `self.X(-1)` | This exogenous variable's own earlier value in this block (an AR path) |
| `DBNAME.X` | A column of a `database` |
| `prev.X` | Expectation errors: the value from the previous information set |
| `learnt_in(p).X` | Expectation errors: the value from information set p |

`periods` that include `end` are a permanent shock and turn on `endval_steady`.

```dynare
db = table(transpose(linspace(0,1,101)),'VariableNames',{'foo'});
database db;
shock_paths;
   var x;
   periods 1, 2:5, 6:end;
   values initval.x*1.05, self.x(-1)*1.05, self.x(-1);
   var y;
   periods 1:3;
   values db.foo;
   exogenize c; periods 2,4:5; values 1.6,1.7; endogenize z;
end;
```

Expressions need no extra parentheses. `database NAME;` declares the table first.

## Perfect foresight with expectation errors

Commands: `perfect_foresight_with_expectation_errors_setup` and `_solver`. The terminal condition is
always treated as a steady state (implicit `endval_steady`; Dynare recomputes it when expectations
change).

```dynare
shocks(learnt_in=1);
  var x; periods 1:2 3:4 5; values 1 1.2 1.4;
end;
shocks(learnt_in=2);
  var x; periods 3:4; add 0.1;        // add, or multiply
end;
endval(learnt_in=3);
  x = 1.1;  y += 0.1;  z *= 2;
end;
perfect_foresight_with_expectation_errors_setup(periods=30);
perfect_foresight_with_expectation_errors_solver;
```

A CSV can hold the whole information set (`datafile=…`): `p+3` rows and `k*p+1` columns. Results:
`oo_.pfwee.shocks_info(k,t,s)` and `oo_.pfwee.terminal_info(k,s)`. Learnt-in shocks without these
commands are E422–E425.

## Complementarity conditions (`lmmcp`)

For a deterministic occasionally binding constraint, write the condition after the equation with `⟂`
(U+27C2) or ASCII `_|_`, and solve with `lmmcp`. Do not write "MCP" for this. The older `[mcp='…']`
tag is obsolete (Dygnosis W170). An `[mcp]` tag together with `⟂` is E180.

```dynare
model;
   r = rho*r(-1) + (1-rho)*(gpi*Infl+gy*YGap) + e  ⟂  r > -1.94478;
end;
perfect_foresight_setup(periods=200);
perfect_foresight_solver(lmmcp);     // stack_solve_algo=7 and solve_algo=10
```

Attach the condition to the equation it belongs to. Dynare's residual is left-hand side minus
right-hand side. A lower bound needs a positive residual, so the interest rate stays on the left.
Both bounds: `… ⟂ -1.94478 < r < 1+2*alpha;`. The same operator works with `extended_path(lmmcp)`.
This avoids `max` and `min` (R6). Check the form with `dynare_diagnose` (E183, E262–E265).

## Paths from a file

```dynare
initval_file(datafile='mydata.csv');
perfect_foresight_setup(periods=200);
perfect_foresight_solver;
```

Column names match variable names. The first column may be a date. Options: `first_obs`, `nobs`,
`first_simulation_period`, `last_simulation_period`, `series`. The path length is the number of
simulation periods plus lags plus leads (200 periods, 2 lags, 1 lead: at least 203 rows). Do not
combine it with `initval`. A later `histval` or `histval_file` can still replace the history.
`histval_file` is often filled by `smoother2histval`.

## Other commands

- Expectation errors: the pair above. News about a future shock uses `learnt_in`.
- Extended path: `extended_path(periods=…)`. It needs `periods` (E216).
- OccBin: [occbin.md](occbin.md). Do not put `occbin_solver` in this file together with a stochastic
  command.

## Course examples

Folder: `references/examples-code/Dynare_Course/Chapter_11_perfect_foresight/`.
Shared equations: `rbc_model_eq.inc` (the `det` files use `@#include`).
Search: `grep -iE "perfect_foresight|extended_path|expectation_errors|endval|histval|lmmcp" references/catalog-code.csv`.

| File | What it shows |
|---|---|
| `rbc_basic.mod` | Setup and solver; return after a temporary shock |
| `rbc_det1.mod` | `histval`: initial capital off the steady state |
| `rbc_det2.mod` | One unanticipated period in `shocks` |
| `rbc_det3.mod` | Several anticipated future periods (`4`, `5:8`) |
| `rbc_det4.mod` | `endval`: permanent change, new steady state |
| `rbc_det5.mod` | `endval` plus a dated shock |
| `rbc_det_dates.mod` | `first_simulation_period=2025Q2` |
| `rbc_ep.mod` | `extended_path` |
| `rbc_expectation_errors.mod` | `shocks(learnt_in=…)` |
| `rbcii.mod` | Irreversible investment. The file uses an `[mcp]` tag; write `⟂` instead (W170) |
| `nk3.mod` | Three-equation NK baseline |
| `nk3_zlb_det.mod` | Liquidity trap under perfect foresight |
| `nk3_zlb_det_anticipated_exit.mod` | Announced exit from the ZLB |
| `nk3_zlb_det_unexpected_exit.mod` | Surprise exit: two solves |
| `nk3_zlb_ep.mod` | ZLB with `extended_path` |
| `nk3_zlb_stoch.mod` | `stoch_simul` on a ZLB model, on purpose: perturbation misses the kink. Use OccBin or perfect foresight. Copy warning: it uses `max` under `stoch_simul` (W200). |

The anticipated and unexpected exit files share the equations. They differ in when agents learn that
the constraint will relax.

Official examples: `<dynare-root>/examples/perfect_foresight/`. Manual: "Deterministic simulation"
and "Perfect foresight". Run Dynare from `SKILL.md` "Numerical runs".
