// inventory: e330_init2shocks_first_not_endo
var y;
varexo e;
parameters rho;
rho = 0.9;
model;
y = rho * y(-1) + e;
end;
init2shocks;
rho e;
end;
