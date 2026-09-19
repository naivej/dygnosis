// inventory: e329_init2shocks_dup
var y;
varexo e;
parameters rho;
rho = 0.9;
model;
y = rho * y(-1) + e;
end;
init2shocks;
y e;
y e;
end;
