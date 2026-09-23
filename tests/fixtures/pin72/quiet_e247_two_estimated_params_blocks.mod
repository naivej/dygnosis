// inventory: pin72_quiet_e247_two_estimated_params_blocks
var y z;
varexo e e2;
parameters rho;
rho = .5;
model;
y = rho*y(-1) + e + e2;
z = y;
end;
estimated_params;
skew e, 0;
end;
estimated_params;
skew e, .1;
end;
