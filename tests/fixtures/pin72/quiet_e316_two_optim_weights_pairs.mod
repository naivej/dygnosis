// inventory: pin72_quiet_e316_two_optim_weights_pairs
var y z;
varexo e e2;
parameters rho;
rho = .5;
model;
y = rho*y(-1) + e + e2;
z = y;
end;
optim_weights;
y,z 1;
end;
optim_weights;
y,z 2;
end;
