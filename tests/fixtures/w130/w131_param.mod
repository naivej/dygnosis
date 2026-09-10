// inventory: w131_psi_param
var y;
parameters rho psi;
varexo e;
rho = 0.9;
psi = 1;
model;
y = rho * y(-1) + e;
end;
steady_state_model;
y = 0;
psi = y / 0.5;
psi = 99;
end;
