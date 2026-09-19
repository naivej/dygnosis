// inventory: quiet_optim_weights
var y z;
varexo e;
parameters rho;
rho = 0.9;
model;
y = rho * y(-1) + e;
z = y;
end;
osr_params rho;
optim_weights;
y 1;
y, z 2;
end;
osr;
