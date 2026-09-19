// inventory: e030_trend_and_var
var y;
varexo e;
parameters rho;
rho = 0.9;
model;
y = rho * y(-1) + e;
end;
var A;
trend_var(growth_factor=1.02) A;
