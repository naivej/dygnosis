// inventory: e307_trend_twice
var y;
varexo e;
parameters rho;
rho = 0.9;
trend_var(growth_factor=1.02) A;
trend_var(growth_factor=1.02) A;
model;
y = rho * y(-1) + e;
end;
