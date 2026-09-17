// inventory: e202_disc_ramsey
var y;
varexo e;
parameters rho;
rho = 0.9;
model;
y = rho * y(-1) + e;
end;
planner_objective y;
ramsey_model(instruments=(y));
discretionary_policy(instruments=(y));
