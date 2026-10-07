// ============================================================
// Trend RBC with government investment
// Trend RBC with public capital externality
//
// Mechanism: government investment -> public capital -> a production externality (Aschauer 1989)
//       trend growth rate gam (labor-augmenting technical progress, quarterly)
//       quantity variables are detrended by the technology level A_t
//
// References: Aschauer (1989, JME), Barro (1990, JPE),
//       King-Plosser-Rebelo (1988, QJE)
// ============================================================

// ---------- endogenous variables (16, all detrended) -------------------
var y        ${y}$          (long_name='output, detrended')
    c        ${c}$          (long_name='consumption, detrended')
    k        ${k}$          (long_name='private capital, detrended')
    n        ${n}$          (long_name='labor hours')
    invest   ${\imath}$     (long_name='private investment, detrended')
    ig       ${i_g}$        (long_name='government investment, detrended')
    kg       ${k_g}$        (long_name='public capital stock, detrended')
    z        ${z}$          (long_name='TFP shock, log level')
    w        ${w}$          (long_name='real wage, detrended')
    rk       ${r^k}$        (long_name='rental rate of private capital')
    log_y    (long_name='log output')
    log_c    (long_name='log consumption')
    log_k    (long_name='log private capital')
    log_kg   (long_name='log public capital')
    log_ig   (long_name='log government investment')
    log_n    (long_name='log labor');

// ---------- exogenous variables (innovations only, R3) -----------------------
varexo eps_z   ${\varepsilon_z}$   (long_name='TFP innovation')
       eps_ig  ${\varepsilon_g}$   (long_name='government investment innovation');

// ---------- parameters ----------------------------------------------
parameters betta    ${\beta}$        (long_name='household discount factor')
           gam      ${\gamma}$       (long_name='quarterly trend growth rate')
           delta    ${\delta}$       (long_name='private capital depreciation')
           deltag   ${\delta_g}$     (long_name='public capital depreciation')
           alppha   ${\alpha}$       (long_name='private capital income share')
           alphag   ${\alpha_g}$     (long_name='public capital externality')
           phhi     ${\varphi}$      (long_name='inverse Frisch elasticity')
           psi      ${\psi}$         (long_name='labor disutility, back-solved to hit n=1/3')
           rhoz     ${\rho_z}$       (long_name='TFP shock persistence')
           rho_ig   ${\rho_g}$       (long_name='government investment persistence')
           gy       ${g_y}$          (long_name='steady-state gov investment / output ratio')
           ig_ss    ${i_g^{ss}}$     (long_name='steady-state gov investment, back-solved');

// ---------- parameter calibration ------------------------------------------
// Standard RBC calibration (quarterly)
betta   = 0.99;     // quarterly discount factor -> about 4% annual
gam     = 0.005;    // quarterly trend growth -> 2% annual
delta   = 0.025;    // private-capital depreciation -> 10% annual
deltag  = 0.05;     // public-capital depreciation -> 20% annual (infrastructure)

// Production-function elasticities
alppha  = 0.33;     // private-capital share
alphag  = 0.10;     // public-capital externality elasticity (Aschauer 1989)

// Household preferences
phhi    = 1.0;      // inverse Frisch elasticity (= 1, the standard setting)
// psi is reverse-solved in steady_state_model to hit n = 1/3

// Shock processes
rhoz    = 0.95;     // TFP persistence
rho_ig  = 0.80;     // government-investment persistence

// Fiscal structure
gy      = 0.05;     // steady-state government investment / output = 5%
// ig_ss is reverse-solved in steady_state_model to hit the gy target

// ============================================================
// Model equations (16, one per endogenous variable, R4)
// Timing: k, kg and ig are state variables (stock at the end of the period, R2)
//       y, c, n, invest, w and rk are control variables (they jump in the current period)
// ============================================================
model;

// --- production ---

// (1) production function (log utility, detrended; public capital is an externality, so kg(-1), R2)
[name='production function']
y = exp(z) * kg(-1)^alphag * k(-1)^alppha * n^(1-alppha-alphag);

// (2) labor-demand first-order condition (firm, F3)
[name='labor demand FOC']
w = (1-alppha-alphag) * y / n;

// (3) rental-rate first-order condition (firm, F4)
[name='capital rental rate FOC']
rk = alppha * y / k(-1);

// --- household ---

// (4) consumption Euler equation (log utility; detrended discount factor = betta/(1+gam), F1)
[name='consumption Euler equation']
c^(-1) = (betta/(1+gam)) * c(+1)^(-1) * (rk(+1) + 1 - delta);

// (5) labor-supply first-order condition (F2)
[name='labor supply FOC']
psi * n^phhi * c = w;

// --- capital accumulation ---

// (6) private capital accumulation (detrended, R2: k is the stock at the end of the period)
[name='private capital law of motion']
(1+gam) * k = (1-delta) * k(-1) + invest;

// (7) public capital accumulation (detrended, R2: kg is the stock at the end of the period)
[name='public capital law of motion']
(1+gam) * kg = (1-deltag) * kg(-1) + ig;

// --- market clearing ---

// (8) resource constraint (goods-market clearing; Walras: the household budget constraint is redundant)
[name='resource constraint']
y = c + invest + ig;

// --- exogenous processes ---

// (9) TFP shock (E1; AR(1), z endogenous, eps_z the innovation, R3)
[name='TFP process']
z = rhoz * z(-1) + eps_z;

// (10) government-investment process (E2; log AR(1), ig_ss is the steady-state level)
[name='government investment process']
log(ig) = (1-rho_ig)*log(ig_ss) + rho_ig*log(ig(-1)) + eps_ig;

// --- log-level helper variables (for IRFs, so a deviation reads as a percent) ---

// (11)-(16) log level of each quantity
[name='log output']
log_y = log(y);

[name='log consumption']
log_c = log(c);

[name='log private capital']
log_k = log(k);

[name='log public capital']
log_kg = log(kg);

[name='log government investment']
log_ig = log(ig);

[name='log labor']
log_n = log(n);

end;

// ============================================================
// Closed-form steady state (steady_state_model, in order; derivation note section 6)
// psi and ig_ss are reverse-solved here, replacing the placeholders in parameters
// ============================================================
steady_state_model;
    z = 0;

    // step 1: rental rate on capital (steady state of the Euler equation)
    rk = (1+gam)/betta - (1-delta);

    // step 2: key ratios
    ky  = alppha / rk;                   // private capital / output
    kgy = gy / (gam + deltag);           // public capital / output

    // step 3: steady-state labor normalization
    n_ss = 1/3;

    // step 4: steady-state output (from the production function and the ratios)
    // y = (kgy*y)^alphag * (ky*y)^alppha * n^(1-alppha-alphag)
    // => y^(1-alppha-alphag) = kgy^alphag * ky^alppha * n^(1-alppha-alphag)
    // => y = (kgy^alphag * ky^alppha)^(1/(1-alppha-alphag)) * n
    y = (kgy^alphag * ky^alppha)^(1/(1-alppha-alphag)) * n_ss;

    // step 5: the other quantity variables
    k      = ky * y;
    kg     = kgy * y;
    invest = (gam + delta) * k;
    ig_ss  = gy * y;                     // reverse-solve the parameter ig_ss
    ig     = ig_ss;
    c      = y - invest - ig;
    w      = (1-alppha-alphag) * y / n_ss;
    rk_chk = alppha * y / k;             // check (must equal rk)

    // step 6: reverse-solve the labor-disutility weight psi (hit n = 1/3)
    psi = w / (n_ss^phhi * c);

    n = n_ss;

    // steady state of the log-level helper variables
    log_y  = log(y);
    log_c  = log(c);
    log_k  = log(k);
    log_kg = log(kg);
    log_ig = log(ig);
    log_n  = log(n);
end;

// ============================================================
// Check: residuals, steady state, Blanchard-Kahn conditions
// ============================================================
resid;
steady;
check;

// ============================================================
// Stochastic simulation: shocks and stoch_simul
// ============================================================
shocks;
    var eps_z;   stderr 0.01;    // TFP shock standard deviation = 1%
    var eps_ig;  stderr 0.02;    // government-investment shock standard deviation = 2%
end;

// First-order approximation; IRFs for 40 periods; nograph (figures from plot_irfs_pub.m)
stoch_simul(order=1, irf=40, nograph, hp_filter=1600)
    y c invest ig kg n w rk
    log_y log_c log_k log_kg log_ig log_n;
