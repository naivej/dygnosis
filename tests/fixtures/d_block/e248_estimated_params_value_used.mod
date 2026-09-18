// inventory: e248_estimated_params_value_used
var y;
varexo e;
parameters rho alpha;
rho = 0.9;
alpha = 0.5;
model;
y = rho * y(-1) + e;
end;
estimated_params;
rho, 0.5;
alpha, rho;
end;
