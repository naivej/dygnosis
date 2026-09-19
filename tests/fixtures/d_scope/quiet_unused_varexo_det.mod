// inventory: quiet_unused_varexo_det
var y;
varexo_det ed;
parameters rho;
rho = 0.9;
model;
y = rho * y(-1);
end;
