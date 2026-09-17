// inventory: e204_osr_both
var y;
varexo e;
parameters rho betta;
rho = 0.9;
betta = 0.99;
model;
y = rho * y(-1) + e;
end;
planner_objective y^2;
osr_params betta;
optim_weights;
y 1;
end;
osr;
