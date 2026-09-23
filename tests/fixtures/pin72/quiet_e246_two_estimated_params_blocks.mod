// inventory: pin72_quiet_e246_two_estimated_params_blocks
var y z;
varexo e e2;
parameters rho;
rho = .5;
model;
y = rho*y(-1) + e + e2;
z = y;
end;
estimated_params;
corr e,e2, normal_pdf, .1, .1;
end;
estimated_params;
corr e,e2, normal_pdf, .2, .1;
end;
