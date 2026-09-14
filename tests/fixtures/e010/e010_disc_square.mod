// inventory: e010_disc_square
var y c;
varexo e;
parameters rho;
rho = 0.5;

model;
y = rho * y(-1) + e;
c = 0;
end;

planner_objective y;
discretionary_policy(instruments=(y));
