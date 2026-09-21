// inventory: w205_shock_groups_label_reused
var y;
varexo e1, e2;
parameters rho;
rho = 0.9;
model;
y = rho*y(-1) + e1 + e2;
end;
shock_groups;
g1 = e1;
g1 = e2;
end;
