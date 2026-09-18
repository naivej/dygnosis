// inventory: e223_weight_and_calibrated
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
estimation(dsge_var=0.5, datafile='d.csv');
