# BGG financial-friction comparison model — derivation (optimization problems + first-order conditions)

Read this when you build or check a BGG-type financial accelerator model, or a `@#define` switch between two model variants in one `.mod`.

> This derivation is the basis for the Dynare .mod. Move to the coding stage only after it is confirmed.  
> Source: Bernanke, Gertler & Gilchrist (1999), "The Financial Accelerator in a Quantitative Business Cycle Framework", *Handbook of Macroeconomics*  
> Comparison experiment: `@#define WITH_FA = 1` (with BGG financial frictions) vs `WITH_FA = 0` (frictionless standard NK)

---

## 1. Model overview

- **Model**: small closed-economy NK-BGG, following the Bernanke-Gertler-Gilchrist (1999) financial accelerator framework.
- **Experiment**: stochastic simulation (`stoch_simul, order=1`). One file; the `@#define WITH_FA` switch selects the case with or without financial frictions. Look at the IRFs of output, inflation, investment and the external finance premium to a monetary policy shock (MP shock) and a TFP shock, and compare monetary policy transmission in the two cases.
- **Agents**:
  - **Representative household**: CRRA utility; chooses consumption / labor / bonds; does not hold capital directly.
  - **Entrepreneurs**: buy capital with leverage and borrow from financial intermediaries. Agency costs (optimal CSV contract) create an **external finance premium**. Each period an entrepreneur "dies" with probability $(1-\gamma)$ and consumes its net worth.
  - **Capital goods producers**: face investment adjustment costs; their optimization gives Tobin's $Q$.
  - **Retailers / intermediate-goods firms**: Calvo sticky prices, CES differentiated goods.
  - **Central bank**: Taylor rule with interest-rate smoothing.
  - **Government**: exogenous AR(1) spending, Ricardian fiscal policy ($T_t = G_t$, zero net bond supply).
- **Form**: nonlinear (R8); Dynare does the first-order Taylor expansion. **Do not linearize by hand.**

---

## 2. Optimization problems of the agents

### 2.1 Representative household

$$\max_{\{C_t,\,N_t,\,B_t\}}\;E_0\sum_{t=0}^{\infty}\beta^t
\left[\frac{C_t^{1-\sigma}-1}{1-\sigma} - \psi\frac{N_t^{1+\varphi}}{1+\varphi}\right]$$

**Budget constraint** (real terms; $R^n_t$ is the gross nominal interest rate, $\Pi_t \equiv P_t/P_{t-1}$ is gross inflation, $W_t$ is the real wage, $T_t$ is a lump-sum tax):

$$C_t + \frac{B_t}{R^n_t / \Pi_{t+1}^e} = W_t N_t + B_{t-1} + \Pi_t^{div} - T_t$$

Simplified equivalent form (with the real interest rate $R_t \equiv R^n_t / E_t[\Pi_{t+1}]$; bonds clear at the end of the period, $B_t=0$ by Walras):

$$C_t = W_t N_t + \text{transfers}$$

The household **does not own capital**; entrepreneurs hold the capital.

### 2.2 Entrepreneurs

At the end of period $t$, each entrepreneur uses net worth $N_t$ as equity, borrows $Q_t K_t - N_t$ from the bank, and buys capital $K_t$ (price $Q_t$).

**Optimal CSV contract** (costly state verification): the bank pays the audit cost $\mu$ to verify the entrepreneur's idiosyncratic shock. The optimal contract creates an **external finance premium**:

$$E_t\big[R^k_{t+1}\big] = \Psi\!\left(\frac{Q_t K_t}{N_t}\right) \cdot R_t$$

where $\Psi(\cdot) > 1$ (higher leverage, higher premium), parameterized as $\Psi(x) = x^{\chi}$ ($\chi > 0$). This is a reduced form of the optimal CSV contract, simplified directly from the equilibrium conditions in BGG (1999) Appendix B.

**Net worth evolution** (a fraction $\gamma$ of entrepreneurs survive and keep their net worth; a fraction $(1-\gamma)$ "die" and consume):

Define the entrepreneurs' **total equity** in capital, $V_t$:

$$V_t = R^k_t \cdot Q_{t-1}K_{t-1} - R_{t-1}\cdot(Q_{t-1}K_{t-1} - N_{t-1})
= (R^k_t - R_{t-1})\cdot Q_{t-1}K_{t-1} + R_{t-1}\cdot N_{t-1}$$

$$N_t = \gamma \cdot V_t + \bar{W}^e, \qquad C^e_t = (1-\gamma)\cdot V_t$$

where $\bar{W}^e$ is a small labor-endowment income of entrepreneurs (keeps the participation constraint; a small quantity in steady state).

### 2.3 Capital goods producers

They buy final goods as investment $I_t$, turn them into new capital $I^{net}_t$ with an adjustment-cost technology, and sell it to entrepreneurs at price $Q_t$.

Maximize profit: $Q_t I_t^{net} - I_t$, where $I_t^{net} = I_t \cdot\big[1 - \tfrac{\phi_k}{2}(I_t/I_{t-1}-1)^2\big]$.

FOC with respect to $I_t$ (let $s \equiv I_t/I_{t-1}$, $S(s)=\tfrac{\phi_k}{2}(s-1)^2$, $S'(s)=\phi_k(s-1)$):

$$Q_t = \frac{1}{1 - S(s_t) - S'(s_t)\cdot s_t} + \beta\, E_t\!\left[\frac{u_{t+1}}{u_t}\cdot Q_{t+1}\cdot S'(s_{t+1})\cdot s_{t+1}^2\right]$$

Equivalent **simplified nonlinear Q equation** (kept to first order):

$$1 = Q_t\cdot\Big[1 - \tfrac{\phi_k}{2}(s_t-1)^2 - \phi_k(s_t-1)\cdot s_t\Big]
+ \beta\,E_t\!\left[\frac{C_{t+1}^{-\sigma}}{C_t^{-\sigma}}\cdot Q_{t+1}\cdot \phi_k(s_{t+1}-1)\cdot s_{t+1}^2\right]$$

(This is the Tobin's Q FOC that goes into the model block, in nonlinear form.)

### 2.4 Intermediate-goods firms (Calvo pricing)

**Cost minimization** (given $W_t$, $R^k_t$): the factor-ratio condition gives real marginal cost:

$$MC_t = \frac{1}{A_t}\left(\frac{R^k_t}{\alpha}\right)^{\alpha}\left(\frac{W_t}{1-\alpha}\right)^{1-\alpha}$$

That is, from the capital pricing condition: $R^k_t = \alpha \cdot MC_t \cdot Y_t / K_{t-1}$ (combined with labor market clearing, see §3 F3).

**Calvo optimal pricing** (probability $\theta$ of not being able to reset the price): maximize expected discounted profit. The FOC rearranges into two recursive helper variables:

$$x_{1,t} = C_t^{-\sigma}\cdot MC_t \cdot Y_t + \theta\beta\,E_t\!\left[\Pi_{t+1}^{\varepsilon}\,x_{1,t+1}\right]$$

$$x_{2,t} = C_t^{-\sigma}\cdot Y_t + \theta\beta\,E_t\!\left[\Pi_{t+1}^{\varepsilon-1}\,x_{2,t+1}\right]$$

**Optimal reset price**:

$$\frac{P^*_t}{P_t} = \frac{\varepsilon}{\varepsilon-1}\cdot\frac{x_{1,t}}{x_{2,t}}$$

---

## 3. First-order conditions (FOC)

> Consecutive numbering F1–F20 across sections 3–5. Each FOC maps to one equation in the .mod (the `[name=]` tag refers back to it).

**Household**

**(F1) Euler equation** (intertemporal consumption choice):

$$C_t^{-\sigma} = \beta\cdot\frac{R^n_t}{E_t[\Pi_{t+1}]}\cdot E_t\!\left[C_{t+1}^{-\sigma}\right]$$

Or equivalently (with $R_t = R^n_t / \Pi_{t+1}$): $C_t^{-\sigma} = \beta R_t \,E_t[C_{t+1}^{-\sigma}]$

**(F2) Labor market equilibrium** (household MRS = firm labor demand, merged into one equation):

$$\psi N_t^{\varphi} C_t^{\sigma} = MC_t \cdot \frac{(1-\alpha)Y_t}{N_t}$$

(The left side is the household's marginal cost of labor supply; the right side is the firm's real wage $W_t = MC_t \cdot (1-\alpha) Y_t / N_t$.)

**Capital and investment**

**(F3) Return on capital** (total gross return on capital held by entrepreneurs, set by the end-of-period price of capital and the rental):

$$R^k_t = \frac{MC_t\cdot\alpha\cdot Y_t / K_{t-1} + (1-\delta)\cdot Q_t}{Q_{t-1}}$$

**(F4) Capital arbitrage condition / external finance premium** (entrepreneurs' capital demand equation, forward-looking):

$$E_t\big[R^k_{t+1}\big] = \text{premium}_t \cdot R_t$$

In Dynare: `rk(+1) = premium * r;`

**(F5) Tobin's Q (capital goods producer FOC, simplified form)**:

Let $s_t \equiv I_t / I_{t-1}$,

$$1 = Q_t\Bigl[1 - \tfrac{\phi_k}{2}(s_t-1)^2 - \phi_k(s_t-1)s_t\Bigr]
+ \beta\,E_t\!\left[\frac{C_{t+1}^{-\sigma}}{C_t^{-\sigma}}\cdot Q_{t+1}\cdot\phi_k(s_{t+1}-1)\cdot s_{t+1}^2\right]$$

**(F6) Capital accumulation** (stock at the end of the period, R2):

$$K_t = (1-\delta)K_{t-1} + \Bigl[1 - \tfrac{\phi_k}{2}(s_t-1)^2\Bigr] I_t$$

**Financial friction block (switched by @#define WITH_FA)**

> The next two equations differ between the two cases:

**(F7) Net worth evolution**

| Case | Equation |
|------|------|
| **WITH_FA = 1** (financial frictions) | $N_t = \gamma\bigl[(R^k_t - R_{t-1})\,Q_{t-1}K_{t-1} + R_{t-1} N_{t-1}\bigr] + \bar{W}^e$ |
| **WITH_FA = 0** (frictionless) | $N_t = Q_t K_t$ (entrepreneurs are fully equity-financed; net worth = total value of capital)|

**(F8) External finance premium definition**

| Case | Equation |
|------|------|
| **WITH_FA = 1** | $\text{premium}_t = \left(\dfrac{Q_t K_t}{N_t}\right)^{\chi}$ (higher leverage, higher premium)|
| **WITH_FA = 0** | $\text{premium}_t = 1$ (no premium)|

**(F9) Entrepreneur consumption**:

$$C^e_t = (1-\gamma)\bigl[(R^k_t - R_{t-1})\,Q_{t-1}K_{t-1} + R_{t-1}N_{t-1}\bigr]$$

(Same in both cases. With WITH_FA = 0, premium = 1 gives $R^k = R$, so $C^e$ symmetrically reduces to a small quantity near 0; the equation form stays the same to keep R4.)

**Production and prices**

**(F10) Production function** (with Calvo price dispersion $\Delta_t$, which inflates total factor input):

$$Y_t \cdot \Delta_t = A_t \cdot K_{t-1}^{\alpha} \cdot N_t^{1-\alpha}$$

**(F11) Calvo recursion $x_{1}$**:

$$x_{1,t} = C_t^{-\sigma}\cdot MC_t \cdot Y_t + \theta\beta\,E_t\!\left[\Pi_{t+1}^{\varepsilon}\,x_{1,t+1}\right]$$

**(F12) Calvo recursion $x_{2}$**:

$$x_{2,t} = C_t^{-\sigma}\cdot Y_t + \theta\beta\,E_t\!\left[\Pi_{t+1}^{\varepsilon-1}\,x_{2,t+1}\right]$$

**(F13) Optimal reset price**:

$$\frac{P^*_t}{P_t} = \frac{\varepsilon}{\varepsilon-1}\cdot\frac{x_{1,t}}{x_{2,t}}$$

**(F14) Price index evolution** (Calvo draw rule):

$$1 = \theta\,\Pi_t^{\varepsilon-1} + (1-\theta)\left(\frac{P^*_t}{P_t}\right)^{1-\varepsilon}$$

**(F15) Price dispersion evolution**:

$$\Delta_t = (1-\theta)\left(\frac{P^*_t}{P_t}\right)^{-\varepsilon} + \theta\,\Pi_t^{\varepsilon}\,\Delta_{t-1}$$

**Monetary authority and government**

**(F16) Fisher equation**:

$$R^n_t = R_t \cdot \Pi_{t+1}$$

(Dynare form: `rn = r * pi(+1);`)

**(F17) Taylor rule** (nominal interest rate, with smoothing $\rho_R$):

$$\frac{R^n_t}{R^n} = \left(\frac{R^n_{t-1}}{R^n}\right)^{\rho_R}\!\cdot
\left[\left(\frac{\Pi_t}{\Pi}\right)^{\phi_\pi}\!\left(\frac{Y_t}{Y}\right)^{\phi_y}\right]^{1-\rho_R}\!\cdot e^{\varepsilon^m_t}$$

---

## 4. Market clearing and aggregate identities

**(F18) Resource constraint** (Walras' law: bond market clearing $B_t=0$ is implied by the household budget + government budget; it is **not written separately**, and the redundant equation is removed):

$$Y_t = C_t + I_t + G_t + C^e_t$$

> **Walras' law check**: household budget $C_t = W_t N_t - T_t$; government budget $G_t = T_t$; entrepreneur net worth allocation $C^e + \Delta NW = R^k \cdot QK - R\cdot (QK - NW_{-1}) + \bar{W}^e$; firm profit distribution ($MC \cdot Y - W \cdot N - R^k_{\text{rental}}\cdot K$) is zero under CRS (the Calvo adjustment cost is used in CY). Sum all budgets + goods market clearing → bond market $B_t=0$ holds automatically. **Redundant equation: bond/asset market clearing (removed; not in the model block).**

---

## 5. Exogenous processes

**(F19) TFP shock** (AR(1), steady state $\bar{A}=1$):

$$\log A_t = \rho_a \log A_{t-1} + \varepsilon^a_t$$

**(F20) Government spending** (AR(1), steady state $G = \bar{g}_y \cdot \bar{Y}$):

$$\log G_t = (1-\rho_g)\log\bar{G} + \rho_g \log G_{t-1} + \varepsilon^g_t$$

> The monetary policy shock $\varepsilon^m_t$ is already in F17. `varexo` holds only the three innovations: `eps_a, eps_g, eps_m` (R3).

---

## 6. Steady-state solution

In steady state $X_t = X_{t+1} = \bar{X}$, $\Pi = 1$ (zero inflation target), $\bar{A}=1$, $\Delta = 1$ (price dispersion is 1 at zero inflation).

**Solve step by step in this order** (can be substituted top-down into `steady_state_model`):

```
// exogenous steady state
A_ss   = 1
G_ss   = gy_share * Y_ss  (back-solved, see below)
eps_*  = 0

// interest rates and returns
R_ss   = 1/betta               // from household Euler F1: C^{-σ}=β R C^{-σ}
Pi_ss  = 1
Rn_ss  = R_ss * Pi_ss = R_ss  // Fisher F16

// financial friction steady state (WITH_FA=1)
premium_ss = s_ss              // calibration target; external finance premium about 1.005 (0.5% quarterly)
Rk_ss  = premium_ss * R_ss    // capital arbitrage F4

// capital-labor ratio (from production function + return on capital)
// F3 steady state: Rk = (mc*α*Y/K + (1-δ)*Q) / Q, Q=1 (adjustment cost = 0 in steady state)
// → Rk = mc*α*Y/K + (1-δ)
// define kl = K/N (capital-labor ratio); then:
mc_ss  = 1/X_ss = (ε-1)/ε     // zero-profit steady state (CES; X=ε/(ε-1) is the inverse of the markup)
Rk_rental_ss = Rk_ss - (1-delta)  // rental = Rk - (1-δ) (when Q=1)
kl_ss  = (alpha * mc_ss / Rk_rental_ss)^(1/(1-alpha))  // from Rk_rental = alpha*mc*(kl)^{α-1}

// wage (steady state), see labor market F2
W_ss   = (1-alpha) * mc_ss / (kl_ss)^(-alpha) = (1-alpha)*mc_ss*(kl_ss)^alpha

// normalize labor N=1/3 → back-solve ψ
N_ss   = 0.333
K_ss   = kl_ss * N_ss
Y_ss   = K_ss^alpha * N_ss^(1-alpha)    // Δ=1, A=1
invest_ss = delta * K_ss
Q_ss   = 1                              // adjustment cost = 0 in steady state

// entrepreneur net worth (WITH_FA=1)
// F7 steady state: N_ss = γ*[(Rk-R)*QK + R*N] + We → N*(1-γ*R) = γ*(Rk-R)*QK + We
// let We → 0; approximation: N_ss = γ*(Rk-R)/(1-γ*R) * K_ss
// then from F8 steady state: premium_ss = (Q*K/N)^chi → N_ss = K_ss / premium_ss^(1/chi)
NW_ss  = Q_ss * K_ss / premium_ss^(1/chi)   // FROM F8
We_ss  = NW_ss * (1 - gamma*Rk_ss) - gamma*(Rk_ss - R_ss)*Q_ss*K_ss
         // back-solve We so that F7 holds in steady state

// entrepreneur consumption
V_ss   = (Rk_ss - R_ss)*Q_ss*K_ss + R_ss*NW_ss  ... approximate simplification
Ce_ss  = (1-gamma)*V_ss

// household consumption: from resource constraint F18
gy_share = 0.2                          // calibration target G/Y
G_ss   = gy_share * Y_ss
C_ss   = Y_ss - invest_ss - G_ss - Ce_ss

// back-solve labor disutility parameter ψ (so that steady-state labor = N_ss)
psi_ss = W_ss / (N_ss^phi * C_ss^sigma) // back-solved from F2 in steady state

// Calvo steady state (zero inflation)
pstar_ss = 1, x1_ss = C_ss^{-σ}*mc_ss*Y_ss / (1 - θ*β), x2_ss = C_ss^{-σ}*Y_ss / (1 - θ*β)
delta_p_ss = 1

// nominal interest rate
Rn_ss  = R_ss (already computed)

// TFP and government spending steady-state levels
a_ss   = 1, g_ss = G_ss
```

> **Calibration targets**: $\bar{N}=1/3$ (back-solve $\psi$), $G/Y=0.2$ (back-solve $\bar{G}$), $\text{premium}=1.005$ (external finance premium 0.5% quarterly; back-solve $\bar{W}^e$).

**With WITH_FA = 0**: $\text{premium}_{ss}=1$, $N_t = Q_t K_t$ (no leverage), $R^k_{ss} = R_{ss}$; everything else is unchanged.

---

## 7. Timing and form conventions

- **Stock at the end of the period (R2)**: $K_t$ is capital at the end of period $t$; production in period $t$ uses $K_{t-1}$; the capital accumulation equation has $K_t$ on the left.
- **Net worth**: $N_t$ is net worth at the end of period $t$ (after the entrepreneur's capital decision in period $t$). The period-$t$ return $R^k_t$ uses $K_{t-1}$, so net worth evolution contains $N_{t-1},K_{t-1}$.
- **Price dispersion** $\Delta_t$: state variable (contains $\Delta_{t-1}$).
- **Calvo helper variables** $x_{1,t}, x_{2,t}$: current-period control variables, forward-looking.
- **Fisher equation** (F16): written as `rn = r * pi(+1)`; given the current nominal interest rate it determines the real interest rate; forward-looking.
- **Form**: fully nonlinear (R8); let Dynare do the first-order expansion.

---

## 8. Variable and parameter table (R4 pre-check)

### Endogenous variables (20)

| Variable name (ASCII) | Meaning | Determined by equation |
|---|---|---|
| `c` | household consumption $C_t$ | F1 (Euler) |
| `n_lab` | hours worked $N_t$ | F2 (labor market) |
| `pi` | gross inflation $\Pi_t$ | F14 (price index) |
| `r` | gross real interest rate $R_t$ | F16 (Fisher) |
| `rn` | gross nominal interest rate $R^n_t$ | F17 (Taylor rule) |
| `q` | Tobin's Q $Q_t$ | F5 (Q equation) |
| `k` | capital stock $K_t$ (end of period) | F6 (capital accumulation) |
| `nw` | entrepreneur net worth $N_t$ | F7 (net worth, @#define) |
| `rk` | return on capital $R^k_t$ | F3 (return definition) |
| `y` | total output $Y_t$ | F18 (resource constraint) |
| `mc` | real marginal cost $MC_t$ | F2 (labor market, jointly with n_lab) |
| `invest` | investment $I_t$ | F4 (capital arbitrage) |
| `a` | TFP $A_t$ | F19 (TFP process) |
| `c_e` | entrepreneur consumption $C^e_t$ | F9 (entrepreneur consumption) |
| `g` | government spending $G_t$ | F20 (government spending process) |
| `premium` | external finance premium $\Psi_t$ | F8 (premium, @#define) |
| `x1` | Calvo recursion $x_{1,t}$ | F11 |
| `x2` | Calvo recursion $x_{2,t}$ | F12 |
| `pstar` | optimal reset price $P^*/P$ | F13 |
| `delta_p` | price dispersion $\Delta_t$ | F15 |

**Pass: equations = variables = 20 (R4 check).**

### Exogenous variables (3)

| Variable name (ASCII) | Meaning |
|---|---|
| `eps_a` | TFP innovation (R3: innovations only)|
| `eps_g` | government spending innovation |
| `eps_m` | monetary policy innovation |

### Main parameters

| Parameter name (ASCII) | Meaning | Calibrated value |
|---|---|---|
| `betta` | household discount factor $\beta$ | 0.99 |
| `sigma` | risk aversion $\sigma$ | 1 |
| `phi_n` | inverse labor supply elasticity $\varphi$ | 3 |
| `psi` | labor disutility (back-solved from steady state) | — |
| `alpha` | capital share of output $\alpha$ | 0.35 |
| `delta` | depreciation rate $\delta$ | 0.025 |
| `theta` | Calvo probability of not resetting the price $\theta$ | 0.75 |
| `eps_p` | CES elasticity of substitution $\varepsilon$ | 6 |
| `phi_k` | investment adjustment cost $\phi_k$ | 1.0 |
| `gamma_e` | entrepreneur survival rate $\gamma$ | 0.9728 |
| `chi` | elasticity of the premium to leverage $\chi$ | 0.05 |
| `s_ss` | steady-state external finance premium (calibration target)| 1.005 |
| `gy_share` | government spending to output ratio | 0.20 |
| `rho_R` | interest-rate smoothing $\rho_R$ | 0.90 |
| `phi_pi` | Taylor rule inflation coefficient $\phi_\pi$ | 1.50 |
| `phi_y` | Taylor rule output gap coefficient $\phi_y$ | 0.125 |
| `rho_a` | TFP persistence $\rho_a$ | 0.90 |
| `rho_g` | government spending persistence $\rho_g$ | 0.90 |

The runnable file is `bgg_financial.mod`. Follow it when this note and the file differ.
