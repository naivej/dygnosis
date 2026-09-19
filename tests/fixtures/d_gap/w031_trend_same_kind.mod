// inventory: w031_trend_same_kind
var y;
varexo e;
parameters rho;
rho = 0.9;
model;
y = rho * y(-1) + e;
end;
trend_var(growth_factor=1.02) A, A;
