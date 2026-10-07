# Trend RBC model with government investment: derivation

Read this when you study, reuse or extend the archived trend RBC model with a public capital externality (`trend_rbc_gov_inv.mod`): a detrended model on a balanced growth path, solved with `stoch_simul, order=1`.

**Model file**: `model.mod`  
**References**: Aschauer (1989) "Is public expenditure productive?", *Journal of Monetary Economics*; Barro (1990) "Government spending in a simple model of endogenous growth"; King, Plosser & Rebelo (1988) "Production, growth and business cycles"

---

## Section 1. Model overview

**Economic question**: the effect of government investment (public infrastructure spending) on the trend of the economy's aggregates.  
**Mechanism**: government investment builds a public capital stock, which enters the private production function as an externality and raises total factor productivity; at the same time it competes with private investment for resources (crowding out).  
**Economy**: closed economy, representative agent, perfectly competitive RBC framework, with deterministic labor-augmenting technical progress (trend growth rate $\gamma$).  
**Shocks**: (1) temporary total factor productivity shock $\varepsilon_z$; (2) government investment shock $\varepsilon_g$.  
**Method**: first-order linear approximation around the balanced growth path (BGP) of the detrended model (`stoch_simul, order=1`).

**Detrending convention**: divide every quantity variable by the technology level $A_t = (1+\gamma)^t$ to get detrended variables (lowercase); labor $N_t$ (hours) has no trend. Effective discount factor $\tilde{\beta} = \beta(1+\gamma)^{-1}$ (log utility).

---

## Section 2. Optimization problems of each agent

### 2.1 Representative household

Choose $\{C_t, N_t, I_t, K_t\}$ to maximize discounted expected utility:

$$\max \; E_0 \sum_{t=0}^{\infty} \beta^t \left[ \ln C_t - \psi \frac{N_t^{1+\varphi}}{1+\varphi} \right]$$

**Budget constraint** (levels):

$$C_t + I_t = W_t N_t + R_t^k K_{t-1} - T_t$$

where profits are zero (competitive equilibrium); the government finances government investment with a lump-sum tax $T_t = I_{G,t}$.

**Private capital accumulation** (levels; R2: the left side is the end-of-period stock):

$$K_t = (1-\delta) K_{t-1} + I_t$$

In detrended coordinates ($\hat{c}_t = C_t/A_t$, $\hat{k}_t = K_t/A_t$, etc.):

$$\hat{c}_t + \hat{\imath}_t = \hat{w}_t N_t + \hat{r}_t^k \hat{k}_{t-1}$$

$$(1+\gamma)\hat{k}_t = (1-\delta)\hat{k}_{t-1} + \hat{\imath}_t$$

### 2.2 Representative competitive firm

The firm maximizes profit statically each period. The production function contains labor-augmenting technology and a public capital externality (Aschauer 1989):

$$Y_t = e^{z_t} K_{G,t-1}^{\alpha_G} K_{t-1}^{\alpha} (A_t N_t)^{1-\alpha-\alpha_G}$$

Detrended form:

$$\hat{y}_t = e^{z_t} \hat{k}_{G,t-1}^{\alpha_G} \hat{k}_{t-1}^{\alpha} N_t^{1-\alpha-\alpha_G}$$

$K_{G,t}$ is the public capital stock (exogenous to the firm, an externality); the firm takes it as given.

Profit maximization:

$$\max_{\{K_{t-1}, N_t\}} \hat{y}_t - \hat{r}_t^k \hat{k}_{t-1} - \hat{w}_t N_t$$

### 2.3 Government

The government chooses $I_{G,t}$ and finances it with a lump-sum tax ($T_t = I_{G,t}$). It does not optimize (exogenous rule):

$$\ln I_{G,t} = (1-\rho_g)\ln I_{G}^{ss} + \rho_g \ln I_{G,t-1} + \varepsilon_{g,t}$$

Public capital accumulation (levels; detrended: $(1+\gamma)\hat{k}_{G,t} = (1-\delta_G)\hat{k}_{G,t-1} + \hat{\imath}_{G,t}$):

$$K_{G,t} = (1-\delta_G)K_{G,t-1} + I_{G,t}$$

---

## Section 3. First-order conditions (FOC)

Lagrange multiplier $\lambda_t$ on the budget constraint, $\mu_t$ on capital accumulation. After eliminating $\mu_t$ (no adjustment costs), $\lambda_t = C_t^{-1} = \hat{c}_t^{-1} A_t^{-1}$.

**(F1) Consumption Euler equation** (log utility, detrended):

$$\hat{c}_t^{-1} = \frac{\beta}{1+\gamma} E_t\left[\hat{c}_{t+1}^{-1}\left(\hat{r}_{t+1}^k + 1-\delta\right)\right]$$

**(F2) Labor supply FOC** (consumption-leisure trade-off):

$$\psi N_t^{\varphi} \hat{c}_t = \hat{w}_t$$

**(F3) Labor demand (firm FOC for $N_t$)**:

$$\hat{w}_t = (1-\alpha-\alpha_G) \frac{\hat{y}_t}{N_t}$$

**(F4) Capital demand (firm FOC for $K_{t-1}$)**:

$$\hat{r}_t^k = \alpha \frac{\hat{y}_t}{\hat{k}_{t-1}}$$

---

## Section 4. Market clearing and Walras' law check

**Goods market clearing** (resource constraint, detrended):

$$\hat{y}_t = \hat{c}_t + \hat{\imath}_t + \hat{\imath}_{G,t}$$

**Walras' law check for the redundant equation**:

The model has three conditions: the household budget constraint, goods market clearing, and the government budget ($T_t = I_{G,t}$). Of these:
- Household: $\hat{c}_t + \hat{\imath}_t = \hat{w}_t N_t + \hat{r}_t^k \hat{k}_{t-1} - T_t$
- Firm zero profit: $\hat{y}_t = \hat{w}_t N_t + \hat{r}_t^k \hat{k}_{t-1}$ (CRS)
- Government: $T_t = \hat{\imath}_{G,t}$ (detrended)

Substitution gives $\hat{c}_t + \hat{\imath}_t = \hat{y}_t - \hat{\imath}_{G,t}$, which is the resource constraint.

**Conclusion**: goods market clearing follows from the household budget constraint + firm zero profit + the government budget identity. So **in the sense of Walras' law, the household budget constraint is the redundant equation**.  
The `model` block contains **only the resource constraint**, not the household budget constraint.

---

## Section 5. Exogenous processes

**(E1) Temporary TFP shock** (AR(1), log level):

$$z_t = \rho_z z_{t-1} + \varepsilon_{z,t}, \quad \varepsilon_{z,t} \sim \mathcal{N}(0, \sigma_z^2)$$

**(E2) Government investment process** (log AR(1), mean equal to the steady-state value):

$$\ln \hat{\imath}_{G,t} = (1-\rho_g)\ln \hat{\imath}_{G}^{ss} + \rho_g \ln \hat{\imath}_{G,t-1} + \varepsilon_{g,t}, \quad \varepsilon_{g,t} \sim \mathcal{N}(0, \sigma_g^2)$$

where $\hat{\imath}_{G}^{ss}$ is back-solved in the steady-state block from $\hat{\imath}_{G}^{ss} / \hat{y}^{ss} = g_y$ (calibration target).

---

## Section 6. Steady state (detrended, sequential computation)

1. $z^{ss} = 0$

2. **Rental rate of capital** (from the Euler equation F1):

$$\hat{r}^{k,ss} = \frac{1+\gamma}{\beta} - (1-\delta) = \frac{1+\gamma}{\beta} - 1 + \delta$$

3. **Capital-output ratio** (from F4): $ky \equiv \hat{k}^{ss}/\hat{y}^{ss} = \alpha / \hat{r}^{k,ss}$

4. **Public capital-output ratio** (from the steady state of the public capital accumulation equation): $kgy \equiv \hat{k}_G^{ss}/\hat{y}^{ss} = g_y/(\gamma+\delta_G)$

5. **Normalized labor target**: $N^{ss} = 1/3$ (calibration target; back-solves $\psi$)

6. **Steady-state output** (from the production function, $\hat{y}^{ss} = kgy^{\alpha_G} ky^{\alpha} \hat{y}^{ss^{\alpha_G+\alpha}} N^{ss^{1-\alpha-\alpha_G}}$):

$$\hat{y}^{ss} = \left(kgy^{\alpha_G} \cdot ky^{\alpha}\right)^{1/(1-\alpha-\alpha_G)} \cdot N^{ss}$$

7. $\hat{k}^{ss} = ky \cdot \hat{y}^{ss}$, $\hat{k}_G^{ss} = kgy \cdot \hat{y}^{ss}$

8. $\hat{\imath}^{ss} = (\gamma+\delta)\hat{k}^{ss}$ (private investment); $\hat{\imath}_G^{ss} = g_y \hat{y}^{ss}$ (government investment)

9. $\hat{c}^{ss} = \hat{y}^{ss} - \hat{\imath}^{ss} - \hat{\imath}_G^{ss}$ (resource constraint)

10. $\hat{w}^{ss} = (1-\alpha-\alpha_G)\hat{y}^{ss}/N^{ss}$ (labor demand F3)

11. **Back-solve the labor disutility weight** (from F2): $\psi = \hat{w}^{ss} / \left((N^{ss})^{\varphi} \hat{c}^{ss}\right)$

---

## Section 7. Timing convention (R2, stock at the end of the period)

| Variable | Timing class | Dynare form | Note |
|------|---------|-------------|------|
| $\hat{k}_t$ | State (end-of-period stock) | `k` current period, `k(-1)` previous period; production uses `k(-1)` | decided at the end of period $t$, used in production in period $t+1$ |
| $\hat{k}_{G,t}$ | State (end-of-period stock) | `kg` current period, `kg(-1)` previous period; production uses `kg(-1)` | same as above |
| $z_t$ | State | `z(-1)` in the AR process | log level |
| $\hat{\imath}_{G,t}$ | State | `ig(-1)` in the AR process | log AR |
| $\hat{c}_t, N_t, \hat{\imath}_t, \hat{y}_t$ | Control (forward-looking) | no lag | decided in the current period |
| $\hat{w}_t, \hat{r}_t^k$ | Forward-looking (helper) | no lag | determined within the period by the firm FOCs |

---

## Section 8. Variable and parameter table (R4 pre-check)

| Endogenous variable | Determined by |
|---------|--------------|
| `y` | production function |
| `c` | resource constraint |
| `k` | private capital accumulation equation |
| `n` | labor market clearing (F2 = F3, solved jointly) |
| `invest` | private investment (follows from F4 + Euler; equivalently: capital accumulation + resource constraint) |
| `ig` | government investment AR(1) process |
| `kg` | public capital accumulation equation |
| `z` | TFP AR(1) process |
| `w` | labor demand FOC (F3) |
| `rk` | capital demand FOC (F4) |
| `log_y`…`log_n` | 6 log helper equations |

**Equation count**: 10 + 6 = 16; **endogenous variable count**: 16. R4 is satisfied.

**Exogenous variables**: `eps_z` (TFP innovation), `eps_ig` (government investment innovation).

**Calibration summary**:

| Parameter | Value | Description |
|------|-----|------|
| `betta` | 0.99 | quarterly discount factor |
| `gam` | 0.005 | quarterly trend growth rate (≈2% annualized) |
| `delta` | 0.025 | private capital depreciation (≈10% annualized) |
| `deltag` | 0.05 | public capital depreciation (≈20% annualized, infrastructure) |
| `alppha` | 0.33 | output elasticity of private capital |
| `alphag` | 0.10 | public capital externality elasticity (Aschauer 1989) |
| `phhi` | 1.0 | inverse Frisch elasticity |
| `psi` | *back-solved* | labor disutility, hits $N^{ss}=1/3$ |
| `rhoz` | 0.95 | TFP shock persistence |
| `rho_ig` | 0.80 | government investment shock persistence |
| `gy` | 0.05 | steady-state government investment/output ratio = 5% |
| `ig_ss` | *back-solved* | steady-state government investment level |
