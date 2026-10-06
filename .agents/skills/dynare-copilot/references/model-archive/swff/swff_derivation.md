# SWFF model: derivation and replication notes
**Del Negro, Giannoni & Schorfheide (2015, AEJ:Macro), "Inflation in the Great Recession and New Keynesian Models"**

Read this when you study, reuse or extend the archived SWFF model (`swff.mod`): Smets-Wouters (2007) with financial frictions and a time-varying inflation target, written as a log-linearized `model(linear)`.

> Replication goal: with the paper's parameters (SWFF posterior mode, no estimation), rebuild the log-linearized model,
> pass the Blanchard-Kahn and steady-state checks, and produce IRFs and publication-quality figures for the main shocks
> (especially the financial/spread shock and the monetary policy shock).
> Not replicated: their 2008Q4 conditional forecast and the piecewise-linear solution for the ZLB with forward guidance (that needs their data set and an OccBin-type solution method).

---

## Section 1. Model overview

SWFF = **Smets-Wouters (2007) medium-scale NK model** + **two extensions**:
1. **Time-varying inflation target** π*_t (follows an AR(1) and enters the Taylor rule);
2. **Financial frictions** (the costly state verification mechanism of Bernanke-Gertler-Gilchrist / Christiano-Motto-Rostagno):
   banks lend to entrepreneurs; entrepreneurs lever their own net worth n to buy capital; idiosyncratic shocks make some entrepreneurs default;
   banks cover the default risk with a markup over the deposit rate (the spread). The spread moves with leverage and with entrepreneurial risk σ_ω.

The whole model is given as **log-linearized deviations from the steady state** (paper §2.1 gives the linear system directly). Skill rule R8 allows the exception "the source gives only a linearized system", so this replication uses Dynare `model(linear)`.

**Authoritative implementation source**: this replication transcribes, equation by equation, the equilibrium conditions of **FRBNY DSGE.jl `m990`** (`eqcond.jl`) and its financial-friction
steady state (`financial_frictions.jl` + `steadystate!` in `m990.jl`). m990 is the official code of the DGS2015 paper model.
Equations (3)–(21) of paper §2.1 map one to one onto m990. m990 also contains several shocks that FRBNY added later (`zp` long-run technology,
`μ_e` bankruptcy cost, `γ` entrepreneurial wealth, anticipated monetary policy shocks). In SWFF these have **zero variance (not estimated)**, so this replication removes them
and keeps only the 9 structural shocks that have a σ in paper Table A-2.

### The "two-track" structure of the economy
The SW Taylor rule uses the **output gap** = actual output y − flexible-price output y^f. So the model solves two economies at the same time:
- **Sticky economy**: price/wage rigidities + financial frictions — the actual economy;
- **Flexible economy** (superscript f): no nominal rigidities and no financial frictions — gives y^f.

---

## Section 2. Optimization problems of each agent (summary; for the first-order conditions see the SW2007 / BGG originals)

The paper and m990 give the linearized equilibrium conditions directly, so this section only lists each agent and the source of its first-order conditions. It does not repeat the derivation:

| Agent | Decisions | First-order condition → linear equation |
|---|---|---|
| Households | consumption/saving, labor, capital utilization, investment | Euler equation (F1), investment Euler (F2), capital utilization (F8), MRS (F13) |
| Unions (sticky wages) | Calvo wages + indexation | wage Phillips curve (F14) |
| Intermediate-goods firms (monopolistic competition, Calvo prices + Kimball aggregator) | price setting | price Phillips curve (F11); factor demands (F10, F12) |
| Capital producers | investment → capital transformation (investment adjustment costs) | capital law of motion (F9), Tobin's Q (F2) |
| Entrepreneurs + banks (BGG-CSV) | levered capital purchase, default, spread | return-on-capital definition (F3), spread equation (F4), net worth evolution (F5) |
| Central bank | interest rate feedback rule (with π*_t) | monetary policy rule (F15) |

---

## Section 3. Linearized equilibrium conditions (model core; numbered F1–F16 sticky + FF1–FF11 flexible)

Notation: lowercase = log deviation from steady state; `E_t x_{t+1}` is written `x(+1)` in Dynare; a lag is written `x(-1)`.
Helper constants: `zg = z_star` (log of the steady-state per-capita growth rate), `he = h*exp(-zg)`, `bb = betta*exp((1-sigc)*zg)` (the paper's β̄).

### 3.1 Sticky economy (actual economy, 16 equations)

**F1 Consumption Euler equation** (paper eq.3)
```
c = -(1-he)/(sigc*(1+he)) * (R - π(+1) - b)
    + he/(1+he) * (c(-1) - z)
    + 1/(1+he) * (c(+1) + z(+1))
    + (sigc-1)*wl_c/(sigc*(1+he)) * (L - L(+1))
```
(Note: in m990, b enters as the "raw preference shock" with coefficient 1; see the normalization in Section 6.)

**F2 Investment Euler (Tobin's Q)** (eq.4)
```
i = 1/(1+bb) * (i(-1) - z) + bb/(1+bb) * (i(+1) + z(+1))
    + 1/(S2*exp(2*zg)*(1+bb)) * qk + mu
```
where `S2 = S''` (second derivative of the investment adjustment cost).

**F3 Return on capital definition R̃^k** (eq.20)
```
Rtil_k - π = r_k_star/(1+r_k_star-delta) * rk + (1-delta)/(1+r_k_star-delta) * qk - qk(-1)
```

**F4 Spread equation** (eq.19; core of the financial-friction block)
```
Rtil_k(+1) - R = bcoef*b + zeta_spb*(qk + kbar - n) - sigw_t
```
where `bcoef = sigc*(1+he)/(1-he)`; `sigw_t` = financial risk shock σ̃_ω.

**F5 Entrepreneurial net worth evolution** (eq.21)
```
n = zeta_nRk*(Rtil_k - π) - zeta_nR*(R(-1) - π) + zeta_nqk*(qk(-1)+kbar(-1))
    + zeta_nn*n(-1) - (zeta_nsigw/zeta_spsigw)*sigw_t(-1)
    - (gstar_v_n)*z
```
`gstar_v_n = γ_star*vstar/nstar` (coefficient on z, see Section 6); γ_star = entrepreneur survival rate = 0.99 (fixed).

**F6 Production function** (eq.11) `y = Phi*(alppha*k + (1-alppha)*L)`

**F7 Capital utilization → effective capital** (eq.7) `k = u - z + kbar(-1)`

**F8 Optimal capital utilization** (eq.8) `u = (1-ppsi)/ppsi * rk`

**F9 Capital law of motion** (eq.5)
```
kbar = (1 - istar/kbarstar)*(kbar(-1) - z) + istar/kbarstar * i
       + istar*S2*exp(2*zg)*(1+bb)/kbarstar * mu
```

**F10 Real marginal cost** (eq.9) `mc = w + alppha*L - alppha*k`

**F11 Price Phillips curve** (eq.13)
```
π = kappa * mc + ι_p/(1+ι_p*bb) * π(-1) + bb/(1+ι_p*bb) * π(+1) + λ_f
```
`kappa = (1-zeta_p*bb)*(1-zeta_p) / ( zeta_p*((Phi-1)*eps_p+1)*(1+ι_p*bb) )` (Kimball).

**F12 Factor price relation** (eq.10) `k = w - rk + L`  ⇔ `rk + k - L - w = 0`

**F13 Labor MRS** (eq.15) `μ_ω = 1/(1-he)*(c - he*c(-1)) + he/(1-he)*z + ν_l*L` (that is, w^h)

**F14 Wage Phillips curve** (eq.14)
```
w = (1-zeta_w*bb)*(1-zeta_w)/(zeta_w*((λ_w-1)*eps_w+1)*(1+bb)) * (μ_ω - w)... 
```
(Full coefficients: eqcond.jl lines 237–247, transcribed term by term; includes w(-1), w(+1), π, π(-1), π(+1), z, z(+1), λ_w.)

**F15 Monetary policy rule** (eq.17)
```
R = ρ*R(-1) + (1-ρ)*( ψ1*(π - π_star) + ψ2*(y - y^f) ) + ψ3*((y-y^f) - (y(-1)-y^f(-1))) + rm
```

**F16 Resource constraint** (eq.12)
```
y = gstar*g + cstar/ystar*c + istar/ystar*i + r_k_star*kstar/ystar*u
```

### 3.2 Flexible economy (superscript f, 11 equations, **no financial frictions, no rigidities**)
FF1 Euler (same as F1, with R → real interest rate r^f, no π); FF2 investment Euler (same as F2);
**FF3 Capital arbitrage (no financial frictions)**: `r_k_star/(1+r_k_star-δ)*rk^f(+1) + (1-δ)/(1+r_k_star-δ)*qk^f(+1) - qk^f - r^f + bcoef_b = 0` (eqcond 116–120);
FF4 production; FF5 capital utilization → effective capital; FF6 optimal utilization; FF7 capital law of motion;
FF8 marginal cost = 0: `w^f = alppha*(k^f - L^f)`; FF9 factor prices; FF10 MRS: `w^f = μ_ω^f`; FF11 resource constraint.

### 3.3 Exogenous processes (9 active shocks; eqcond §EXOGENOUS)
```
ztil = ρ_z*ztil(-1) + ε_z                         (eq.1, trend-stationary technology level)
z    = (ρ_z-1)/(1-alppha)*ztil(-1) + 1/(1-alppha)*ε_z   (eq.2, growth rate; zp set to zero)
b    = ρ_b*b(-1) + ε_b
mu   = ρ_μ*mu(-1) + ε_μ
g    = ρ_g*g(-1) + ε_g + η_gz*ε_z                  (government spending, with technology spillover)
λ_f  = ρ_λf*λ_f(-1) - η_λf*λ_f1(-1) + ε_λf ; λ_f1 = ε_λf   (ARMA(1,1))
λ_w  = ρ_λw*λ_w(-1) - η_λw*λ_w1(-1) + ε_λw ; λ_w1 = ε_λw   (ARMA(1,1))
rm   = ρ_rm*rm(-1) + ε_rm                          (monetary policy shock)
sigw = ρ_σw*sigw(-1) + ε_σw                        (financial risk / spread shock)
π_star = ρ_π*π_star(-1) + ε_π*                     (time-varying inflation target)
```

---

## Section 4. Market clearing and Walras' law
The resource constraint F16 is goods market clearing. Bond/deposit market clearing is implicit in the entrepreneur-bank financing identity (already folded into F4/F5).
The linear system follows the m990 construction. The equation count strictly equals the variable count, **with no redundant equation** (the Sims gensys form is already a minimal representation).
After the transcription into Dynare, each equation is checked (Section 8). No further Walras-redundant equation needs to be removed.

---

## Section 5. Shock list (9, matching the entries with a σ in Table A-2)
ε_z (technology), ε_b (preference), ε_μ (MEI, investment-specific), ε_g (government), ε_λf (price markup), ε_λw (wage markup),
ε_rm (monetary), ε_σw (**financial/spread**), ε_π* (inflation target).
**Removed** (σ=0 in SWFF, not estimated): ε_zp, ε_μe, ε_γ, anticipated monetary policy shocks.

---

## Section 6. Steady state and financial-friction coefficients (copied from DSGE.jl; ready to code)

The steady state of the linear model is all zeros. First compute "deep parameters → derived parameters". Growth and prices:
```
zg   = log(1+gam) + alppha/(1-alppha)*log(Upsilon)   (Upsilon=1)
rstar= exp(sigc*zg)/betta
r_k_star = spr*rstar*Upsilon - (1-delta)
wstar = (alppha^alppha*(1-alppha)^(1-alppha)*r_k_star^(-alppha)/Phi)^(1/(1-alppha))
Lstar = 1
kstar = alppha/(1-alppha)*wstar*Lstar/r_k_star
kbarstar = kstar*(1+gam)*Upsilon^(1/(1-alppha))
istar = kbarstar*(1-(1-delta)/((1+gam)*Upsilon^(1/(1-alppha))))
ystar = kstar^alppha*Lstar^(1-alppha)/Phi
cstar = (1-gstar)*ystar - istar
wl_c  = wstar*Lstar/(cstar*λ_w)
```
**Financial-friction steady state** (BGG-CSV; `spr` = steady-state gross quarterly spread = 1 + SP*/400):
```
z_ω    = norminv(F_star)            (F_star=0.03, fixed default probability)
solve for sigw_ss:  zeta_spb_fn(z_ω, sigw_ss, spr) = zeta_spb   (fzero)
ω̄ = exp(sigw_ss*z_ω - sigw_ss^2/2)
G=Φ(z_ω-sigw_ss); Γ=ω̄(1-Φ(z_ω))+Φ(z_ω-sigw_ss); and their derivatives (see financial_frictions.m)
μ_e* = μ_fn(...);  nk* = nk_fn(...);  Rho* = 1/nk* - 1
derived: zeta_spsigw, zeta_nRk, zeta_nR, zeta_nqk, zeta_nn, zeta_nsigw
      (formulas copied one by one from m990 steadystate!, lines 645–661)
gstar_v_n = γ_star*vstar/nstar
```
> Numerical self-check: expect nk* (net worth/capital ≈ 1/leverage) in 0.4–0.6, F_star=0.03, annualized spread ≈ 1.9%.
> The MATLAB solution script `swff_ff_coeffs.m` computes these values and prints them for checking.

**Deep parameters (paper Table A-2, SWFF column, posterior mode)**
α=0.1787, ζp=0.8680, ιp=0.2259, Φ=1.5262, S''=3.0437, h=0.2440, ψ=0.1884, νl=2.6732,
ζw=0.8875, ιw=0.4187, r*=0.1331 (→β=1/(1+r*/100)=0.998670), ψ1=1.3737, ψ2=0.01804,
ψ3=0.2398, π*=0.7662 (→Π=1.007662), σc=1.3159, ρ(=ρR)=0.6750, γ (growth)=0.4012,
SP*=1.9081, ζsp,b=0.044292;
ρg=0.9793, ρb=0.9440, ρμ=0.6435, ρz=0.9564, ρλf=0.7939, ρλw=0.6609, ρrm=0.0673,
ρσw=0.9899, ρπ*=0.99 (fixed);
σg=2.9080, σb=0.0384, σμ=0.5033, σz=0.4961, σλf=0.1535, σλw=0.2568, σrm=0.2919,
σσw=0.0575, σπ*=0.0300;
ηgz=0.8737, ηλf=0.7143, ηλw=0.5720.
Fixed: δ=0.025, gstar=0.18, λw=1.5, εp=εw=10, Upsilon=1, F_star=0.03, γ_star (survival)=0.99.

---

## Section 7. Timing convention (R2)
- Capital `kbar` follows the "stock at the end of the period" convention and is a state variable: production/utilization use `kbar(-1)` (capital at the end of the previous period is used in this period);
  the left side of the law of motion is this period's end-of-period `kbar`. Effective capital `k = u - z + kbar(-1)`.
- Net worth `n` is a state variable: the right side of its law of motion contains `n(-1)`, `qk(-1)`, `kbar(-1)`, `R(-1)`, `sigw(-1)`.
- All AR/ARMA exogenous processes are endogenous variables; only the innovations `ε_*` are `varexo` (R3).
- The spread equation contains `Rtil_k(+1)` (expected return on capital); the Euler/Phillips/wage equations contain the corresponding `(+1)` lead terms.

---

## Section 8. Variable–equation table (R4 pre-check)

**Sticky economy (16 variables / 16 equations)**

| # | Variable | Determined by |
|---|---|---|
|1|c|F1 Euler|
|2|i (invest)|F2 investment Euler|
|3|qk|F3 return-on-capital definition|
|4|Rtil_k|(defined in F3; F4 gives its expectation) → F3|
|5|n|F5 net worth evolution|
|6|y|F6 production function|
|7|k|F7 effective capital|
|8|u|F8 optimal utilization|
|9|kbar|F9 capital law of motion|
|10|mc|F10 marginal cost|
|11|π|F11 price Phillips curve|
|12|rk|F12 factor price relation|
|13|μ_ω (w^h)|F13 MRS|
|14|w|F14 wage Phillips curve|
|15|R|F15 Taylor rule|
|16|(spread closure)|F4 spread equation|

> Note: F3 defines Rtil_k, and F4 closes the spread with E_t Rtil_k(+1); together they determine {qk, Rtil_k}. The 16 equations determine exactly the 16 sticky-economy variables.

**Flexible economy (11 variables / 11 equations)**: c^f, i^f, qk^f, rk^f, k^f, kbar^f, u^f, w^f, L^f, y^f, r^f — matching FF1–FF11.
In addition, L in the sticky economy is determined jointly by the lead terms in the F1 Euler equation and the whole system (SW standard: labor demand F12/F10 + closure by the wage equation).

**Exogenous processes (11 variables / 11 equations)**: ztil, z, b, mu, g, λ_f, λ_f1, λ_w, λ_w1, rm, sigw, π_star
(Note: z comes from its definition, ztil from the AR(1), and each λ has one MA helper variable.)

> **Total**: 16 (sticky) + 11 (flexible) + 12 (exogenous, including π_star) = 39 endogenous variables = 39 equations.
> When you build the .mod, run Dynare in Stage 3 (model block). The structural check passes when "Found 39 equation(s)" matches the variable count.

---

## Section 9. Replication plan (incremental build)
1. Write `swff_ff_coeffs.m` (BGG functions + ζ coefficients, MATLAB) → run it alone and print the nk*/spread self-check.
2. Write `swff.mod`: declarations → deep parameters (literal values) → derived parameters (injected by the script) → `model(linear)` → shocks → `stoch_simul`.
3. Three locked runs, each locking one check: structure (equation count) → steady state/Blanchard-Kahn (`check`) → IRF (no NaN).
4. Publication-quality IRF figures: responses of y, π, R, spread and n to the financial shock σ_ω, the monetary shock rm, technology z, preference b and others.
