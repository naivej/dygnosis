// inventory: d_ms_e239_undeclared_irf
// `ms_irf` takes a trailing symbol list, so the shipped E239 reaches it.
var y c k;
varexo e;
parameters alpha beta;
alpha = 0.36;
beta = 0.99;
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
ms_irf zzz;
