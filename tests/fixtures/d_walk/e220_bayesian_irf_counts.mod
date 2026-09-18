// inventory: e220_bayesian_irf_counts
var y;
varexo e u;
parameters rho;
rho = 0.5;
model;
y = rho * y(-1) + e + u;
end;
varobs y;
estimated_params;
dsge_prior_weight, 0.5;
end;
estimation(dsge_var, bayesian_irf, datafile='d.csv');
