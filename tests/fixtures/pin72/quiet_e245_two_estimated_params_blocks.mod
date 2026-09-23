// inventory: pin72_quiet_e245_two_estimated_params_blocks
var y z;
varexo e e2;
parameters rho;
rho = .5;
model;
y = rho*y(-1) + e + e2;
z = y;
end;
estimated_params;
stderr e, normal_pdf, .5, .1;
end;
estimated_params;
stderr e, normal_pdf, .6, .1;
end;
