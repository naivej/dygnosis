// inventory: e221_shocks_lt_varobs
var y z;
varexo e;
parameters rho;
rho = 0.5;
model;
y = rho * y(-1) + e;
z = y;
end;
varobs y z;
estimated_params;
dsge_prior_weight, 0.5;
end;
estimation(dsge_var, datafile='d.csv');
