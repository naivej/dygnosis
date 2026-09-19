// inventory: e317_optim_weights_not_endo
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
e 1;
end;
osr;
