// inventory: e001_conditional_forecast_datafile
// `conditional_forecast(datafile=…)` — the option arrives after 7.1; at the pin it is `unexpected DATAFILE`.
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
conditional_forecast(datafile='x.csv', periods=8);
