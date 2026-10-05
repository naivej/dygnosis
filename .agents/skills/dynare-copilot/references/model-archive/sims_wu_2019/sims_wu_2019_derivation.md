# Sims & Wu (2019) "The Four Equation New Keynesian Model" — replication notes

Read this when you replicate a paper that gives only a linearized system (`model(linear);`), or an NK model with a QE rule and credit shocks.

Replication target: Eric Sims & Jing Cynthia Wu, *The Four Equation New Keynesian Model* (2019).
This replication implements the **linearized system of Subsection 2.1 / 2.3** of the paper (the positive analysis used for Figures 1–4),
not a new derivation from microfoundations: the paper already gives the full nonlinear model and its log-linearization in Appendix A–B.
All variables are log deviations from the steady state, so the steady state is always 0. Use `model(linear);` (consistent with skill rule R8 exception 2:
the replicated paper gives only a linearized system / the user explicitly asks for a linear version).

---

## 1. Model overview

Four-equation NK model = standard three-equation NK model (IS, Phillips, interest-rate rule) + a fourth equation, the QE rule.
Financial intermediaries face a risk-weighted leverage constraint. The credit shock θ and the central bank's long-bond holdings qe **enter both the IS curve and the Phillips curve**,
so a credit shock has a double effect, both "demand" and "cost-push", and the Divine Coincidence fails.
With `z=0` the model reduces to the standard three-equation model.

## 2. Endogenous variables (7)

| Variable | Meaning                                                   | Type            | Determining equation |
| -------- | --------------------------------------------------------- | --------------- | -------------------- |
| `x`      | output gap x_t = y_t − y_t^f                              | forward-looking | IS (E1)              |
| `pi`     | inflation π_t                                             | forward-looking | PC (E2)              |
| `rs`     | short-term nominal interest rate r^s_t                    | forward-looking | Taylor (E3)          |
| `qe`     | real value of the central bank's long-bond portfolio qe_t | state (AR)      | QE rule (E4)         |
| `rf`     | natural rate of interest r^f_t                            | state (AR)      | process (E5)         |
| `theta`  | credit shock θ_t (leverage; positive = easing)            | state (AR)      | process (E6)         |
| `exr`    | expected excess return on long bonds E_t r^b_{t+1}−r^s_t  | forward-looking | definition (E7)      |

Exogenous innovations (4): `eps_f` (natural rate), `eps_theta` (credit), `eps_r` (monetary policy), `eps_q` (QE).

## 3. Equations (mapped to the paper's equation numbers)

- **E1 IS curve** (eq. 2.1):
  x_t = E_t x_{t+1} − ((1−z)/σ)·(r^s_t − E_t π_{t+1} − r^f_t)
        − z·[ b̄^FI·(E_t θ_{t+1} − θ_t) + b̄^cb·(E_t qe_{t+1} − qe_t) ]

- **E2 Phillips curve** (eq. 2.2):
  π_t = γζ·x_t − (zγσ/(1−z))·[ b̄^FI·θ_t + b̄^cb·qe_t ] + β·E_t π_{t+1}

- **E3 Taylor rule** (eq. 2.33):
  r^s_t = ρ_r·r^s_{t−1} + (1−ρ_r)·(φ_π·π_t + φ_x·x_t) + ε_{r,t}

- **E4 QE rule** (eq. 2.34):  qe_t = ρ_q·qe_{t−1} + ε_{q,t}

- **E5 Natural rate process** (eq. 2.35):  r^f_t = ρ_f·r^f_{t−1} + ε_{f,t}

- **E6 Credit shock process** (eq. 2.36):  θ_t = ρ_θ·θ_{t−1} + ε_{θ,t}

- **E7 Expected excess return** (used for Fig. 4, from eqs. B.37 + 2.37):
  E_t r^b_{t+1} − r^s_t = E_t π_{t+1} + σ·[ b̄^FI·(E_t θ_{t+1}−θ_t) + b̄^cb·(E_t qe_{t+1}−qe_t) ] − r^s_t

  Pass: equations = variables = 7 (R4 holds). E7 is a definition and does not affect the dynamics of x/π/rs.

## 4. Calibration (Table 1, "parameters of the linearized model")

| Parameter   | Value  | Meaning                                                  |
| ----------- | ------ | -------------------------------------------------------- |
| β (betta)   | 0.995  | discount factor                                          |
| z           | 0.33   | child consumption share                                  |
| σ (sigma)   | 1      | inverse of the intertemporal elasticity of substitution  |
| b̄^FI (bFI)  | 0.70   | weight on leverage in IS/PC                              |
| b̄^cb (bcb)  | 0.30   | weight on QE in IS/PC (bFI+bcb=1)                        |
| γ (gam)     | 0.086  | elasticity of inflation with respect to real marginal cost |
| ζ (zeta)    | 2      | elasticity of the output gap with respect to real marginal cost |
| ρ_r         | 0.8    | Taylor smoothing                                         |
| φ_π         | 1.5    | Taylor inflation coefficient                             |
| φ_x         | 0      | Taylor gap coefficient                                   |
| ρ_f,ρ_θ,ρ_q | 0.8    | AR(1) coefficients of the three exogenous processes      |

**Note (small calibration inconsistencies; values follow Table 1)**:
- The text says the PC slope is γζ = 0.21, but γ=0.086 and ζ=2 from Table 1 give γζ=0.172.
  This replication uses exactly the values listed in Table 1 (the table is explicitly labeled "used to solve the linearized model").
- The formula ζ=(χ(1−z)+σ)/(1−z) with χ=σ=1 and z=0.33 should give ζ≈2.49; Table 1 rounds it to 2. Again Table 1 is used.

## 5. Timing convention

- AR state variables (qe, rf, theta): the law of motion has the end-of-period stock on the left, and the variable itself carries a lag `(-1)` in the equation;
  expectation terms such as E_t qe_{t+1}=ρ_q·qe are written with `(+1)` (in a linear model Dynare takes the conditional expectation automatically).
- Forward-looking variables (x, pi, rs, exr) have no lag; rs carries `rs(-1)` because of interest-rate smoothing.
- All variables are log deviations and the steady state is all zeros → no steady_state_model is needed (zero steady state by default).

## 6. Experiment and replication targets

- Shocks: a unit shock to each of eps_f / (eps_theta or eps_q) / eps_r, `stoch_simul(order=1, irf=20)`.
- Replicate Figure 1 (natural rate / potential output shock), Figure 2 (monetary policy shock),
  Figure 3 (leverage/QE shock; the two differ only by a scale factor), Figure 4 (excess return after an MP vs a QE shock).
- Report inflation and interest-rate IRFs annualized (×4).
