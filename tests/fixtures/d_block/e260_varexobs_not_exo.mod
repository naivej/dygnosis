// inventory: e260_varexobs_not_exo
var y;
varexo e;
parameters rho;
rho = 0.9;
model;
y = rho * y(-1) + e;
end;
varexobs y;
