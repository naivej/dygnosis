// inventory: e209_tags_ramsey
var y;
varexo e;
parameters rho;
rho = 0.9;
model;
[static] y = 0;
[dynamic] y = rho * y(-1) + e;
end;
planner_objective y;
ramsey_model;
