// inventory: quiet_bvar
var y;
varexo e;
parameters rho;
rho = 0.9;
model;
y = rho * y(-1) + e;
end;
bvar_density 4;
bvar_forecast 2;
bvar_irf(12, 'name');
