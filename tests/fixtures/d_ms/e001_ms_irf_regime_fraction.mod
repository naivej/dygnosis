// inventory: e001_ms_irf_regime_fraction
// `ms_irf(regime=1.5) y;` — `regime` takes an integer; 7.1 refuses `unexpected FLOAT_NUMBER`.
var y c k;
varexo e;
parameters alpha beta gamma;
alpha = 0.36;
beta = 0.99;
gamma = 0.5;
model;
c = alpha*y + beta*c(-1) + e;
y = beta*y(-1) + c;
k = y;
end;
initval;
y = 0;
c = 0;
k = 0;
end;
shocks;
var e; stderr 0.1;
end;
ms_irf(regime=1.5) y;
