// inventory: e058_init2shocks_undeclared
var y;
varexo e;
parameters rho;
rho = 0.9;
model;
y = rho * y(-1) + e;
end;
init2shocks;
y zzz;
end;
