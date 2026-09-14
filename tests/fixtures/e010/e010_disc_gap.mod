// inventory: e010_disc_gap
var y c;
varexo e;
parameters rho;
rho = 0.5;

model;
y = rho * y(-1) + e;
end;

planner_objective y;
discretionary_policy(instruments=(y));
