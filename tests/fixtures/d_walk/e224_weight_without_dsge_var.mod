// inventory: e224_weight_without_dsge_var
var y;
varexo e;
parameters rho;
rho = 0.5;
model;
y = rho * y(-1) + e;
end;
estimated_params;
dsge_prior_weight, 0.5;
end;
estimation(datafile='d.csv');
