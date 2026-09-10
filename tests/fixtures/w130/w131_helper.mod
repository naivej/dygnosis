// inventory: w131_nss_helper
var y;
varexo e;
parameters rho;
rho = 0.9;
model;
y = rho * y(-1) + e;
end;
steady_state_model;
n_ss = 1/3;
n_ss = 0.5;
y = 0;
end;
