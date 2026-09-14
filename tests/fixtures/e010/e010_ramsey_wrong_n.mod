// inventory: e010_ramsey_wrong_n
var y c z;
varexo e;
parameters rho;
rho = 0.5;

model;
y = rho * y(-1) + e;
end;

planner_objective y;
ramsey_model(instruments=(y));
