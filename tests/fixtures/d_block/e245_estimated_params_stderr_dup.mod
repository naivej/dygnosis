// inventory: e245_estimated_params_stderr_dup
var y;
varexo e;
parameters rho;
rho = 0.9;
model;
y = rho * y(-1) + e;
end;
estimated_params;
stderr e;
stderr e;
end;
