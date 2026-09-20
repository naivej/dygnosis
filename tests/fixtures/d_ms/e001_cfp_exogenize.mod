// inventory: e001_cfp_exogenize
// `exogenize` inside `conditional_forecast_paths` — a token with no production; 7.1 refuses `unexpected EXOGENIZE, expecting VAR`.
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
conditional_forecast_paths;
exogenize e;
periods 1;
values 0.1;
end;
