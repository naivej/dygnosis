// inventory: e058_shock_groups_undeclared
var y;
varexo e;
parameters rho;
rho = 0.9;
model;
y = rho * y(-1) + e;
end;
shock_groups;
g = zzz;
end;
