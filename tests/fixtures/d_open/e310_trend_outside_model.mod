// inventory: e310_trend_outside_model
var y;
varexo e;
parameters rho;
rho = 0.9;
trend_var(growth_factor=1.02) A;
model;
y = rho * y(-1) + e;
end;
rho = A;
