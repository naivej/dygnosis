// inventory: e233_estimated_planner_discount
var y;
varexo e;
parameters beta rho;
beta = 0.99;
rho = 0.5;
model;
y = rho * y(-1) + e;
end;
planner_objective y;
ramsey_model(planner_discount=beta);
estimated_params;
beta, 0.99;
end;
estimation(datafile='d.csv');
