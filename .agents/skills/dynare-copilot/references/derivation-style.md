# Derivation note style

Read this when you write `<model>_derivation.md` in Stage 1 (derivation note). This file fixes the
structure, the LaTeX rules, the numbering and the symbol conventions, so that each note has the same
form, correct formulas, and a one-to-one match with the `.mod`.

This file covers form only. For the content of sections 2 to 4 (how to set up each agent's problem
(households, firms, government and central bank, market clearing), how to derive the first-order
conditions, and which variants change the equation structure), see modeling-blocks.md. Use the two
files together.

## Principles

- The derivation note is a mathematical document for a human reader, who can review it. It is not
  code: write formulas in standard LaTeX, not in Dynare syntax.
- One first-order condition = one number = one equation in the `.mod`. The `.mod` equation refers back
  to the number with `[name='...']`.
- Prose and headings follow the user's language (R1). Keep the section numbers and their order. When the
  prose is not English, give the English term at its first use, for example "Euler equation".
- Formulas use LaTeX symbols ($`\beta`$, $`C_t`$). The variable and parameter table (section 8) uses the
  model's ASCII names, the names that the `.mod` declares.

## Fixed structure (eight sections; keep the order and the headings)

Translate the headings when the prose is not English.

```markdown
# <Model name>: derivation (optimization problems and first-order conditions)

> This note is the blueprint for the Dynare file `<model>.mod`.

## 1. Model overview
- Model: <name>, source <paper / textbook chapter>.
- Experiment: <stochastic simulation / perfect foresight / estimation / optimal policy>; which shocks; what to look at.
- Agents: <households / firms / central bank / government / banks ...>, one sentence each on their role.
- Form: <nonlinear (default) / linearized model(linear)>, and why (normally nonlinear by default, R8).

## 2. Optimization problems
(One subsection per agent: objective and constraints, all as display math.)
> Only agents that optimize (households, firms, ...). Agents that do not optimize (exogenous government
> spending, a mechanical central-bank rule, a pure budget identity such as lump-sum taxes $`T_t=G_t`$) do
> not go in sections 2 and 3: put each such relation in section 4 (identities, market clearing) or in
> section 5 (exogenous processes). Then the FOC numbers of section 3 map one-to-one to the endogenous
> variables they determine, which makes the R4 check easy.

## 3. First-order conditions (FOC)
(Per agent; number each condition (F1), (F2), ...; one phrase of economic meaning may follow.)

## 4. Market clearing and aggregate identities
(Resource constraint, factor-market clearing, aggregation of heterogeneous agents; continue the numbering.
Mark the equation that Walras's law makes redundant.)

## 5. Exogenous processes
(Each AR process or shock; continue the numbering.)

## 6. Steady-state solution (ready to copy into steady_state_model)
- List the **steady-state system**: the conditions of sections 3 to 5 without time subscripts
  ($`X_{t}=X_{t+1}=\bar X`$).
- Give the **closed-form solution or the reverse-solve steps**, in an order that evaluates from top to
  bottom (the same order as steady_state_model): first the exogenous steady-state values (such as
  $`\bar A=1`$, shock means 0), then each endogenous steady-state value in turn.
  **Reverse-solve calibration targets**: if you calibrate a target such as $`\bar N=1/3`$, treat the
  matching parameter (such as $`\psi`$) as an unknown and solve for it.
- Linearized model: state that the steady state is zero (no step-by-step solution needed).
- Mark the **solution order** of each steady-state value, so that every value is computed before it is
  used (steady_state_model evaluates from top to bottom).

## 7. Timing and form conventions
(Stock at the end or at the beginning of the period, logs or levels, nonlinear or linearized.)

## 8. Variable and parameter table
(Three lists, with the equation that determines each endogenous variable; preview of Stage 2.)
```

## LaTeX rules

- **Display formulas** use the fenced `math` code block that GitHub supports. **Inline formulas** use the
  backtick-protected inline math that GitHub supports: dollar and backtick to open, backtick and dollar
  to close. Do not use LaTeX display delimiters such as `\[ ... \]`: GitHub Markdown does not treat them
  as math delimiters.
- **Optimization problems** use one form:
  ````markdown
  ```math
  \max_{\{C_t,N_t,B_t\}} \; E_0\sum_{t=0}^{\infty}\beta^t\, U(C_t,N_t)
  \quad\text{s.t.}\quad P_tC_t + Q_tB_t \le B_{t-1} + W_tN_t + \Pi_t
  ```
  ````
  Write the objective and the constraints together, the constraints after `\quad\text{s.t.}\quad`.
  Several constraints go on separate lines or get numbers.
- **Expectation operator** `E_t` (information set at t); **discount factor** `\beta`; **sum**
  `\sum_{t=0}^{\infty}`.
- **Time subscripts**: current `_t`, lead `_{t+1}`, lag `_{t-1}`. They correspond to Dynare's `(+1)` and
  `(-1)`, but the note uses mathematical subscripts. Do not write Dynare syntax in the note.
- **Common Greek letters**: `\beta \sigma \varphi \kappa \theta \phi \rho \alpha \delta \lambda`. The note
  can use `\alpha` and `\beta`; only the `.mod` renames them (`alppha`, `betta`) to avoid name clashes
  (R5). The section 8 table records each mapping.
- **Steady-state values**: a superscript star or no time subscript, `C^*` or `\bar{C}`. Use one notation
  in the whole note.
- **Log deviations** (linearized models): a lowercase hat, `\hat{x}_t`, with its definition stated in
  section 6: `\hat{x}_t=\log(X_t/\bar X)`.
- Fractions `\frac{}{}`, exponents `^{}`, brackets around products inside an expectation
  `E_t\big[\cdot\big]`; balance long expressions with `\big( \big)`.

## FOC numbering and economic meaning

- Give each equilibrium condition a unique number (F1), (F2), ..., **continuous across sections 3 to 5**
  (market clearing and exogenous processes count too).
- Put the number right next to the formula, for example:
  ````markdown
  - **(F1) Euler equation** (intertemporal consumption choice):
  ```math
  C_t^{-\sigma} = \beta\,E_t\Big[C_{t+1}^{-\sigma}\big(R_{t+1}\big)\Big]
  ```
  ````
- One phrase of economic meaning is enough; do not write long verbal derivations. For an intermediate
  step, write "the FOC with respect to $`C_t`$ gives".

## Section 8 table format

Use three lists or one table. For each endogenous variable, give the equation that determines it, so
that Stage 3 can check R4:

```markdown
| Class | Name (`.mod`) | Math | Meaning | Determined by |
|---|---|---|---|---|
| endogenous (`var`) | `c` | $`C_t`$ | Consumption | (F1) |
| endogenous (`var`) | `n` | $`N_t`$ | Hours worked | (F2) |
| exogenous (`varexo`) | `eps_a` | $`\varepsilon^a_t`$ | TFP innovation | — |
| parameter | `betta`, `sigma`, ... | $`\beta, \sigma`$ | Discount factor, risk aversion, ... | — |
```

- The number of endogenous variables = the number of determining equations among (F1) ... (Fn). This is
  the R4 pre-check. The R4 exceptions apply: with `ramsey_model` or `discretionary_policy`, a policy
  instrument has no determining equation in the note (write "policy instrument" in its row);
  heterogeneous models count each heterogeneity dimension separately (heterogeneity.md).
- The Name column uses the ASCII names that the `.mod` declares (`betta`, not `beta`; R5), so Stage 2
  can copy them. The Meaning column follows the user's language.

## Quality self-check (before you deliver the note)

1. All eight sections are present; the headings match the template (translated when the prose is not
   English).
2. Every FOC has a number; the LaTeX is correct (balanced brackets, correct subscripts).
3. The number of endogenous variables = the number of determining FOCs (the section 8 table matches,
   R4 exceptions marked).
4. The form (nonlinear or linearized) is stated and follows R8.
5. The timing convention (stock at the end or at the beginning of the period) is stated (R2).
6. **The steady-state solution is complete**: the steady-state system and the closed-form or
   reverse-solve steps in top-to-bottom order, ready to copy into steady_state_model. A linearized
   model states that the steady state is zero.
7. All agents are covered; no constraint or market-clearing condition is missing.
8. The Walras-law redundancy is found and marked, and each CES aggregator has its own definition
   equation (workflow-detail.md, Stage 1 (derivation note)).
