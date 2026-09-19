// inventory: e331_init2shocks_second_not_exo
var y;
varexo e;
parameters rho;
rho = 0.9;
model;
y = rho * y(-1) + e;
end;
init2shocks;
y rho;
end;
