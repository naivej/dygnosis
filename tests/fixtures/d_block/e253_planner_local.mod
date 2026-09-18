// inventory: e253_planner_local
var y;
varexo e;
parameters rho;
rho = 0.9;
model;
#z = y;
y = rho * y(-1) + e;
end;
ramsey_model;
planner_objective z;
