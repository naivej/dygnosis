// inventory: pin72_quiet_e244_two_estimated_params_bounds_blocks
var y z;
varexo e e2;
parameters rho;
rho = .5;
model;
y = rho*y(-1) + e + e2;
z = y;
end;
estimated_params_bounds;
rho, 0, 1;
end;
estimated_params_bounds;
rho, 0, 2;
end;
