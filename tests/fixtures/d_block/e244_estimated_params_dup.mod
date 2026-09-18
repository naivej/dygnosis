// inventory: e244_estimated_params_dup
var y;
varexo e;
parameters rho;
rho = 0.9;
model;
y = rho * y(-1) + e;
end;
estimated_params;
rho;
rho;
end;
