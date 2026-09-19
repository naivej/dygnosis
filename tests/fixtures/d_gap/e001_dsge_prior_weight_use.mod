// inventory: e001_dsge_prior_weight_use
var y dsge_prior_weight;
varexo e;
parameters rho;
rho = 0.9;
model;
y = rho * y(-1) + e;
dsge_prior_weight = 0;
end;
