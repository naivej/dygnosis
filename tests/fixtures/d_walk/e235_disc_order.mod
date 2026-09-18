// inventory: e235_disc_order
var y;
varexo e;
parameters rho;
rho = 0.5;
model;
y = rho * y(-1) + e;
end;
planner_objective y;
discretionary_policy(order=2, instruments=(y));
