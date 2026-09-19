// inventory: e030_trend_mixed
var y;
varexo e;
parameters rho;
rho = 0.9;
model;
y = rho * y(-1) + e;
end;
trend_var(growth_factor=1.02) A;
log_trend_var(log_growth_factor=1.02) A;
