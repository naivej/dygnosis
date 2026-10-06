# Modeling blocks: optimization problems and first-order conditions by agent

Read this when you write Stage 1 (derivation note), before you write `<model>_derivation.md`.

This file gives the content of the derivation: for households, firms, government and central bank, and
market clearing, it gives the standard optimization problem, how to derive the first-order conditions
(FOCs), and the common variants that change the equation structure. `references/derivation-style.md`
gives the format of the derivation note (eight sections, LaTeX, numbering). Use the two files together:
this file supplies the substance of sections 2–5, derivation-style.md the layout. Formulas here use
`$…$`; in the derivation note use the delimiters that derivation-style.md prescribes.

**How to use it.** After you route the task (SKILL.md "Route the task"), build the model one block at a
time: households, then firms, then government and central bank, then market clearing. Write each block's
optimization problem and FOCs into the matching section of the derivation note as soon as you settle it,
then start the next block. Do not derive all four blocks before you write anything.

**Conventions for the whole file** (SKILL.md "Writing rules"):

- **R2 Timing.** In Dynare the timing of a variable reflects when it is decided. A stock decided in
  period $t$ is $X_t$ and is used from $t+1$. Capital $K_t$ is the stock at the end of the period, so
  production in $t$ uses $K_{t-1}$, and the law of motion has $K_t$ on the left. In the `.mod`:
  `y = k(-1)^alppha*…;` and `k = invest + (1-delta)*k(-1);`. The alternative is
  `predetermined_variables k;` with `y = k^alppha*…;` and `k(+1) = invest + (1-delta)*k;`. Use one
  convention per file. Check the written timing with `dynare_model_info` (timing classes) and the
  per-equation identifiers in `dynare_equations` (`references/dygnosis-workflow.md`).
- **R8 Nonlinear by default.** Write the original nonlinear FOCs and let Dynare approximate them. Do not
  linearize by hand. Each block below gives the nonlinear form; linearized forms (such as the NKPC in gap
  form) appear only where marked "Linearized result".
- **Agents that do not optimize get no FOC.** Exogenous government spending, a mechanical central-bank
  rule and pure budget identities (such as $T_t=G_t$) have no optimization problem. Put their relations in
  market clearing and identities, or in exogenous processes (see section C).

---

## A. Households: dynamic optimization problem and FOCs

### A.1 Standard setup (separable CRRA utility, bonds and capital, closed-economy representative household)

The representative household chooses consumption $C_t$, labor $N_t$, bonds $B_t$, investment $I_t$ and
capital $K_t$ to maximize expected discounted utility:

$$\max_{\{C_t,\,N_t,\,B_t,\,I_t,\,K_t\}}\;
E_0\sum_{t=0}^{\infty}\beta^t\left[\frac{C_t^{1-\sigma}-1}{1-\sigma}\;-\;\psi\frac{N_t^{1+\varphi}}{1+\varphi}\right]$$

Constraint 1, the **budget constraint** (real terms; $R_t$ is the gross return on bonds, $R_t^k$ the
rental rate of capital, $\Pi_t$ firm profits paid out, $T_t$ lump-sum taxes):

$$C_t + I_t + \frac{B_t}{R_t} \;=\; W_t N_t + R_t^k K_{t-1} + B_{t-1} + \Pi_t - T_t$$

Constraint 2, **capital accumulation** (end-of-period stock $K_t$ on the left, R2):

$$K_t = (1-\delta)K_{t-1} + I_t$$

> **Timing.** The household rents out $K_{t-1}$, decided last period, so rental income is
> $R_t^k K_{t-1}$. $K_t$, decided now, produces from next period. Bonds $B_t$ are also held at the end
> of the period: the household pays $1/R_t$ per unit now and receives $B_t$ next period (the payoff is
> written as $B_{t-1}$ in the next period's budget).

### A.2 First-order conditions (Lagrange multiplier $\lambda_t$ on the budget constraint)

Differentiate with respect to $C_t,N_t,B_t,K_t$ (investment $I_t$ enters the $K_t$ condition through
the law of motion) and eliminate the multiplier with $\lambda_t=C_t^{-\sigma}$:

- **(HH-1) Marginal utility**: $\;\lambda_t = C_t^{-\sigma}\;$ (marginal utility of consumption = budget
  multiplier; keep it as its own equation or substitute it).

- **(HH-2) Labor supply** (marginal rate of substitution between consumption and leisure = real wage):
$$\psi\,N_t^{\varphi}\,C_t^{\sigma} = W_t$$

- **(HH-3) Bond Euler equation** (intertemporal consumption choice):
$$C_t^{-\sigma} = \beta\,R_t\,E_t\big[C_{t+1}^{-\sigma}\big]$$

- **(HH-4) Capital Euler equation** (intertemporal return on investment; combines $\partial K_t$ and
  $\partial I_t$; without adjustment costs Tobin's $q_t=\lambda_t$):
$$C_t^{-\sigma} = \beta\,E_t\Big[C_{t+1}^{-\sigma}\big(R_{t+1}^k + 1-\delta\big)\Big]$$

> (HH-3) and (HH-4) together imply no arbitrage: under certainty equivalence
> $R_t = E_t[R_{t+1}^k+1-\delta]$. If you keep both, do not also write the no-arbitrage equation, or one
> equation is redundant (R4).

In the derivation note, number each FOC (F1), (F2), … (derivation-style.md) and point to it from the
`.mod` with the equation's `name` tag (R1), for example:

```dynare
[name='Bond Euler equation (F3)']
c^(-sigma) = betta*r*c(+1)^(-sigma);
```

### A.3 Common variants (they change the equation structure)

| Variant | How to change the setup | Effect |
|---------|-------------------------|--------|
| **No capital** (endowment economy / basic NK) | Remove $I_t,K_t,R_t^k$ and (HH-4); drop investment and rental income from the budget | Only (HH-2) and (HH-3) remain; much smaller model |
| **GHH preferences** (no wealth effect on labor) | $U=\dfrac{\big(C_t-\psi\frac{N_t^{1+\varphi}}{1+\varphi}\big)^{1-\sigma}-1}{1-\sigma}$ | Labor supply becomes $\psi N_t^{\varphi}=W_t$ (independent of $C_t$); common in open-economy and RBC fitting |
| **Habit formation** | Utility contains $C_t-h\,C_{t-1}$ | The Euler equation has $C_{t-1}$ and $C_{t+1}$; one more state; more persistence |
| **Investment adjustment costs** | $K_t=(1-\delta)K_{t-1}+\big[1-S(I_t/I_{t-1})\big]I_t$ | Tobin's $q_t$ becomes its own variable with its own equation; investment responds more smoothly |
| **Sticky wages** (EHL / Calvo wages) | Households supply differentiated labor and set wages | Adds a wage Phillips curve and wage dispersion; same Calvo logic as section B, applied to wages |
| **Two types or heterogeneous households** (savers + hand-to-mouth, or HANK) | One budget and FOC set per type, aggregated with population weights | Twice the agents; HANK: `references/heterogeneity.md` |
| **Open economy** | Budget includes foreign bonds and net exports; add UIP (uncovered interest parity) | Adds a UIP equation and a foreign-bond state |

> Do not choose among these alone. If the user or the source has not settled them, ask before you write
> the household section: capital or not, preference form, one type or heterogeneous, closed or open. Ask
> them as one multiple-choice question with a recommended default (SKILL.md "New model", step
> "Decisions"). If the source settles them, state the choice and continue.

---

## B. Firms: profit maximization and FOCs

**First fork:** one layer of competitive firms (RBC type) or two layers with monopolistic competition (NK
type). Ask if the user or the source has not settled it.
RBC: perfect competition, price = marginal cost, no pricing power, so only factor-demand FOCs.
NK: final-goods firms (competitive, CES aggregation) plus intermediate firms (monopolistic, sticky
prices), so marginal cost and pricing equations are added.

### B.1 RBC: one layer of perfectly competitive firms

Each period the firm rents capital $K_{t-1}$, hires labor $N_t$, produces with constant returns to scale
(CRS) $Y_t=A_tK_{t-1}^{\alpha}N_t^{1-\alpha}$, and maximizes profit period by period (a static problem):

$$\max_{\{K_{t-1},\,N_t\}}\; A_t K_{t-1}^{\alpha}N_t^{1-\alpha} - R_t^k K_{t-1} - W_t N_t$$

**FOCs** (marginal product = factor price):

- **(Firm-1) Labor demand**: $\;W_t = (1-\alpha)\dfrac{Y_t}{N_t} = (1-\alpha)A_t K_{t-1}^{\alpha}N_t^{-\alpha}$
- **(Firm-2) Capital demand**: $\;R_t^k = \alpha\dfrac{Y_t}{K_{t-1}} = \alpha A_t K_{t-1}^{\alpha-1}N_t^{1-\alpha}$

Under CRS profits are zero ($\Pi_t=0$), so $\Pi_t$ can be dropped from the household budget. The
production function $Y_t=A_tK_{t-1}^{\alpha}N_t^{1-\alpha}$ is an identity in section D.

### B.2 NK: two layers of firms (monopolistic competition + Calvo sticky prices)

**Layer 1, final-goods firms** (perfect competition) aggregate intermediate goods $Y_t(j)$ with a CES
aggregator and minimize the cost of inputs. This gives:

- **Intermediate-goods demand**: $\;Y_t(j)=\Big(\dfrac{P_t(j)}{P_t}\Big)^{-\epsilon}Y_t$
- **Price index**: $\;P_t=\Big(\displaystyle\int_0^1 P_t(j)^{1-\epsilon}\,dj\Big)^{\frac{1}{1-\epsilon}}$

Here $\epsilon>1$ is the elasticity of substitution and $\frac{\epsilon}{\epsilon-1}$ the markup.

**Layer 2, intermediate firm $j$** (monopolistic competition) produces with $Y_t(j)=A_t N_t(j)$ (or with
capital, $A_tK_{t-1}(j)^{\alpha}N_t(j)^{1-\alpha}$).

Cost minimization gives **real marginal cost $MC_t$** (the same for all $j$):
- Labor only: $\;MC_t = \dfrac{W_t}{A_t}$
- With capital: $\;MC_t = \dfrac{1}{A_t}\Big(\dfrac{R_t^k}{\alpha}\Big)^{\alpha}\Big(\dfrac{W_t}{1-\alpha}\Big)^{1-\alpha}$, factor ratio $\dfrac{R_t^k}{W_t}=\dfrac{\alpha}{1-\alpha}\dfrac{N_t(j)}{K_{t-1}(j)}$

**Calvo pricing.** Each period only a fraction $1-\theta$ of firms can reset their price. A resetting firm
chooses $P_t^{*}$ to maximize expected discounted real profits subject to demand. The FOC gives the
**optimal reset price** (with $\Pi_t\equiv P_t/P_{t-1}$ gross inflation):

$$\frac{P_t^{*}}{P_t}=\frac{\epsilon}{\epsilon-1}\frac{x_{1,t}}{x_{2,t}},\qquad
\begin{cases}
x_{1,t}=C_t^{-\sigma}\,MC_t\,Y_t + \theta\beta\,E_t\big[\Pi_{t+1}^{\epsilon}\,x_{1,t+1}\big]\\[2pt]
x_{2,t}=C_t^{-\sigma}\,Y_t + \theta\beta\,E_t\big[\Pi_{t+1}^{\epsilon-1}\,x_{2,t+1}\big]
\end{cases}$$

($x_{1,t}$ and $x_{2,t}$ are two recursive endogenous variables that write the infinite sums as
recursions; this is the standard nonlinear Calvo form.)

**Price index law of motion** (from the Calvo draw):
$$1=\theta\,\Pi_t^{\epsilon-1}+(1-\theta)\Big(\frac{P_t^{*}}{P_t}\Big)^{1-\epsilon}$$

> **FOC numbering.** $MC_t$, $x_{1,t}$, $x_{2,t}$, $P_t^{*}/P_t$ and the price index law of motion each
> count as one equilibrium condition; continue the (F·) numbering in the derivation note. **Price
> dispersion** $\Delta_t$ goes into section D (market clearing), because it affects aggregate output.
>
> **Linearized result (for reference; do not write it into a nonlinear `.mod`).** Log-linearizing the
> Calvo block around the zero-inflation steady state gives the **New Keynesian Phillips curve (NKPC)**
> $\;\pi_t=\beta E_t\pi_{t+1}+\kappa\,\widehat{mc}_t$, with
> $\kappa=\frac{(1-\theta)(1-\theta\beta)}{\theta}$. Write this equation directly, with `model(linear)`,
> only when the replicated paper gives only the linear system or the user asks for the linear version (R8).

### B.3 Other common firm-side variants

| Variant | How to change the setup |
|---------|-------------------------|
| **Rotemberg quadratic price adjustment costs** (instead of Calvo) | One representative intermediate firm, no price dispersion $\Delta_t$; the pricing equation is a nonlinear difference equation with $\Pi_t(\Pi_t-1)$. Cleaner for nonlinear second-order welfare analysis |
| **Sticky wages** | Apply the Calvo logic to the labor market: a wage NKPC and wage dispersion |
| **Variable capital utilization** | Add utilization $u_t$ and a cost function; $R_t^k$ becomes a function of utilization |
| **Indexation** (prices or wages partly indexed to last period's inflation) | $\Pi_{t-1}$ appears in the Calvo recursions and the price index law of motion; more persistence |
| **Fixed costs / entry and exit** | Change the profit expression and add a zero-profit condition |

> One or two layers of firms, Calvo or Rotemberg, with or without capital: these are structural forks
> of the same kind as A.3. Ask if they are not settled; otherwise state the choice and continue.

---

## C. Government and central bank: policy functions (usually not an optimization problem)

**Key distinction.** In a standard model without optimal policy, the government and the central bank do
not solve an optimization problem. They follow **mechanical policy rules**. So this block gives **policy
functions, budget identities and exogenous processes**, with no FOC. In the derivation note they go in
section 4 (market clearing and identities) or section 5 (exogenous processes), not in sections 2 or 3.

**Exception: optimal policy.** With optimal policy under commitment (Ramsey), optimal policy under
discretion or optimal simple rules (OSR), the central bank does optimize. Read
`references/optimal-policy.md`: policy follows from maximizing welfare. With `ramsey_model` or
`discretionary_policy` the model block holds only the private-sector equilibrium conditions: one
equation fewer than endogenous variables per policy instrument (R4).

### C.1 Fiscal policy (government budget + spending and tax rules)

**Government budget constraint** (mirrors the household budget, real terms):

$$G_t + B_{t-1} = T_t + \frac{B_t}{R_t}$$

Closure. Default: Ricardian lump-sum taxes. State it and continue, unless the task is about fiscal
policy or debt and the user has not chosen; then ask.

- **Ricardian / lump-sum taxes** (simplest, the default): $B_t=0$ every period and taxes adjust so that
  $T_t=G_t$. Government bonds leave the model, and $B$ drops out of the household budget.
- **Fiscal rule with debt**: taxes respond to debt to keep it sustainable, for example
  $\;T_t-T = \gamma_b\,(B_{t-1}-B)\;$ ($\gamma_b$ large enough for stable debt); $B_t$ becomes an
  endogenous state.

**Government spending** is usually an exogenous AR(1) process (section 5, exogenous processes):
$$\log G_t = (1-\rho_g)\log G + \rho_g\log G_{t-1} + \varepsilon_t^{g}$$

### C.2 Monetary policy (central-bank rule)

**Taylor rule (nonlinear level form with interest-rate smoothing).** $R_t^{n}$ is the **nominal** gross
interest rate:

$$\frac{R_t^{n}}{R^{n}}=\left(\frac{R_{t-1}^{n}}{R^{n}}\right)^{\rho_R}
\left[\left(\frac{\Pi_t}{\Pi}\right)^{\phi_{\pi}}\left(\frac{Y_t}{Y^{*}}\right)^{\phi_{y}}\right]^{1-\rho_R}
\exp(\varepsilon_t^{m})$$

Here $\Pi$ is the inflation target (steady-state gross inflation), $Y^{*}$ the output target (potential or
steady-state output), and $\varepsilon_t^{m}$ the monetary policy shock.

**Fisher equation** (links the nominal rate to the real rate in the household Euler equation):
$$R_t^{n} = R_t\,E_t\big[\Pi_{t+1}\big]\quad\text{(certainty equivalence)}$$
The $R_t$ in the household Euler equation is this real rate. NK models often write the Euler equation
with the nominal rate directly, $\;C_t^{-\sigma}=\beta R_t^{n} E_t\big[C_{t+1}^{-\sigma}/\Pi_{t+1}\big]$;
the two forms are equivalent.

> **Determinacy and the Taylor principle.** In the basic NK model a determinate equilibrium needs roughly
> $\phi_{\pi}>1$. If `check` reports that the Blanchard-Kahn conditions fail, look here first. Only
> `check` decides; do not infer determinacy from the parameter values.
>
> **Zero lower bound on the nominal interest rate (ZLB).** Under perturbation (`stoch_simul`, `estimation`)
> do not write the ZLB with `max`, `min`, `abs`, `sign` or comparison operators: derivatives at the kink
> are wrong (R6; Dygnosis W200). Use OccBin (`references/occbin.md`), or perfect foresight with the
> `lmmcp` option of `perfect_foresight_solver` and a complementarity condition written after the
> equation with `⟂` (ASCII `_|_`), as in the manual: `r = … + e ⟂ r > -1.94478;`
> (`references/perfect-foresight.md`). The older `[mcp='…']` equation tag is obsolete (Dygnosis W170).
> In `model(linear)` Dygnosis reports nonsmooth operators as errors (E210, E211).

**Common alternative rules.** Use the Taylor rule with smoothing as the default and say so. Ask when
the user has not chosen and the rule matters for the experiment.

| Rule | Key points |
|------|------------|
| Simple Taylor rule (no smoothing) | Drop the $\rho_R$ term |
| Price-level targeting | Respond to the price-level gap instead of inflation |
| Money growth rule | $\Delta\log M_t$ exogenous or with feedback, plus a money demand equation (real balances in utility, or cash in advance) |
| Inflation or exchange-rate peg | Common in open economies; combine with UIP |

### C.3 Government and central bank: where the equations go

- Taylor rule, Fisher equation, government budget, fiscal rule: **identities or rules**. Continue the
  numbering in the derivation note, in section 4.
- $G_t$ and other exogenous policy disturbances: section 5 (exogenous processes).
- Each of these equations **determines one endogenous variable** (the Taylor rule determines $R_t^n$, the
  Fisher equation $R_t$, the government budget $T_t$ or $B_t$). They count in the R4 check (equations =
  endogenous variables). No optimization does not mean no equation.

---

## D. Market clearing and aggregate identities

Aggregate the blocks and set supply equal to demand to close the model. These are not FOCs (no
optimization), but they are numbered and they count in R4.

### D.1 Market-clearing conditions

- **Goods market (resource constraint)**, closed economy:
$$Y_t = C_t + I_t + G_t$$
(Without capital, $I_t=0$; in an open economy add net exports $NX_t$.)

- **Labor market**: $\;N_t=\displaystyle\int_0^1 N_t(j)\,dj$ (with one RBC firm this reduces to labor
  demand = labor supply, which the factor-demand equations usually already imply; no separate equation).

- **Capital market**: $\;K_{t-1}=\displaystyle\int_0^1 K_{t-1}(j)\,dj$ (as above; usually implied with
  one firm).

- **Bond / asset market**: closed economy with a representative household, $\;B_t=0$; with government
  debt, household holdings = government issuance.

### D.2 Production function and (NK) price dispersion

- The production function is an identity in section D: $\;Y_t=A_tK_{t-1}^{\alpha}N_t^{1-\alpha}$ (RBC).
- **NK price dispersion $\Delta_t$**: with sticky prices, firms produce different quantities, which drives
  a wedge between aggregate inputs and aggregate output:
$$Y_t\,\Delta_t = A_t K_{t-1}^{\alpha}N_t^{1-\alpha},\qquad
\Delta_t=(1-\theta)\Big(\frac{P_t^{*}}{P_t}\Big)^{-\epsilon}+\theta\,\Pi_t^{\epsilon}\,\Delta_{t-1}$$
$\Delta_t\ge 1$, and $\Delta=1$ in a zero-inflation steady state. $\Delta_t$ is a state variable; each of
these is one equation.

### D.3 Walras' law (R4: drop one equation)

All market-clearing conditions and all budget constraints together are **linearly dependent**: the
clearing of any one market follows from the other markets and the budget constraints. So **drop one
redundant market-clearing equation** (usually the bond/asset market or one factor market).

- If you keep the redundant equation, there are more equations than endogenous variables and Dynare
  refuses the file. Dygnosis reports the mismatch (E188, W013; `count_gap` in `dynare_equations`).
- If you balance the count by adding a variable while you keep a redundant equation, the system is
  singular. Only the run shows it (`steady` or `check` fails); static checks cannot see it.

> **In practice.** Keep the resource constraint $Y_t=C_t+I_t+G_t$ and drop bond-market clearing ($B_t=0$
> already follows from the household and government budgets). After you write the model block, count
> equations against endogenous variables (R4). If there is one equation too many, first look for Walras
> redundancy.

### D.4 Exogenous processes (section 5 of the derivation note; plan them with the blocks)

For stochastic commands (`stoch_simul`, `estimation`, …) each structural shock has one AR(1) process,
and **`varexo` holds only the innovations** (R3):
$$\log A_t=\rho_a\log A_{t-1}+\varepsilon_t^{a},\quad
\log G_t=(1-\rho_g)\log G+\rho_g\log G_{t-1}+\varepsilon_t^{g},\quad
v_t=\rho_v v_{t-1}+\varepsilon_t^{m},\;\dots$$
The persistent processes ($A_t,G_t,v_t$) are **endogenous variables**; only the $\varepsilon_t^{\cdot}$ go
in `varexo`. In the `.mod` (with `a`, `g`, `v` declared in `var` and `eps_a`, `eps_g`, `eps_m` in
`varexo`):

```dynare
[name='TFP process']
log(a) = rhoa*log(a(-1)) + eps_a;
[name='Government spending process']
log(g) = (1-rhog)*log(g_ss) + rhog*log(g(-1)) + eps_g;
[name='Monetary policy shock process']
v = rhov*v(-1) + eps_m;
```

In a perfect foresight experiment an exogenous variable may carry the path itself (`shocks` with
`periods`/`values`, or `endval`; `references/perfect-foresight.md`). For `varexo(heterogeneity=…)`
follow `references/heterogeneity.md`.

---

## E. Assembly: combine the four blocks into a complete model (check each item)

1. **Write each block into the derivation note as you go**: households, firms, government and central
   bank, market clearing. Each goes into its section (section 2 optimization problems, section 3 FOCs,
   section 4 market clearing and identities, section 5 exogenous processes). Do not hold them back.
2. **R4 pre-check.** In section 8 of the derivation note, list the equation that determines each
   endogenous variable; the number of rows must equal the number of equations. With `ramsey_model` or
   `discretionary_policy`, the model block has one equation fewer per policy instrument. Common causes of
   one equation too many: (1) the Walras-redundant market-clearing equation was not dropped; (2) the bond
   Euler equation, the capital Euler equation **and** the no-arbitrage equation were all written (keep
   two of the three).
3. **Fork list.** Before you build the model, collect the forks that the user or the source has not
   settled and ask once, each with a recommended default: capital or not / household preferences (CRRA,
   GHH, habit) / one household type or heterogeneous / closed or open economy / one competitive layer or
   two monopolistic layers of firms / Calvo or Rotemberg / sticky wages or not / Ricardian fiscal policy
   or a debt rule / monetary rule type (Taylor, price level, money growth). For settled forks, state the
   choice and continue.
4. **Steady state.** Write the steady-state version of the four blocks (time subscripts removed) in
   section 6 of the derivation note, in an order that can be evaluated top to bottom, then copy it into
   `steady_state_model` (`references/steady-state.md`).
5. **Form and names.** Nonlinear by default (R8). ASCII names in the `.mod`: `betta`, `alppha`,
   `invest`, … (R5). Timing as in R2. Equation tags, TeX names and `long_name` as in R1.
6. **Static check, then run.** After you write the `.mod`, run `dynare_diagnose` and read `count_gap` in
   `dynare_equations` (`references/dygnosis-workflow.md`). Steady state, Blanchard-Kahn conditions and
   determinacy come only from running Dynare (`resid`, `steady`, `check`); never infer them from static
   checks.
