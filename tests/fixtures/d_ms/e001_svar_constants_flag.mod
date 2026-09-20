// inventory: e001_svar_constants_flag
// `svar(constants, chain=1);` — `CONSTANTS` is a token `svar` does not take; 7.1 refuses `unexpected CONSTANTS`.
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
svar(constants, chain=1);
