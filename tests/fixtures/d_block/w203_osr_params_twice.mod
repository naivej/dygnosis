// inventory: w203_osr_params_twice
var y;
varexo e;
parameters rho;
rho = 0.9;
model;
y = rho * y(-1) + e;
end;
planner_objective y;
osr_params rho;
osr_params rho;
osr;
