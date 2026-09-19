// inventory: e309_deflator_nonstationary
var y z w;
varexo e;
parameters rho;
rho = 0.9;
trend_var(growth_factor=1.02) A;
var(deflator=A) z;
var(deflator=z) w;
model;
y = rho * y(-1) + e;
z = y;
w = z;
end;
