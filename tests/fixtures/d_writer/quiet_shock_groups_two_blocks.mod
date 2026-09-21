// inventory: quiet_shock_groups_two_blocks
var y;
varexo e1, e2;
parameters rho;
rho = 0.9;
model;
y = rho*y(-1) + e1 + e2;
end;
shock_groups;
g1 = e1;
end;
shock_groups;
g1 = e2;
end;
