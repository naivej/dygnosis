// inventory: e010_ramsey_gap
var y c;
varexo e;
parameters rho;
rho = 0.5;

model;
y = rho * y(-1) + e;
end;

planner_objective y;
ramsey_model(instruments=(y));
