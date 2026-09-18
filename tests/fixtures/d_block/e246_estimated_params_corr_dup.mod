// inventory: e246_estimated_params_corr_dup
var y;
varexo e e2;
parameters rho;
rho = 0.9;
model;
y = rho * y(-1) + e + e2;
end;
estimated_params;
corr e, e2, 0.1;
corr e, e2, 0.2;
end;
