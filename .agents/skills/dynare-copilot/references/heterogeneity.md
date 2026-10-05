# Heterogeneous-agent models (HANK, Krusell-Smith)

Read this when the task involves heterogeneous agents, HANK, Krusell-Smith, a wealth distribution that
affects individual choices, one- or two-asset HANK, or the sequence-space Jacobian, or uses
`heterogeneity_dimension`, `var(heterogeneity=…)`, `model(heterogeneity=…)`, `SUM()` or a
`heterogeneity_*` command.

This file covers how Dynare organizes a heterogeneous-agent model: declarations and blocks, the
steady-state, solve and simulate commands, and where the results go. The framework is recent and still
changes between releases. Before you write block contents, compare with the official examples in
`<dynare-root>/examples/heterogeneity/` and the manual section "Heterogeneity" of the installed Dynare
version. Do not write block details from memory.

Dynare solves models with a continuum of agents that differ in wealth, income or employment status.
Aggregate dynamics come from the interaction of individual decisions with the distribution of agents'
states; HANK is one case. The solution method combines Bhandari-Bourany-Evans-Golosov (2023) and
Auclert-Bardóczy-Rognlie-Straub (2021, sequence-space Jacobian); Rion (2026) documents the algorithms
(manual, "Heterogeneity"). A model has two layers: the **heterogeneous (individual) layer** and the
**aggregate layer**. The aggregation operator `SUM()` links them.

## Declarations and operators

- `heterogeneity_dimension NAME;` declares a heterogeneity dimension (for example households
  distributed over wealth and productivity). It must come before every declaration that uses it.
  Dynare 7.2 supports **one** dimension per model. The preprocessor accepts a second distinct dimension,
  but the steady-state routines refuse it (Dygnosis W207). A repeated name is E460; an option that names
  an undeclared dimension is E459.
- `var(heterogeneity=NAME) …;` / `varexo(heterogeneity=NAME) …;` declare heterogeneous endogenous and
  exogenous variables (one value per agent).
- `model(heterogeneity=NAME); … end;` is the **heterogeneous agent model block**. It holds the
  individual first-order conditions and constraints (Euler equation, budget constraint). Each equation
  holds for every agent in that dimension. Restrictions (manual, "Heterogeneous Agent Model Block"):
  - A heterogeneous exogenous variable appears only at time t: a lag is E469, a lead is E470.
  - A heterogeneous endogenous variable appears only at t-1, t and t+1: E471 (lag beyond -1),
    E472 (lead beyond +1).
  - An expression that combines leads (≥ 1) with lagged states (-1) must be separable (E473):
    `+`, `-`, `=` are always separable; `*` is separable if one factor holds all the leads; `/` only
    when the numerator holds the leads (`c(+1)/a(-1)` passes, `a(-1)/c(+1)` fails); a unary function
    whose argument holds both fails (`log(k(-1) + c(+1))`); `^` and other nonlinear binary operators
    fail when the operands span both.
  - `SUM` is not allowed inside this block (E475).
  - Complementarity conditions use `⟂` (U+27C2) or ASCII `_|_` after the equation, with the same
    conventions as the perfect-foresight option `lmmcp` (R6):
    `c^(-1/eis)-beta*Va(+1)=0 ⟂ a>=0;`. An `[mcp=…]` tag is refused here (E479).
- `shocks(heterogeneity=NAME); var eps_e; stderr 0.01; end;` is the **heterogeneous shocks block**
  (variances of heterogeneous exogenous variables). A variance, standard error, covariance or
  correlation on any other name is refused (E465–E468).
- `SUM(x)` in the aggregate model block integrates a heterogeneous endogenous variable over the
  stationary distribution (`SUM(a)` = aggregate assets). The argument must be one variable (E476),
  without a lead or lag (E477), and a heterogeneous endogenous variable (E478). Parameters, exogenous
  variables and `SUM(a+b)` are not valid arguments; write `SUM(a) + SUM(b)`.
- A heterogeneous symbol cannot appear outside the model blocks (E463), in `planner_objective` (E461),
  in `occbin_constraints` (E462) or in `epilogue` (E464).

The aggregate layer uses ordinary `var`, `varexo`, `shocks` and `model` (without `heterogeneity=`) for
aggregate identities, market clearing and policy rules. It reaches individual values only through
`SUM(...)`.

The manual documents the parts in this order: heterogeneity dimension, heterogeneous variables,
heterogeneous agent model block, heterogeneous shocks block, aggregate variables, aggregate shocks block,
aggregate model block. That is the order of the manual sections, not a file order. In the file, declare
every symbol before the first block that uses it; the official examples put all declarations first.
Dynare 7.2 refuses a heterogeneous block that uses an aggregate variable declared later
(`Unknown symbol: r`; Dygnosis E020).

```dynare
// Conceptual skeleton: check the exact block contents against the official examples
heterogeneity_dimension hh;                 // household dimension

var(heterogeneity=hh) a c;                  // individual assets, consumption (vary across agents)
varexo(heterogeneity=hh) idio_e;            // idiosyncratic productivity state

var K r w;                                  // aggregate variables, declared before any model block

parameters bet gam r_ss ...;

model(heterogeneity=hh);                    // individual problem (holds for each agent)
   c^(-gam) = bet*(1+r)*c(+1)^(-gam);       // Euler equation; the examples add a borrowing limit: ... ⟂ a >= 0;
   a = (1+r)*a(-1) + w*idio_e - c;          // individual budget / asset accumulation
end;

model;                                      // aggregate layer
   K = SUM(a);                              // aggregate capital = sum of individual assets
   r = alpha*(K(-1))^(alpha-1) - delta;     // factor prices
   w = (1-alpha)*(K(-1))^alpha;
end;
```

## Equation count (R4)

Count each layer separately: aggregate `model` equations against aggregate `var` names, and, for each
dimension, `model(heterogeneity=…)` equations against that dimension's `var(heterogeneity=…)` names.
`#` model-local variables and `[static]` rows do not count. Dynare refuses a mismatch:
`There are 1 equations but 2 endogenous variables in the model for heterogeneity dimension 'hh'!`
(verified with the 7.2 preprocessor). Dynare counts again after it adds auxiliary variables for some
leads, so its numbers can differ from the written counts.

Check the written counts with Dygnosis `dynare_model_info` (per-dimension names and counts) and
`dynare_equations` (`count_gap`); Dygnosis reports a heterogeneous mismatch as E192 or W208, and an
aggregate mismatch as E188 or W013. See `references/dygnosis-workflow.md`.

## Commands Dynare refuses on heterogeneous models

When a heterogeneity dimension is declared, Dynare 7.2 refuses `check`, `steady`, `stoch_simul`,
`estimation`, the perfect-foresight solvers, `extended_path`, `osr` / `osr_params` / `optim_weights`,
`ramsey_model` / `ramsey_policy`, `discretionary_policy`, identification, sensitivity analysis,
`method_of_moments`, `occbin_constraints` and the `block` option of `model`
(`The 'check' command is not supported for heterogeneous models`; Dygnosis E474). Use only the
`heterogeneity_*` commands below for the steady state, the solution and simulation. The usual
`steady;` / `check;` steps of debugging.md do not apply.

## Steady state, solution and simulation

The steady state of a heterogeneous model is the set of policy functions, the discretized idiosyncratic
shocks, the stationary distribution and the aggregate values. Dynare gets it in one of two ways.

1. **Load a precomputed steady state**: `heterogeneity_load_steady_state(filename = FILE);` reads a
   steady-state structure from a MAT file, or from a workspace variable with `variable = NAME`
   (default `'steady_state'`; useful when a `verbatim` block builds it). It checks the residuals of the
   aggregate equations up to `tolf` (default `1e-6`). It does **not** check the heterogeneous equations;
   you are responsible for them. Structure fields:
   - `steady_state.agg`: scalar steady-state value of each aggregate `var` (Dynare computes its own
     aggregate auxiliary variables).
   - `steady_state.pol.grids` (state grids, column vectors), `steady_state.pol.values` (policy
     arrays for every `var(heterogeneity=…)` variable), `steady_state.pol.order` (dimension order,
     shocks first, then states, e.g. `{'e', 'a'}`).
   - `steady_state.shocks.grids` and `steady_state.shocks.Pi` (Markov transition matrices, rows sum
     to 1) for every `varexo(heterogeneity=…)` variable. The helper `rouwenhorst` builds both
     (manual, "Helper functions").
   - `steady_state.d.hist` (stationary histogram, sums to 1), optional `steady_state.d.grids` and
     `steady_state.d.order`.
2. **Compute it in Dynare**: `heterogeneity_compute_steady_state(variable = initial_guess);` takes an
   initial guess with the same fields (`d.hist` is ignored) plus optional
   `steady_state.free_parameters.<name>.initial_guess` / `.lower_bound` / `.upper_bound`. Each
   residual evaluation runs time iteration for the policy functions (complementarity conditions are
   handled with a Fischer-Burmeister function), forward iteration for the distribution, and aggregation
   of the `SUM` terms; a Broyden solver moves the free parameters until the target equations hold.
   - Target equations default to the aggregate equations that contain `SUM`. Override with
     `calibration_target_equations=['name', …]`; use `name` tags rather than indices (R1). The number of
     targets must equal the number of free parameters.
   - The command takes the aggregate endogenous values as given. Supply aggregate values that give a
     zero residual on the non-target aggregate equations.
   - Options: `calibration_tolf` (1e-4), `calibration_max_iter` (50), `calibration_verbosity`,
     `time_iteration_max_iter` (1000), `time_iteration_tol` (1e-8), `time_iteration_learning_rate` (1),
     `time_iteration_early_stopping` (3), `time_iteration_verbosity`, `time_iteration_solver_tolf` and
     `time_iteration_solver_tolx` (1e-10), `time_iteration_solver_factor` (100),
     `time_iteration_solver_max_iter` (1000), `time_iteration_solver_stop_on_error`,
     `forward_max_iter` (10000), `forward_tol` (1e-10), `forward_check_every` (100),
     `forward_verbosity` (1). Dygnosis `dynare_list_options` lists the valid options of the installed
     version.
   - It updates `M_.params` with the calibrated values.

After the steady state:

- `heterogeneity_solve(truncation_horizon = 300);` computes the linearized solution of the aggregate
  dynamics (sequence-space Jacobians). `truncation_horizon` defaults to 300.
- `heterogeneity_simulate(OPTIONS…) [VARIABLE_NAME…];` computes IRFs and stochastic simulations of
  unanticipated shocks drawn from the aggregate `shocks` block (`periods > 0` adds simulated paths). If
  the `shocks` block uses `periods` and `values`, it switches to **news shock sequence** mode: agents
  learn at t=0 about the whole sequence of future shocks. Options: `irf` (40), `periods` (0),
  `irf_shocks`, `relative_irf`, `nograph`, `nodisplay`, `graph_format`, `tex`, `irf_plot_threshold`,
  `print`, `noprint`. News shock mode does not accept `irf`, `periods`, `irf_shocks` or `relative_irf`.

```dynare
heterogeneity_compute_steady_state(variable = initial_guess);   // or heterogeneity_load_steady_state(filename = FILE);
heterogeneity_solve(truncation_horizon = 300);
heterogeneity_simulate(irf = 80);   // IRFs and simulation; news shock mode when the shocks block has periods/values
```

## Official examples (start from these)

`<dynare-root>/examples/heterogeneity/` in Dynare 7.2 (models and calibration as in the
sequence-jacobian toolkit of Auclert et al. 2021):

| File | Shows |
| --- | --- |
| `krusell_smith_1998.mod` | Krusell-Smith (1998); `heterogeneity_load_steady_state(filename = krusell_smith_1998)`, `heterogeneity_solve(truncation_horizon = 400)`, `heterogeneity_simulate(irf = 80)` |
| `krusell_smith_1998_steady_state.mod` | same model; initial guess built in `verbatim`, then `heterogeneity_compute_steady_state(variable = initial_guess)` |
| `hank_one_asset.mod` | one-asset HANK; loads `hank_one_asset.mat`, solves, `heterogeneity_simulate(periods = 1000)` |
| `hank_one_asset_steady_state.mod` | one-asset HANK; compute with calibration (`calibration_target_equations=['Asset market clearing', 'Labor market clearing']`) |
| `hank_two_assets.mod` | two-asset (liquid / illiquid) HANK; loads the steady state; news shock sequence (`shocks` with `periods` / `values`, then `heterogeneity_simulate;`) |
| `hank_two_assets_steady_state.mod` | two-asset HANK; compute with three calibration targets and tightened time-iteration options |

## Practical points

- **The steady state is the hard part.** A steady state from mature external code (for example the
  sequence-jacobian toolkit) loaded with `heterogeneity_load_steady_state` is often more robust than
  `heterogeneity_compute_steady_state`. Choose by model complexity. Manual advice for `compute`: grid
  homotopy (solve on a coarse grid, save `oo_.heterogeneity.steady_state` from a `verbatim` block,
  interpolate onto a finer grid, solve again); a coarse `pol.grids` with a denser `d.grids`; lower
  `time_iteration_learning_rate` (0.5–0.8) when time iteration oscillates; keep `calibration_tolf`
  looser than `time_iteration_tol`. Numerical convergence does not prove the solution is economically
  meaningful.
- The individual layer holds only individual first-order conditions and constraints. Where an aggregate
  equation needs the cross-sectional total of a heterogeneous endogenous variable, write `SUM(a)`;
  writing `a` does not aggregate. A heterogeneous exogenous variable or a parameter cannot be a `SUM()`
  argument; follow the official examples for how they enter aggregate equations.
- R3 does not apply to `varexo(heterogeneity=…)`: such a variable is a discretized idiosyncratic state
  (grid and transition matrix in `steady_state.shocks`) and appears only at time t.
- This framework is for a **continuous distribution** whose shape is itself a state. A model with a few
  discrete household types (TANK, a small number of agent types) does not need it: write an ordinary
  `.mod` and generate the types with macro-processor loops (macro-processor.md).
- The framework and its options can change between releases. The example files and the manual of the
  installed Dynare version are the authority for exact keywords; this file gives the structure and the
  command list.

## Running

- Use a Dynare version that includes the heterogeneity framework; Dygnosis follows Dynare 7.2.
- Run official Dynare under MATLAB or Octave through the route the host offers (a MATLAB MCP server,
  `matlab -batch "…"`, `octave --eval "…"`). If no route exists, say what could not be run and give the
  commands to the user.
- Run an official example first (`krusell_smith_1998.mod`, `hank_one_asset.mod`) to confirm the
  installation, then adapt it to your model.
- When you load a steady state, check that the MAT-file fields (policy functions, shock discretization,
  stationary distribution) match the model's variables and grid dimensions.
- Static checks (Dygnosis) say nothing about convergence of the steady state or the solution; only the
  Dynare run does.

## Pitfalls

**`heterogeneity_solve` fails with `eq` undefined in `process_jacobian_block` (cause: `varexo` order)**

Observed with Dynare 7.1; not re-tested with 7.2. The aggregate layer has a `varexo` that appears only
with a lag (typical: a Taylor rule with an implementation lag `rstar(-1)`). Dynare creates an auxiliary
equation `aux(+1) = rstar` for it, whose row index is **greater** than `M_.orig_endo_nbr`.
`heterogeneity_solve` walks the Jacobian blocks with `find()`, and MATLAB's `find()` returns entries in
**column-major** order. If the lag-only `varexo` is declared **first** in the `varexo` statement, its
auxiliary row is visited before the regular equation entries, so `eq` is used before it is assigned.

**Fix: declare every lag-only `varexo` last in the `varexo` statement.** The regular entries are then
visited first and assign `eq`. The Dynare 7.2 examples `hank_one_asset.mod` and
`hank_one_asset_steady_state.mod` follow this order (`varexo G markup rstar;` with `rstar(-1)` in the
Taylor rule).

```dynare
// Correct: the lag-only rstar is declared last
varexo G TR markup rstar;     // rstar(-1) enters the Taylor rule, so declare it last
// Wrong (triggers the failure): varexo rstar G TR markup;
```

In a **non-heterogeneous** model, the same lag-only `varexo` caused a different failure in `disp_dr`
(`subst_auxvar`, "index produces 2 values") with Dynare 7.1. There, replace the lagged `varexo` with an
AR(1) endogenous process driven by an innovation (R3); see the entry "Plain model: `disp_dr` /
`subst_auxvar` crash with a lag-only exogenous variable" in `references/known-issues.md`.

**A pure `varexo` is 0 in the steady state**

When `G`, `TR` and similar are pure `varexo` (not AR endogenous processes), their steady-state value is
0. Do not substitute them as positive values in steady-state equations such as the government budget: if
`Tax = r*B + G + TR`, the steady state is `Tax_ss = r_ss*B` (`G = TR = 0`). Writing
`r_ss*B + G_ss + TR_ss` leaves a nonzero steady-state residual. The official `hank_two_assets.mod` writes
the level as a parameter plus the shock: `(r * Bg + G_ss + G) / w / N - tax;`.

## IRFs and other results

- `heterogeneity_simulate` stores IRFs in `oo_.irfs` (fields `<var>_<shock>`), simulated endogenous
  paths in `oo_.endo_simul` (when `periods > 0`, or in news shock mode) and news shock paths in
  `oo_.exo_simul` (manual, "Simulating").
- `heterogeneity_solve` stores the sequence-space Jacobians in `oo_.heterogeneity.dr.G`;
  `oo_.heterogeneity.steady_state` holds the steady state (`agg`, `pol`, `shocks`, `d`).
- `oo_.heterogeneity.dr.G.<var>.<shock>` is a **T×T matrix** (T = `truncation_horizon`). Entry `(t,s)` is
  the linear response of `<var>` in period t to a unit deviation of `<shock>` in period s, known at
  time 0.
- The IRF to a one-time shock in the first period is the **first column**:
  `oo_.heterogeneity.dr.G.Y.G(:,1)` is the response of Y to a unit G shock.
- These are responses **per unit of shock**; multiply by the shock size (for example 1%).
- For an aggregate that the model does not store (for example aggregate consumption C), derive it from
  an identity of stored variables (goods market: `C = Y - G`).

```matlab
% IRF of Y to a unit G shock (first 20 periods), scaled to a 1% shock
irf_Y_G = oo_.heterogeneity.dr.G.Y.G(1:20, 1) * 0.01;
```

To compare with a representative-agent model (`oo_.irfs` from `stoch_simul`) or to iterate on plots,
save `oo_` to a MAT file once and analyze the saved copy; do not rerun the HANK solution (tens of
seconds) for every plot change. See `references/matlab-workflow.md`.

References: Dynare manual, "Heterogeneity"; the official examples above; Auclert, Bardóczy, Rognlie and
Straub (2021); Bhandari, Bourany, Evans and Golosov (2023); Rion (2026).
