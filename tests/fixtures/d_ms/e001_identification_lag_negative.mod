// inventory: e001_identification_lag_negative
// `exclusion lag -1;` — the lag is unsigned; 7.1 refuses `unexpected MINUS, expecting INT_NUMBER`.
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
svar_identification;
exclusion lag -1;
equation 1, y;
end;
