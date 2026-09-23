// inventory: pin72_quiet_e315_two_optim_weights_blocks
var y z;
varexo e e2;
parameters rho;
rho = .5;
model;
y = rho*y(-1) + e + e2;
z = y;
end;
optim_weights;
y 1;
end;
optim_weights;
y 2;
end;
