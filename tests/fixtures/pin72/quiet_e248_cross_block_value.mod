// inventory: pin72_quiet_e248_cross_block_value
var y z;
varexo e e2;
parameters rho alpha;
rho = .5;
alpha = .4;
model;
y = rho*y(-1) + e + e2;
z = y;
end;
estimated_params;
rho, .5;
end;
estimated_params;
alpha, rho;
end;
