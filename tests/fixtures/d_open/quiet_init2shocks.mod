// inventory: quiet_init2shocks
var y;
varexo e;
parameters rho;
rho = 0.9;
model;
y = rho * y(-1) + e;
end;
init2shocks(name=g1);
y e;
end;
