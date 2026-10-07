# Templates (starting skeletons for `.mod` files)

Read this when you need a ready skeleton to start a model: basic RBC, three-equation New Keynesian,
perfect foresight transition, macro-processor variants, LaTeX output.

The skeletons follow Johannes Pfeifer's DSGE_mod house style: a header comment block, a TeX name and
`long_name` on every declaration, a `name` tag on every equation, a closed-form steady state with
back-solved calibration in `steady_state_model`, `log_*` helper variables for plots, and the
`resid; steady; check;` sequence. Adapt them to the user's model. Do not copy the numbers unchanged.

## Labels, comments and names

- **R1.** Comments follow the user's language. Identifiers, `long_name` values, equation tags and TeX
  names stay English ASCII. This is house style, not a Dynare rule: Dynare 7.2 accepts UTF-8 in comments
  and quoted strings, but ASCII labels keep MATLAB/Octave output, TeX and plots portable. The templates
  use English comments. Dygnosis reports missing metadata as Information: I208 (equation without a
  `name` tag) and I209 (declaration without `long_name`). Neither is a Dynare refusal.
- **R5.** The templates use `betta`, `alppha`, `gam` and `invest`. `psi` and `pi` are also MATLAB
  function names. They are safe with `steady_state_model`; rename them if you move the steady state to a
  user-written steady-state file (`<model>_steadystate.m`).
- Helper variables such as `log_y = log(y);` are ordinary endogenous variables that you write. Do not
  call them auxiliary variables: in Dynare, auxiliary variables are the `AUX_*` variables that Dynare
  creates.

## Starting points in the local libraries

When the user's model is close to a known paper, start from the matching DSGE_mod file in
`references/examples-code/<Folder>/<CodeID>.mod` (lookup: `references/catalog-lookup.md`):

| Model family | Local file (Folder/CodeID) |
|---|---|
| RBC | `RBC_baseline/RBC_baseline`, `Hansen_1985/Hansen_1985` |
| Basic NK | `Gali_2015/Gali_2015_chapter_3` (linear), `Gali_2015/Gali_2015_chapter_3_nonlinear` |
| Medium-scale estimation | `Smets_Wouters_2007/Smets_Wouters_2007_45` |
| Open economy | `SGU_2003/SGU_2003`, `Gali_Monacelli_2005/Gali_Monacelli_2005` |
| Optimal policy | `Gali_2015/Gali_2015_chapter_5_commitment`, `Gali_2015/Gali_2015_chapter_5_discretion` |
| Occasionally binding constraints | `Dynare_Course/Chapter_12_OccBin/NK_occbin` |

DSGE_mod files that are not in the local library (for example `GarciaCicco_et_al_2010`,
`Guerrieri_Iacoviello_2015`) are at [github.com/JohannesPfeifer/DSGE_mod](https://github.com/JohannesPfeifer/DSGE_mod).

## Template 1: basic RBC (stochastic simulation)

```dynare
/*
 * Basic RBC model with a TFP shock, stochastic simulation.
 * Timing: "stock at the end of the period" convention; capital enters production as k(-1).
 * Closed-form steady state; steady-state labor is fixed at l = 1/3 and psi is back-solved.
 */

//==================== Declarations ====================
var y      ${y}$      (long_name='output')
    c      ${c}$      (long_name='consumption')
    k      ${k}$      (long_name='capital')
    l      ${l}$      (long_name='labor')
    invest ${i}$      (long_name='investment')
    z      ${z}$      (long_name='log TFP')
    log_y  ${\log y}$ (long_name='log output')
    log_c  ${\log c}$ (long_name='log consumption')
    log_k  ${\log k}$ (long_name='log capital')
    log_l  ${\log l}$ (long_name='log labor') ;
varexo eps_z ${\varepsilon_z}$ (long_name='TFP shock') ;
parameters
    betta  ${\beta}$   (long_name='discount factor')
    sigma  ${\sigma}$  (long_name='risk aversion')
    alppha ${\alpha}$  (long_name='capital share')
    delta  ${\delta}$  (long_name='depreciation rate')
    rhoz   ${\rho_z}$  (long_name='TFP persistence')
    psi    ${\psi}$    (long_name='labor disutility') ;

//==================== Calibration ====================
betta  = 0.99;
sigma  = 1;
alppha = 0.33;
delta  = 0.025;
rhoz   = 0.95;
// psi is back-solved in steady_state_model

//==================== Model ====================
model;
// Euler equation: intertemporal consumption choice
[name='Euler equation']
c^(-sigma) = betta*c(+1)^(-sigma)*(alppha*exp(z(+1))*k^(alppha-1)*l(+1)^(1-alppha) + 1 - delta);
// Labor FOC: consumption-leisure trade-off
[name='labor FOC']
psi*c^sigma/(1-l) = (1-alppha)*exp(z)*k(-1)^alppha*l^(-alppha);
// Law of motion of capital (end-of-period stock on the left)
[name='law of motion of capital']
k = invest + (1-delta)*k(-1);
// Resource constraint
[name='resource constraint']
y = c + invest;
// Production function (capital is predetermined, hence k(-1))
[name='production function']
y = exp(z)*k(-1)^alppha*l^(1-alppha);
// TFP process (z is endogenous; only eps_z is exogenous)
[name='TFP process']
z = rhoz*z(-1) + eps_z;
[name='log output'] log_y = log(y);
[name='log consumption'] log_c = log(c);
[name='log capital'] log_k = log(k);
[name='log labor'] log_l = log(l);
end;

//==================== Steady state (closed form + back-solved calibration) ====================
steady_state_model;
    z = 0;
    l  = 1/3;                                           // calibration target
    kl = ((1/betta - 1 + delta)/alppha)^(1/(alppha-1)); // capital-labor ratio
    k  = kl*l;
    invest = delta*k;
    y  = kl^alppha*l;
    c  = y - invest;
    psi = (1-alppha)*kl^alppha*(1-l)/c^sigma;           // back-solve psi to hit l = 1/3
    log_y = log(y); log_c = log(c); log_k = log(k); log_l = log(l);
end;

//==================== Checks ====================
resid;
steady;
check;

//==================== Shocks ====================
shocks;
    var eps_z; stderr 0.007;
end;

//==================== Experiment ====================
stoch_simul(order=1, irf=40, hp_filter=1600) log_y log_c log_k log_l z;
```

## Template 2: three-equation New Keynesian model (gap form, stochastic simulation)

```dynare
/*
 * Basic three-equation New Keynesian model (Gali gap form).
 * All variables are log deviations from the zero-inflation steady state, so the steady state is all zeros.
 * The source defines the model in linearized form, so this file uses model(linear) (R8).
 * For a nonlinear version (needed, for example, for second-order welfare analysis),
 * start from DSGE_mod Gali_2015_chapter_3_nonlinear.
 */

//==================== Declarations ====================
var x    ${x}$   (long_name='output gap')
    pi   ${\pi}$ (long_name='inflation')
    inom ${i}$   (long_name='nominal interest rate')
    rn   ${r^n}$ (long_name='natural rate of interest (AR(1) demand process)')
    v    ${v}$   (long_name='monetary policy shock process') ;
varexo eps_a ${\varepsilon_a}$ (long_name='demand / natural-rate innovation')
       eps_v ${\varepsilon_v}$ (long_name='monetary policy innovation') ;
parameters
    betta  ${\beta}$    (long_name='discount factor')
    sigma  ${\sigma}$   (long_name='inverse EIS')
    kappa  ${\kappa}$   (long_name='NKPC slope')
    phi_pi ${\phi_\pi}$ (long_name='Taylor rule inflation coefficient')
    phi_x  ${\phi_x}$   (long_name='Taylor rule output-gap coefficient')
    rho_a  ${\rho_a}$   (long_name='demand process persistence')
    rho_v  ${\rho_v}$   (long_name='monetary shock persistence') ;

//==================== Calibration ====================
betta  = 0.99;  sigma = 1;   kappa = 0.13;
phi_pi = 1.5;   phi_x = 0.125;
rho_a  = 0.90;  rho_v = 0.50;

//==================== Model ====================
model(linear);
// Dynamic IS curve (Euler equation in gap form)
[name='Dynamic IS curve']
x = x(+1) - (1/sigma)*(inom - pi(+1) - rn);
// New Keynesian Phillips curve
[name='New Keynesian Phillips Curve']
pi = betta*pi(+1) + kappa*x;
// Taylor rule (monetary policy)
[name='Taylor rule']
inom = phi_pi*pi + phi_x*x + v;
// Demand / natural-rate process (rn is endogenous; only eps_a is exogenous)
[name='demand / natural-rate process']
rn = rho_a*rn(-1) + eps_a;
// Monetary policy process (v is endogenous; only eps_v is exogenous)
[name='monetary policy process']
v = rho_v*v(-1) + eps_v;
end;

//==================== Steady state (all zeros) ====================
steady_state_model;
    x = 0; pi = 0; inom = 0; rn = 0; v = 0;
end;

//==================== Checks ====================
resid;
steady;
check;     // determinacy here needs roughly phi_pi > 1 (Taylor principle); check decides

//==================== Shocks ====================
shocks;
    var eps_a; stderr 0.01;
    var eps_v; stderr 0.0025;
end;

//==================== Experiment ====================
stoch_simul(order=1, irf=20) x pi inom rn;
```

The interest rate is `inom`, not `i`: the manual asks users not to name a variable `i` (R5). Galí's
files and other examples use `i`; rename it when you copy from them.

## Template 3: perfect foresight (transition after a permanent shock, skeleton)

```dynare
/*
 * Perfect foresight transition after a permanent TFP increase (x from 1 to 1.1).
 * x is an exogenous variable that carries the path itself (R3); x(+1) is fine because the path is known.
 */
var c ${c}$ (long_name='consumption')
    k ${k}$ (long_name='capital') ;
varexo x ${x}$ (long_name='TFP level') ;
parameters
    alppha ${\alpha}$ (long_name='capital share')
    betta  ${\beta}$  (long_name='discount factor')
    delta  ${\delta}$ (long_name='depreciation rate')
    gam    ${\gamma}$ (long_name='risk aversion') ;
alppha = 0.33; betta = 0.99; delta = 0.025; gam = 1;

model;
// Resource constraint (homogeneous form; "= 0" omitted)
[name='resource constraint']
c + k - x*k(-1)^alppha - (1-delta)*k(-1);
// Euler equation
[name='Euler equation']
c^(-gam) - betta*(alppha*x(+1)*k^(alppha-1) + 1 - delta)*c(+1)^(-gam);
end;

initval;                       // initial steady state (x at its old level)
   x = 1;
   k = ((1/betta - 1 + delta)/(alppha*x))^(1/(alppha-1));
   c = x*k^alppha - delta*k;
end;
steady;

endval;                        // terminal steady state (x permanently rises to 1.1)
   x = 1.1;
end;
steady;

perfect_foresight_setup(periods=200);
perfect_foresight_solver;

rplot c; rplot k;
```

Keep `x = 1;` in `initval`: an exogenous variable missing from `initval` starts at zero, and TFP must
not be zero. With the perfect-foresight commands below, Dygnosis keeps W051 quiet for that assignment.

## Macro processor: several variants in one file

```dynare
@#define rule_type = 1     // 1 = Taylor rule; 0 = money growth rule

model;
   ...
@#if rule_type == 1
   // Taylor rule
   [name='Taylor rule']
   inom = phi_pi*pi + phi_x*x + v;
@#else
   // Money growth rule
   [name='money growth rule']
   ...
@#endif
   ...
end;
```

The macro processor also supports `@#for` loops (multi-country or multi-sector models), `@#include` for
modular files, and more. Full syntax, operators, comprehensions and typical uses:
`references/macro-processor.md`. To see the text after macro expansion, use the Dygnosis tool
`dynare_expand` (`references/dygnosis-workflow.md`).

## LaTeX output (optional, for checking the model)

```dynare
write_latex_definitions;                         // names, TeX names and long names of the variables
write_latex_parameter_table;                     // parameter table; call it after steady
write_latex_original_model(write_equation_tags); // equations as written, with name tags
write_latex_dynamic_model;                       // dynamic model after Dynare's transformations
write_latex_static_model;                        // static version of the model block
write_latex_steady_state_model;                  // contents of steady_state_model
// estimation: write_latex_prior_table;          // after estimated_params
```

- `write_latex_dynamic_model` and `write_latex_static_model` show the model after Dynare's transformations:
  `predetermined_variables` timing changed to the default convention, and `EXPECTATION`, leads and lags
  of two or more, and leads and lags of exogenous variables replaced by auxiliary variables.
- `write_equation_tags` is an option of `write_latex_original_model`, `write_latex_dynamic_model` and
  `write_latex_static_model`, not of `write_latex_steady_state_model`.
- `write_latex_steady_state_model` needs a `steady_state_model` block; without one Dynare refuses the file.
- LaTeX packages: `geometry`, `fullpage`, `breqn` for the model files; `longtable` for
  `write_latex_definitions`; `longtable`, `booktabs` for the parameter and prior tables.
- `write_latex_definitions`, `write_latex_parameter_table` and `write_latex_prior_table` are MATLAB/Octave
  commands; Dynare passes them to MATLAB/Octave unchanged.

---

## Manual notes (Dynare 7.2 manual: "Expressions", "Model declaration")

### `external_function` (call your own MATLAB/Octave function in the model)

A function used in the model block must return a scalar and must be declared before the model block:

```dynare
external_function(name=funcname);                          // nargs defaults to 1
external_function(name=g, nargs=2, first_deriv_provided, second_deriv_provided);
external_function(name=h, nargs=3, first_deriv_provided=h_deriv);
```

- Without `first_deriv_provided` / `second_deriv_provided`, Dynare uses finite differences.
- `second_deriv_provided` needs `first_deriv_provided` in the same statement.
- No declaration is needed for an external function used in an expression outside the `model` and
  `steady_state_model` blocks (for example in a parameter assignment). Inside `steady_state_model` the
  declaration is needed.

### Model-local variables and two operators

- `#z = MODEL_EXPR;` is a model-local variable: it shares a subexpression between equations. Its scope is
  the model block. With a lead or lag (`z(+1)`), Dynare shifts the whole expression. Do not declare it in
  `var`; `model_local_variable` can give it a TeX name.
- `STEADY_STATE(x)` takes the steady-state value (common in Taylor rules and output gaps). Exogenous and
  deterministic exogenous variables may not appear inside it.
- `EXPECTATION(-1)(x(+1))` takes the expectation with the previous period's information set. Dynare
  replaces it by an auxiliary variable (`AUX_EXPECT_LAG_1`) and a new equation.
