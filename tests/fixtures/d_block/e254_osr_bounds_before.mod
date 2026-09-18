// inventory: e254_osr_bounds_before
var y;
varexo e;
parameters rho;
rho = 0.9;
model;
y = rho * y(-1) + e;
end;
planner_objective y;
osr;
osr_params_bounds;
rho, 0, 1;
end;
osr_params rho;
