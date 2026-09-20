// inventory: e001_conditional_forecast_controlled_varexo_int
// `controlled_varexo=1` — the production wants `( names )`; 7.1 refuses `unexpected INT_NUMBER, expecting '('`.
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
conditional_forecast(periods=8, parameter_set=prior_mean, controlled_varexo=1);
