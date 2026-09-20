// inventory: e001_cfp_endogenize
// `endogenize` inside `conditional_forecast_paths` — 7.1 refuses `unexpected ENDOGENIZE, expecting VAR`.
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
endogenize y;
periods 1;
values 0.1;
end;
