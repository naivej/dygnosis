// inventory: e333_shock_groups_not_exo
var y;
varexo e;
parameters rho;
rho = 0.9;
model;
y = rho * y(-1) + e;
end;
shock_groups;
g = rho;
end;
