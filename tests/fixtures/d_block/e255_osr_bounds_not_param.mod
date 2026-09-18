// inventory: e255_osr_bounds_not_param
var y;
varexo e;
parameters rho;
rho = 0.9;
model;
y = rho * y(-1) + e;
end;
planner_objective y;
osr;
osr_params rho;
osr_params_bounds;
y, 0, 1;
end;
