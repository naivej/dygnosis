// inventory: e303_dsge_prior_weight_parameter
var y;
varexo e;
parameters rho, dsge_prior_weight;
rho = 0.9;
model;
y = rho * y(-1) + e;
end;
