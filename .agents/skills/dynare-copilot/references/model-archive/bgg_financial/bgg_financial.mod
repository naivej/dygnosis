/*
 * BGG financial-frictions comparison
 * Source: Bernanke, Gertler & Gilchrist (1999), Handbook of Macroeconomics
 * Comparison: WITH_FA=1 (costly-state-verification financial frictions) vs WITH_FA=0 (frictionless NK)
 * Timing: stock at the end of the period (R2). K_t is capital at the end of t; production uses K_{t-1}
 * Form: nonlinear (R8). Dynare takes the first-order approximation
 * Run: dynare bgg_financial                  (WITH_FA=1, the default)
 *       dynare bgg_financial -DWITH_FA=0      (frictionless NK comparison)
 */

// Frictions on by default; -DWITH_FA=0 on the command line overrides this
@#ifndef WITH_FA
@#define WITH_FA = 1
@#endif

//============================================================
// Declarations (20 endogenous variables, equations F1-F20)
//============================================================
var
    c        ${C}$         (long_name='household consumption')
    n_lab    ${N}$         (long_name='labor hours')
    infl     ${\Pi}$       (long_name='gross CPI inflation')
    r        ${R}$         (long_name='gross real interest rate')
    rn       ${R^n}$       (long_name='gross nominal interest rate')
    q        ${Q}$         (long_name='Tobin Q - price of capital')
    k        ${K}$         (long_name='capital stock end-of-period')
    nw       ${N^e}$       (long_name='entrepreneur net worth')
    rk       ${R^k}$       (long_name='gross return on capital')
    y        ${Y}$         (long_name='output')
    mc       ${MC}$        (long_name='real marginal cost')
    invest   ${I}$         (long_name='investment')
    a        ${A}$         (long_name='TFP level')
    c_e      ${C^e}$       (long_name='entrepreneur consumption')
    g        ${G}$         (long_name='government spending')
    premium  ${\Psi}$      (long_name='external finance premium')
    x1       ${x_1}$       (long_name='Calvo recursive sum 1')
    x2       ${x_2}$       (long_name='Calvo recursive sum 2')
    pstar    ${P^*/P}$     (long_name='optimal reset price ratio')
    delta_p  ${\Delta}$    (long_name='price dispersion')
;

varexo
    eps_a  ${\varepsilon^a}$  (long_name='TFP innovation')
    eps_g  ${\varepsilon^g}$  (long_name='government spending innovation')
    eps_m  ${\varepsilon^m}$  (long_name='monetary policy innovation')
;

//============================================================
// Parameter declarations
//============================================================
parameters
    betta    ${\beta}$        (long_name='household discount factor')
    sigma    ${\sigma}$       (long_name='risk aversion')
    phi_n    ${\varphi}$      (long_name='inverse Frisch elasticity')
    psi      ${\psi}$         (long_name='labor disutility - SS calibrated')
    alpha    ${\alpha}$       (long_name='capital share')
    delta    ${\delta}$       (long_name='depreciation rate')
    theta    ${\theta}$       (long_name='Calvo non-adjustment probability')
    eps_p    ${\varepsilon_p}$(long_name='CES substitution elasticity')
    phi_k    ${\phi_k}$       (long_name='investment adjustment cost coefficient')
    gamma_e  ${\gamma}$       (long_name='entrepreneur survival rate')
    chi      ${\chi}$         (long_name='premium elasticity wrt leverage')
    we       ${\bar{W}^e}$    (long_name='entrepreneur endowment - SS calibrated')
    s_ss     ${\bar{s}}$      (long_name='steady state external finance premium target')
    gy_share                  (long_name='government spending to output ratio')
    rho_R    ${\rho_R}$       (long_name='Taylor rule smoothing')
    phi_pi   ${\phi_\pi}$     (long_name='Taylor rule inflation coefficient')
    phi_y    ${\phi_y}$       (long_name='Taylor rule output coefficient')
    rho_a    ${\rho_a}$       (long_name='TFP persistence')
    rho_g    ${\rho_g}$       (long_name='government spending persistence')
    y_ss                      (long_name='steady state output - for Taylor rule')
    rn_ss                     (long_name='steady state nominal rate - for Taylor rule')
    g_ss                      (long_name='steady state gov spending - for AR(1) mean')
;

//============================================================
// Parameter calibration
//============================================================

betta    = 0.99;
sigma    = 1;
phi_n    = 3;
alpha    = 0.35;
delta    = 0.025;
phi_k    = 1.0;
theta    = 0.75;
eps_p    = 11;       // markup about 10%, as in BGG

gamma_e  = 0.9728;   // survival rate (original BGG)
chi      = 0.05;     // premium elasticity to leverage (BGG niv=0.05)
s_ss     = 1.005;    // steady-state quarterly premium 0.5% (original BGG)

gy_share = 0.20;
rho_R    = 0.90;
phi_pi   = 1.50;
phi_y    = 0.125;
rho_a    = 0.90;
rho_g    = 0.90;

// Placeholder (updated after the steady-state reverse-solve)
psi      = 1;
we       = 0.01;
y_ss     = 1;
rn_ss    = 1.0101;
g_ss     = 0.2;

//============================================================
// Closed-form steady state (each line solves from the lines above)
//============================================================
steady_state_model;

    // exogenous steady state
    a        = 1;
    infl     = 1;
    delta_p  = 1;

    // interest rates
    r        = 1/betta;
    rn       = r;

    // external finance premium
    @#if WITH_FA
    premium  = s_ss;
    @#else
    premium  = 1;
    @#endif

    // return on capital (steady state of capital arbitrage F4)
    rk       = premium * r;

    // marginal cost (Calvo, zero inflation)
    mc       = (eps_p - 1) / eps_p;

    // Tobin's Q (steady-state adjustment cost is 0)
    q        = 1;

    // capital-output ratio (steady state of F3: rk = mc*alpha*yk + (1-delta), Q=1)
    yk_ss    = (rk - (1-delta)) / (mc * alpha);

    // capital-labor ratio (yk = kl^(alpha-1))
    kl_ss    = yk_ss^(1/(alpha-1));

    // labor normalized at N=1/3
    n_lab    = 1/3;
    k        = kl_ss * n_lab;
    y        = a * k^alpha * n_lab^(1-alpha);
    invest   = delta * k;

    // entrepreneurial net worth
    @#if WITH_FA
    nw       = q * k / s_ss^(1/chi);
    @#else
    nw       = q * k;
    @#endif

    // total equity (intermediate)
    Vss      = (rk - r)*q*k + r*nw;
    c_e      = (1-gamma_e) * Vss;

    // reverse-solve the endowment we (so F7 holds in steady state)
    we       = nw - gamma_e * Vss;

    // government spending
    g_ss     = gy_share * y;
    g        = g_ss;

    // household consumption (resource constraint F18)
    c        = y - invest - g - c_e;

    // reverse-solve labor disutility psi (so F2 holds in steady state)
    psi      = mc * (1-alpha) * y / (n_lab^(1+phi_n) * c^sigma);

    // Calvo steady state
    pstar    = 1;
    x1       = c^(-sigma) * mc * y / (1 - theta*betta);
    x2       = c^(-sigma) * y  / (1 - theta*betta);

    // steady-state level parameters (used in the model equations)
    y_ss     = y;
    rn_ss    = rn;

end;

//============================================================
// Model equations (20, F1-F20)
//============================================================
model;

// F1: household Euler equation
[name='euler']
c^(-sigma) = betta * r * c(+1)^(-sigma);

// F2: labor-market equilibrium (MRS = real wage)
[name='labor_market']
psi * n_lab^phi_n * c^sigma = mc * (1-alpha) * y / n_lab;

// F3: return on capital
[name='rk_identity']
rk = (mc * alpha * y / k(-1) + (1-delta) * q) / q(-1);

// F4: capital arbitrage (external finance premium times the risk-free rate = expected return on capital; forward-looking)
[name='capital_arbitrage']
rk(+1) = premium * r;

// F5: Tobin's Q (capital-goods producer first-order condition, CEE investment adjustment cost)
[name='tobin_q']
1 = q*(1 - phi_k/2*(invest/invest(-1)-1)^2 - phi_k*(invest/invest(-1)-1)*(invest/invest(-1)))
    + betta*(c(+1)^(-sigma)/c^(-sigma))*q(+1)*phi_k*(invest(+1)/invest-1)*(invest(+1)/invest)^2;

// F6: capital accumulation
[name='capital_accum']
k = (1-delta)*k(-1) + (1 - phi_k/2*(invest/invest(-1)-1)^2)*invest;

// F7/F8: financial-frictions block
@#if WITH_FA
[name='net_worth']
nw = gamma_e*((rk - r(-1))*q(-1)*k(-1) + r(-1)*nw(-1)) + we;
[name='premium_eq']
premium = (q*k/nw)^chi;
@#else
[name='net_worth']
nw = q*k;
[name='premium_eq']
premium = 1;
@#endif

// F9: entrepreneur consumption
[name='entrepreneur_cons']
c_e = (1-gamma_e)*((rk - r(-1))*q(-1)*k(-1) + r(-1)*nw(-1));

// F10: production function (with price dispersion)
[name='production']
y*delta_p = a*k(-1)^alpha*n_lab^(1-alpha);

// F11: Calvo recursion x1
[name='calvo_x1']
x1 = c^(-sigma)*mc*y + theta*betta*x1(+1)*infl(+1)^eps_p;

// F12: Calvo recursion x2
[name='calvo_x2']
x2 = c^(-sigma)*y + theta*betta*x2(+1)*infl(+1)^(eps_p-1);

// F13: optimal reset price
[name='optimal_price']
pstar = eps_p/(eps_p-1)*x1/x2;

// F14: price-index evolution
[name='price_index']
1 = theta*infl^(eps_p-1) + (1-theta)*pstar^(1-eps_p);

// F15: price-dispersion evolution
[name='price_disp']
delta_p = (1-theta)*pstar^(-eps_p) + theta*infl^eps_p*delta_p(-1);

// F16: Fisher equation
[name='fisher']
rn = r*infl(+1);

// F17: Taylor rule
[name='taylor']
rn/rn_ss = (rn(-1)/rn_ss)^rho_R * (infl^phi_pi*(y/y_ss)^phi_y)^(1-rho_R) * exp(eps_m);

// F18: resource constraint (Walras: bond-market clearing is implied by the budget constraint and is not written separately)
[name='resource']
y = c + invest + g + c_e;

// F19: TFP process
[name='tfp_process']
log(a) = rho_a*log(a(-1)) + eps_a;

// F20: government-spending process
[name='gov_process']
log(g) = (1-rho_g)*log(g_ss) + rho_g*log(g(-1)) + eps_g;

end;

//============================================================
// Steady state and Blanchard-Kahn check
//============================================================
steady;
resid(non_zero);
check;

//============================================================
// Shocks
//============================================================
shocks;
var eps_m; stderr 0.0025;   // monetary policy shock, 25 basis points
var eps_a; stderr 0.01;     // TFP shock, 1%
var eps_g; stderr 0.01;     // government-spending shock, 1%
end;

//============================================================
// Stochastic simulation (nograph: figures from plot_irfs_pub)
//============================================================
stoch_simul(order=1, irf=20, nograph, periods=0);
