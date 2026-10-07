# ZLB-QE model: derivation (optimization problems + first-order conditions)

Read this when you study, reuse or extend the archived ZLB-QE model (`zlb_qe.mod`): a compact nonlinear New Keynesian model with a portfolio-balance term premium and QE, with the ZLB solved under perfect foresight with `lmmcp`.

> This derivation is the basis for the Dynare `.mod`. **Confirm it before moving to the coding stage.**
> Topic: use the bond-market segmentation / portfolio-balance channel (CCF12/PV16 style) to analyze **how the zero lower bound (ZLB) affects the effectiveness of quantitative easing (QE)**.

---

## 1. Model overview

- **Model**: compact New Keynesian model + portfolio-balance term premium + QE.
  The template is the bond-market segmentation / portfolio-balance mechanism of Chen-Curdia-Ferrero (2012, *Economic Journal*, "The Macroeconomic Effects of Large-Scale Asset Purchase Programmes") (local library `US_CCF12_rep`), **distilled into a compact nonlinear version with a closed-form steady state that runs under perfect foresight with lmmcp**. It strips habit formation, wage rigidity, capacity utilization, two types of price setters and other side features, and keeps only the skeleton of QE transmission.
- **QE transmission channel (core)**: because of market segmentation, short bonds and long-duration assets (long bonds + capital) are **imperfect substitutes**, and a term premium $\zeta_t$ separates them. Central-bank QE = buying long-term assets and reducing private long-bond holdings → $\zeta_t$ falls → the required return on long-duration assets falls → the price of capital $Q^k_t$ rises → investment recovers. **This is the CCF12-style portfolio-balance channel** (their D.23: $\zeta_t = \zeta'(\text{private long-bond holdings}) + \text{shock}$).
- **Experiment**: perfect foresight (deterministic). A **large negative capital quality shock** pushes the natural rate of interest below the ZLB. Compare the economy's response across three scenarios:
  - **Scenario (1)** no ZLB (the short rate can go negative; conventional rate policy is unconstrained), no QE — benchmark;
  - **Scenario (2)** ZLB binds (lmmcp enforces $R_t\ge 1$), no QE — conventional policy fails, and the recession deepens;
  - **Scenario (3)** ZLB binds + QE active — QE fills the policy gap through $\zeta_t$.
  Switch between the three scenarios with the macro processor `@#define`, and overlay the IRFs. **Direct answer: "The ZLB disables conventional policy, and that is exactly when QE becomes useful."**
- **Agents**: representative household (consumption/labor/holds short bonds and long-duration assets), monopolistically competitive firms (Rotemberg price rigidity), capital producers (investment adjustment costs), central bank (short-rate Taylor rule + ZLB + QE rule).
- **Form**: **nonlinear** (R8 default). The ZLB, as an inequality constraint, is inherently nonlinear, and perfect foresight + lmmcp needs exactly the original nonlinear equations. The portfolio-balance term premium is written as a structural relation, not as a reduced linear form.

---

## 2. Optimization problems of each agent

### 2.1 Household

The representative household holds **short-term nominal bonds** $B_t$ (one period, gross nominal rate $R_t$) and **long-duration assets** (long bonds + capital, real returns $R^L_{t+1}, R^k_{t+1}$). Market segmentation / portfolio adjustment frictions make long and short assets imperfect substitutes. This shows up as a term-premium wedge $\zeta_t$ in the Euler equation of the long-duration assets (see the note in Section 3). Utility:

$$\max_{\{C_t,N_t,B_t,\dots\}}\; E_0\sum_{t=0}^{\infty}\beta^t\Big[\frac{C_t^{1-\sigma}}{1-\sigma}-\chi\frac{N_t^{1+\varphi}}{1+\varphi}\Big]$$

Budget constraint (real; with short bonds, long-duration assets, wages, profits, capital rent):

$$C_t + \frac{B_t}{P_t} + (\text{long-duration asset purchases}) \le \frac{R_{t-1}B_{t-1}}{P_t} + (\text{long-duration asset returns}) + W_tN_t + \Pi^f_t$$

Let $\Lambda_t \equiv C_t^{-\sigma}$ be the marginal utility of consumption, and let the stochastic discount factor be $\Lambda_{t,t+1}\equiv\beta\,\Lambda_{t+1}/\Lambda_t = \beta\,C_{t+1}^{-\sigma}/C_t^{-\sigma}$.

### 2.2 Firms (monopolistic competition + Rotemberg price rigidity)

Intermediate-goods firm $j$ produces with effective capital $\tilde K_t=\xi_t K_{t-1}$ and labor, faces demand $Y_t(j)=(P_t(j)/P_t)^{-\epsilon}Y_t$, and pays a quadratic cost $\frac{\phi_p}{2}\big(\frac{P_t(j)}{P_{t-1}(j)}-1\big)^2 Y_t$ to change its price. It maximizes the present value of real profits:

$$\max\; E_0\sum_{t=0}^{\infty}\Lambda_{0,t}\Big[\big(\tfrac{P_t(j)}{P_t}-mc_t\big)\big(\tfrac{P_t(j)}{P_t}\big)^{-\epsilon}Y_t-\frac{\phi_p}{2}\big(\tfrac{P_t(j)}{P_{t-1}(j)}-1\big)^2 Y_t\Big]$$

Cost minimization gives the factor demands ($mc_t$ = real marginal cost).

### 2.3 Capital producers

Capital producers turn investment $I_t$ into new capital and pay investment adjustment costs $\frac{\kappa_I}{2}\big(\frac{I_t}{I_{t-1}}-1\big)^2 I_t$; the price of capital (Tobin's Q) is $Q^k_t$.

---

## 3. First-order conditions (FOC)

> Treatment of the term-premium wedge $\zeta_t$: the short-bond Euler equation **does not contain** $\zeta_t$ (the short rate is the policy rate, constrained directly by the ZLB). The Euler equations of the long-duration assets (long bonds, capital) **contain** $(1+\zeta_t)$ — a positive $\zeta_t$ means that long assets must offer a higher return to be held (lower price, higher yield). The portfolio-balance relation (F11) determines $\zeta_t$, and QE works through it.

- **(F1) Consumption Euler equation (short rate; this is the equation the ZLB constrains)**:
$$C_t^{-\sigma}=\beta\,E_t\Big[C_{t+1}^{-\sigma}\,\frac{R_t}{\Pi_{t+1}}\Big]$$

- **(F2) Labor supply**:
$$\chi N_t^{\varphi}=C_t^{-\sigma}\,W_t$$

- **(F3) Production function** (effective capital $\xi_t K_{t-1}$):
$$Y_t=(\xi_t K_{t-1})^{\alpha}N_t^{1-\alpha}$$

- **(F4) Capital rent (capital demand)**:
$$r^k_t=\alpha\,mc_t\,\frac{Y_t}{\xi_t K_{t-1}}$$

- **(F5) Labor demand**:
$$W_t=(1-\alpha)\,mc_t\,\frac{Y_t}{N_t}$$

- **(F6) Rotemberg New Keynesian Phillips curve** (normalized with $\Pi_{ss}=1$):
$$\phi_p\,(\Pi_t-1)\Pi_t=(1-\epsilon)+\epsilon\,mc_t+\phi_p\,E_t\Big[\Lambda_{t,t+1}(\Pi_{t+1}-1)\Pi_{t+1}\frac{Y_{t+1}}{Y_t}\Big]$$

- **(F7) Capital accumulation** (with the capital quality shock $\xi_t$ and investment adjustment costs):
$$K_t=(1-\delta)\xi_t K_{t-1}+\Big[1-\frac{\kappa_I}{2}\big(\tfrac{I_t}{I_{t-1}}-1\big)^2\Big]I_t$$

- **(F8) Investment FOC (Tobin's Q)**:
$$1=Q^k_t\Big[1-\frac{\kappa_I}{2}\big(\tfrac{I_t}{I_{t-1}}-1\big)^2-\kappa_I\big(\tfrac{I_t}{I_{t-1}}-1\big)\tfrac{I_t}{I_{t-1}}\Big]+E_t\Big[\Lambda_{t,t+1}\,Q^k_{t+1}\,\kappa_I\big(\tfrac{I_{t+1}}{I_t}-1\big)\big(\tfrac{I_{t+1}}{I_t}\big)^2\Big]$$

- **(F9) Realized return on capital** ($t-1\to t$, with $Q^k_{t-1}$ in the denominator):
$$R^k_t=\frac{\xi_t\big[r^k_t+(1-\delta)Q^k_t\big]}{Q^k_{t-1}}$$

- **(F10) Capital Euler equation (with the term-premium wedge)**:
$$1+\zeta_t=\beta\,E_t\Big[\frac{C_{t+1}^{-\sigma}}{C_t^{-\sigma}}\,R^k_{t+1}\Big]$$

- **(F11) Portfolio balance / term premium** (entry point of QE):
$$\zeta_t=\bar\zeta-\zeta'\,qe_t$$
Central-bank long-bond holdings $qe_t$↑ → private long-bond holdings↓ → $\zeta_t$↓. $\zeta'>0$ is the portfolio-balance elasticity.

- **(F12) Long-term nominal rate (term-premium definition, for reporting)**:
$$R^L_t=R_t\,(1+\zeta_t)$$

> **Implementation note on the long rate**: the first design priced a perpetuity explicitly (geometrically decaying coupon $\kappa_L$, price $Q^L_t$). Under perfect foresight its forward-looking Euler equation + the $Q^L_{t-1}$ recursion produced a **singular Jacobian** (two spurious eigenvalues of order $\sim 10^{51}$; the solver failed). Because this long-bond block **does not feed back into the real economy** (the demand channel of QE is capital/investment (F10); the long bond is for reporting only), it was replaced with the **static identity** $R^L_t=R_t(1+\zeta_t)$: long-term nominal rate = short-term nominal rate compounded with the term premium. This removes the singularity and also highlights the QE story: at the ZLB, $R_t$ is locked at 1, so the long rate can fall only when QE compresses $\zeta_t$.

---

## 4. Market clearing and aggregate identities

- **(F15) Resource constraint** (Rotemberg price adjustment costs are a real resource loss; no government purchases; QE uses no resources):
$$Y_t=C_t+I_t+\frac{\phi_p}{2}(\Pi_t-1)^2 Y_t$$

> **Walras' law check**: the redundant equation of this model is the **household budget constraint**. Short bonds are in zero net supply (netted among households / with the central bank), households hold the capital, and firm profits are fully rebated to households. Adding the household budget constraint, the firm profit definition and the central-bank/QE account, and substituting each agent's FOCs, reduces exactly to the resource constraint (F15). So the **household budget constraint is redundant and does not go into the model block** — otherwise the equation count would be one too many and the Blanchard-Kahn conditions would necessarily fail. In this compact setup QE enters in the reduced form (F11), with no separate central-bank balance-sheet account (an explicit modeling trade-off for a closed-form steady state; the mechanism is equivalent to CCF12's D.20+D.23).

---

## 5. Exogenous processes and policy rules

- **(F16) Short-rate Taylor rule + ZLB** ($R_{ss}=1/\beta$, $\Pi_{ss}=1$):
$$R_t=R_{ss}\,\Pi_t^{\phi_\pi}\Big(\frac{Y_t}{Y_{ss}}\Big)^{\phi_y}\qquad\perp\qquad R_t\ge 1$$
Scenario (1): drop the complementarity condition and allow $R_t<1$. Scenarios (2) and (3): lmmcp enforces $R_t\ge1$.
(Dynare residual = LHS−RHS = $R_t-$rule; when the bound binds, $R_t=1$ and the rule value is < 1 → residual > 0, as the lmmcp lower bound requires.)

- **(F17) QE rule** (central-bank long-bond holdings; active in Scenario (3), zero in (1) and (2)):
$$qe_t=\rho_{qe}\,qe_{t-1}+\phi_{qe}\,\frac{Y_{ss}-Y_t}{Y_{ss}}\qquad(\text{Scenarios (1)(2)}:qe_t=0)$$
Recession ($Y_t<Y_{ss}$) → the central bank expands its balance sheet and buys long bonds.

- **(F18) Capital quality shock process** (drives the recession):
$$\log\xi_t=\rho_\xi\,\log\xi_{t-1}+\varepsilon^\xi_t$$
A large shock with $\varepsilon^\xi_t<0$ → effective capital and the return on capital collapse → investment collapses → the natural rate of interest turns negative → the ZLB binds.

---

## 6. Steady state ($\xi=1,\ qe=0,\ \Pi=1$; can be copied top-down into steady_state_model)

Evaluate in this order:

1. $\Pi=1,\quad R=1/\beta,\quad \zeta=\bar\zeta,\quad qe=0,\quad \xi=1$
2. Price-rigidity steady state (F6, $\Pi=1$): $mc=\dfrac{\epsilon-1}{\epsilon}$
3. Investment steady state (F8, $I/I_{-1}=1$): $Q^k=1$
4. Return on capital (F10): $R^k=\dfrac{1+\bar\zeta}{\beta}$
5. Rent (F9, $\xi=1,Q^k=1$): $r^k=R^k-(1-\delta)=\dfrac{1+\bar\zeta}{\beta}-(1-\delta)$
6. Capital-output ratio (F4): $\dfrac{K}{Y}=\dfrac{\alpha\,mc}{r^k}$
7. Investment-output ratio (F7, $\xi=1$): $I=\delta K\Rightarrow \dfrac{I}{Y}=\delta\dfrac{K}{Y}$
8. Normalize $N=1$ (back-solve $\chi$): from $Y=K^\alpha N^{1-\alpha}=K^\alpha$ and $K=(K/Y)Y=(K/Y)K^\alpha$,
$$K=\Big(\frac{K}{Y}\Big)^{1/(1-\alpha)},\quad Y=K^\alpha,\quad I=\delta K,\quad C=Y-I$$
9. Wage (F5): $W=(1-\alpha)mc\,Y/N=(1-\alpha)mc\,Y$
10. Back-solve the preference weight (F2): $\chi=C^{-\sigma}W/N^{\varphi}=C^{-\sigma}W$
11. Long-term rate (F12): $R^L=R\,(1+\bar\zeta)=\dfrac{1+\bar\zeta}{\beta}$
12. Check the short-rate Euler equation (F1): $1=\beta R/\Pi=\beta\cdot(1/\beta)/1=1$ (holds).

> $R_{ss}=1/\beta>1$, so the ZLB is slack in the steady state.

---

## 7. Timing and form conventions

- **State variables are stocks at the end of the period**: $K_t$ is capital at the end of period $t$; production uses $K_{t-1}$ (F3 writes `K(-1)`); $I_{t-1}$ enters the adjustment cost; $Q^k_{t-1}$ enters the realized return (F9); $qe_{t-1},\xi_{t-1}$ enter their own lag terms.
- **Capital quality shock**: $\xi_t$ multiplies $K_{t-1}$ in the same period to give effective capital (F3, F4, F7, F9).
- **Interest-rate timing**: $R_t$ is the gross nominal rate set in period $t$ and paid from $t$ to $t{+}1$; it enters (F1) as `R` (current period).
- **Form**: fully nonlinear; let Dynare do the Taylor expansion (R8). The ZLB uses `perfect_foresight_solver(lmmcp)` + `⟂ R>1` attached after the equation.
- **Solution**: perfect foresight, `perfect_foresight_setup`/`solver`; initial and terminal values = the no-shock steady state.

---

## 8. Variable and parameter table

| Class | Symbol (.mod) | Meaning | Determined by |
|------|-----------|------|--------------|
| Endogenous | `C` | consumption | (F1) |
| Endogenous | `N` | labor | (F2) |
| Endogenous | `Y` | output | (F3) |
| Endogenous | `mc` | real marginal cost | (F5) |
| Endogenous | `W` | real wage | (F4) via factor demand; jointly with (F5) |
| Endogenous | `rk` | capital rent | (F4) |
| Endogenous | `Pi` | gross inflation | (F6) |
| Endogenous | `K` | capital (end of period) | (F7) |
| Endogenous | `I` | investment | (F8) |
| Endogenous | `Qk` | price of capital, Tobin's Q | (F10) |
| Endogenous | `Rk` | realized return on capital | (F9) |
| Endogenous | `zeta` | term premium | (F11) |
| Endogenous | `RL` | long-term gross nominal rate | (F12) |
| Endogenous | `R` | short-term gross nominal rate | (F16) Taylor + ZLB |
| Endogenous | `qe` | central-bank long-bond holdings (QE) | (F17) |
| Endogenous | `xi` | capital quality | (F18) |
| Exogenous | `eps_xi` | capital quality innovation | — |
| Parameter | `betta,sigma,varphi,chi,alppha,delta` | preferences/technology | — |
| Parameter | `epsilon,phi_p` | markup/Rotemberg rigidity | — |
| Parameter | `kappa_I` | investment adjustment cost | — |
| Parameter | `zetabar,zeta_prime` | steady-state term premium/portfolio-balance elasticity | — |
| Parameter | `phi_pi,phi_y` | Taylor coefficients | — |
| Parameter | `rho_qe,phi_qe` | QE rule | — |
| Parameter | `rho_xi` | capital quality persistence | — |

**R4 check (final implementation)**: 16 endogenous variables (C,N,Y,mc,W,rk,Pi,K,I,Qk,Rk,zeta,RL,R,qe,xi), 16 equations (F1–F12, F15–F18; the long-bond block was simplified from the original three equations F12–F14 to one static identity F12, so the variable count and the equation count each drop by 2). The household budget constraint is removed by Walras' law. Runs cleanly: steady-state residuals all 0, Blanchard-Kahn rank condition verified, perfect foresight solution residuals $\sim10^{-10}$.

> **Macro processor switches**: `@#define ZLB` (0 = Scenario (1), 1 = Scenarios (2)(3)) controls whether (F16) carries `⟂`; `@#define QE` (0 = Scenarios (1)(2), 1 = Scenario (3)) controls whether (F17) is the QE rule or `qe=0`. Run once per scenario (three runs) and overlay the IRFs.

---

**Points to confirm (please review)**:
1. **Where the QE channel acts**: QE acts through the term premium $\zeta_t$ on **capital/investment** (the required return on long-duration assets), not through a separate long-rate Euler equation for household consumption. This is the most natural demand channel once CCF12's two household types are distilled into a representative household. Accept?
2. **No separate term-premium shock**: the capital quality shock alone drives the recession; $\zeta_t$ moves only with QE (without QE it stays at $\bar\zeta$). This matches your choice to "use only the capital quality shock". Accept?
3. The **QE rule** responds to the output gap ($\phi_{qe}\cdot(Y_{ss}-Y)/Y_{ss}$) and is active in Scenario (3). Or do you want a one-time QE of **fixed size** (closer to an "LSAP announcement")?
4. **Price rigidity uses Rotemberg** (a single NKPC, no helper variables needed) instead of CCF12's Calvo. Accept?

After you confirm (or name the points to change), I move to Stage 2 (declarations), write the `.mod`, and verify it incrementally with three locked runs.
