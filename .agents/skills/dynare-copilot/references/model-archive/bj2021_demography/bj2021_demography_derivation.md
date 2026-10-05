# Basso & Jimeno (2021, JME) — Dynare replication derivation

Read this when you replicate an OLG model with demographic change, or set up a perfect foresight transition between two balanced growth paths with `initval`/`endval`.

Detrended BGP equilibrium system + deterministic (perfect foresight) transition after a demographic shock.
Source: Banco de España DT 2004 (`paper.txt`), Appendix A (equilibrium conditions), Appendix B (detrending, lines 1799–2010), Appendix D (calibration, lines 2218+).

> This note records the derivation and the open choices: the implementation scope (section "Scope options") and a few parameters that the OCR left ambiguous. The `.mod` in this folder is the model that runs. Follow the `.mod` when the note and the file differ.

---

## Scope options

The full detrended system has about **40 equations**, and after OCR of the appendix tables **some parameter symbols are misaligned or cannot be read without ambiguity**
(details under "Timing convention and OCR-ambiguous parameters"). I give three options; **I recommend B**:

| Option | Content | Equations | Risk | What it can replicate |
|---|---|---|---|---|
| **A Full structure** | Copy all 40 equations of Appendix B one by one (household Epstein-Zin + MPC recursions, two-sector production, R&D innovation/automation, robots, financial intermediaries, all market clearing) | ~40 | **High**: the ambiguous parameters need guessed values in many places; convergence and Blanchard-Kahn (BK) problems are likely; many debugging rounds | In principle all paths of Fig.1 |
| **B Core GE (recommended)** | Keeps **all mechanisms** of the paper faithfully but merges redundant bookkeeping: two-sector production + automation, R&D innovation/automation trade-off, robot curvature χ, Gertler life-cycle saving (MPC changes with demographics), demographic block, market clearing | ~22–26 | Medium | Transition paths of per-capita growth, automation a_z, labor share, interest rate and robot price after the demographic change (the core message of Fig.1) |
| **C Growth-accounting block** | Turn the demographic block + the BGP growth/price relations of Proposition 1 into a Dynare perfect foresight transition | ~10 | Low | Transition of per-capita growth and g_q (= the analytical core I already did, but through the Dynare solver) |

**Why B rather than A**: the paper's two core channels are (i) demographics → saving → interest rate (household side) and (ii) wage/robot price →
innovation vs automation trade-off (production + R&D side). B keeps both in full. What A adds is the term-by-term recursive bookkeeping of Epstein-Zin and of
dividends/human wealth: with ambiguous parameters it is high-risk, and it does not change the qualitative conclusions. Option A needs more debugging.

The sections below derive **option B** (A is a superset of it; C is a subset of it).

---

## §1 Model overview

Endogenous-growth OLG with three sectors + households:
- **Final good**: CES aggregate of a continuum of intermediate goods `i∈[0,Z_t]`, elasticity of substitution ε (>1).
- **Two intermediate-goods sectors**:
  - Labor-intensive `i∈Z_t\A_t`: `y = (K^α L^{1-α})^{1-γ_I} τ^{γ_I}`
  - Automated `i∈A_t`: `y = (K^α (ζ M)^{1-α})^{1-γ_I} τ^{γ_I}`; robots M replace labor; ζ = relative productivity of robots.
  A good that can be automated always uses robots (robots are more productive than labor).
- **R&D**: innovation (create new goods, Z↑; a new good can at first be produced only with labor) + automation (convert existing goods so that robots can produce them, A↑). The two
  compete for limited R&D labor and loans → **innovation/automation trade-off** (the core of the paper).
- **Robot production**: Φ units of final good in, `M = ε̄ Φ^χ` out; **χ<1** is the bottleneck constraint that prevents the "no-labor singularity".
- **Households**: Gertler (1999) two life stages (worker w / retiree r), Epstein-Zin recursive utility (risk neutrality + positive intertemporal substitution).
  Perfect annuity markets. Population dynamics are driven by births ψ_y, retirement 1-ω and retiree survival ψ_r.

**Sources of growth**: new varieties Z↑ (productivity + reallocation of employment) and automation A↑ (productivity + displacement of employment).
BGP: `g = g_n` (total output grows at the population growth rate? See §3: Proposition 1 actually gives `g^{1-χ}=g_n`); per-capita growth = `g_n^{χ/(1-χ)}`.

## §2 Optimization problems of the agents (details in paper.txt §2 and Appendix A)

- **Final-good firm**: `max` profit → demand `y_i = (p_i/p)^{-ε} y`.
- **Intermediate-goods firms** (two sectors): given output, minimize cost `T C = p q M + (r^k+δ)K + P τ` (automated) /
  `= W L + (r^k+δ)K + P τ` (labor). → Factor demands (A.1)–(A.4), (A.8)–(A.11); relative prices (A.5)/(A.12).
- **Innovators**: invest S and hire L_I to create new goods; receive the profit share ν of new goods. FOC (11)+(A.20)/(A.21).
- **Automators**: invest φ and hire L_A to convert labor-produced goods into automated goods, with success probability σ_t; receive the profit share ν of automated goods. FOC (A.23)–(A.25).
- **Robot producers**: `max q M − Φ`, s.t. `M=ε̄Φ^χ` → (A.15)–(A.17), `q = Φ/M·(1/χ)` (curvature pricing).
- **Households**: worker/retiree Epstein-Zin recursions, budget constraint (3). → MPC of workers Φ_t and of retirees Π_t (>Φ),
  consumption functions (6)/(A.42)/(A.33), MPC recursions (A.44)/(A.35), human wealth (A.45), present value of dividends (A.46).

## §3 Key FOCs / BGP relations (Proposition 1, χ<1, with technical progress)

Let `g=Y_t/Y_{t-1}`, `g_n=N_t/N_{t-1}`, `g_q=q_t/q_{t-1}`, `g_wg=W_t/W_{t-1}`.

- **F1 (relative prices grow together)** The relative prices of the two sectors grow at the same rate on the BGP → `g_q = g_wg` (A.69d/A.69l + 16/17).
- **F2 (labor supply)** 1 unit of effective labor per person → `g_L = g_n`, so `g_q = g_wg = g/g_n`.
- **F3 (robot production on the BGP)** `M=ε̄Φ^χ` + constant robot share → `g^{-χ} g_q = 1`.
- **F1–F3 combined** → `g^{1-χ} = g_n` ⇒ **per-capita growth `g/g_n = g_n^{χ/(1-χ)}`** (the engine of Corollary 2).
- **F4 (variety growth)** `g_Z = g_A = g^{(1-α)(ε-1)(1-ρ̂)(1-γ_I)}` (for ρ̂ see the OCR-ambiguous parameters).
- **A1**: `a_z ≤ ā` (upper bound on the share of automated goods; ≤75% under the calibration).
- **A2**: `ε > 1 + 1/[(1-α)(1-ρ̂)(1-γ_I)]` (markup<23%).

## §4 Market clearing and Walras' law check

Market clearing (A.60–A.66 / A.72f–l): final good `1 = c+i+s+φ+τ̃+...`, capital `k=k_m+k_L`,
inputs `τ̃=τ_m+τ_L`, robots `M=A·m_j`, labor `N^{wRD}=L_I+L_A`, `N^{wL}=L`, variety shares `1=y_m+y_L`.

**Walras redundancy**: with zero profit of financial intermediaries (A.55, `F_t=0`) + worker/retiree asset flows (A.58/A.59) + the budget constraints of all agents
taken together, **the final-good resource constraint (A.72g) is redundant** (it is a linear combination of the other conditions).
→ **The resource constraint does not go into the model block** (or it goes in, and one of the asset flows is removed). Choose one of the two when you implement, so that the BK check does not fail because of linearly dependent equations.

## §5 Exogenous processes (drivers of the deterministic transition)

No stochastic shocks. Exogenous = **demographic parameters**, with a permanent jump between initval (old BGP) and endval (new BGP):
- `psi_y` (birth rate n): US 0.0265→0.0236; EU 0.0253→0.0206
- `psi_r` (retiree survival = 1−mortality rate): US 0.93→0.963; EU 0.94→0.976
- `omega` fixed = 1−1/45 = 0.97778 (retirement probability 1/45).

Implement it with the `Solow_growth_rate_changes.mod` pattern: declare psi_y and psi_r as `varexo`;
`initval` gives the 1993 values and solves the old BGP; `endval` gives the 2055 values and solves the new BGP; `perfect_foresight_solver` computes the transition.

## §6 Steady state (BGP): solution order

On the BGP the detrended variables are constant. Solution chain (given demographics → g_n):
1. Demographic block: `g_n=g^w=ω+ψ_y`, `τ^r=(1-ω)/(g^w-ψ_r)`, shares.
2. Growth: `g=g_n^{1/(1-χ)}`, `g_q=g_wg=g/g_n`, `g_Z=g_A` (F4), `a_z` (constant from A.70b: `g_A=σ(1/a_z−1)+φ`).
3. Interest rate R: from the household MPC steady state (A.67g/h) jointly with the asset market (solve numerically, one dimension).
4. Factor prices: `r^k = R−1+δ` (A.71a); `W` and `q` from relative prices and shares (A.69i lst).
5. Sector shares `y_m,y_L` (1=y_m+y_L), R&D allocation `s,φ,l_i,l_a`, values `v,j`, consumption shares.
*Most of it is closed-form or semi-closed-form; R and the shares may need a 1–2 dimensional numerical root (back-solve inside steady_state_model; see trend_rbc_gov_inv).*

## Timing convention and OCR-ambiguous parameters

**Timing**: state variables (stock at the end of the period) `K,Z,A,B,FA^w,FA^r` carry `(-1)` in the current period; the law of motion has the end-of-period quantity on the left (R2).
Growth-rate variables `g,g_q,…` are forward-looking variables.

**Ambiguous parameters (the OCR of table D is misaligned; the narrative conflicts with the table) — my recommended values**:

| Symbol | Meaning | Recommended value | Basis / uncertainty |
|---|---|---|---|
| α | capital share | 0.33 | clear |
| γ_I | intermediate input share | 0.5 | clear |
| ε | elasticity of substitution between varieties | 8 | markup≈14.3% (the paper says 15%)|
| β | discount factor | 0.96 | clear |
| ρ_EZ | EIS parameter | −3 | EIS=0.25=1/(1−ρ) |
| δ | depreciation | 0.08 | clear |
| 1−ω | retirement probability | 0.0222 | =1/45, clear |
| ξ_L | elasticity of inventions to R&D labor | 0.5 | Aksoy/Jones, clear |
| ρ_RD | R&D investment weight | 1 | narrative "ρRD=1" |
| **χ** | **robot-production curvature** | **see next column** | The table says 0.15, but back-solving from `g^{1-χ}=g_n` + 1.6% per-capita growth gives ≈0.79 → **I recommend calibrating to the 1.6% growth target (≈0.79)** rather than copying the table value 0.15 |
| **ρ̂** | the elasticity in the F4/A2 exponent | **≈0.31** | The narrative says "elasticity of intermediate goods to R&D 0.9", but substituting it violates A2; setting the A2 bound = 23% and back-solving gives ρ̂≈0.31. **Recommend 0.31** (to meet the markup<23% bound that the paper itself states)|
| χ_a | automation rate | 0.1 | narrative |
| φ_surv | variety survival rate (obsolescence) | **0.97** | Lost in the table OCR; typical Comin-Gertler value; **needs confirmation** |
| χ̃ | innovation productivity scale | **calibrated** | Back-solved from "innovation spending/GDP=0.012" |
| ζ̄ | relative productivity of robots | calibrate so that W>q/ζ | Explicitly an inequality constraint |

Open choices, with the recommended value first: (1) scope A, B or C; (2) χ ≈ 0.79 or the table value 0.15; (3) ρ̂ = 0.31 or 0.9; (4) φ_surv = 0.97. Use the recommended value unless the user chooses another.

## Variable and equation map (option B)

| Variable | Meaning | Determined by equation |
|---|---|---|
| g, g_q, g_Z(=g_A) | growth rates | F3, F1, F4 |
| g_n, g_w, tau_r | demographics | demographic block A.68 |
| a_z | degree of automation A/Z | A.70b constant |
| y_m, y_L | sector output shares | A.69e/k + 1=y_m+y_L |
| ls | labor share | A.69i |
| R, rk | interest rate / rental rate | A.71a + household saving |
| W, q | wage / robot price | relative prices A.69 |
| c, c_w, c_r | consumption | A.67e/f/g/h |
| Phi, Pi | worker/retiree MPC | A.67g/h(A.44/A.35) |
| s, phi_a, l_i, l_a | R&D inputs / labor | A.70a,l,m + allocation |
| v, j | value of automated / innovated goods | A.70 |
| k, b, fa_w, fa_r | capital / debt / assets | A.72 + asset flows |

Options A, B and C have different variable counts. Each variable needs one equation. The `.mod` in this folder is the implemented scope.
