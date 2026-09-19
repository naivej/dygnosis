// inventory: quiet_shock_groups
var y;
varexo e;
parameters rho;
rho = 0.9;
model;
y = rho * y(-1) + e;
end;
shock_groups;
'grp' = e;
end;
