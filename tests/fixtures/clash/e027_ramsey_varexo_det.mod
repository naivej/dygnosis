// inventory: e027_ramsey_varexo_det
var y;
varexo_det tau;
parameters rho betta;
rho = 0.9;
betta = 0.99;
model;
y = rho * y(-1) + tau;
end;
initval;
y = 0;
tau = 0;
end;
planner_objective y;
ramsey_model(instruments=(y), planner_discount=0.99);
