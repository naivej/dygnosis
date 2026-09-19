// inventory: quiet_trend
varexo e;
parameters rho;
rho = 0.9;
trend_var(growth_factor=1.02) A;
var(deflator=A) y;
model;
y = rho * y(-1) + e;
end;
