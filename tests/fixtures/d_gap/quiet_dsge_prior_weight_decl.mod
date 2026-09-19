// inventory: quiet_dsge_prior_weight_decl
var y dsge_prior_weight;
varexo e;
parameters rho;
rho = 0.9;
model;
y = rho * y(-1) + e;
end;
estimated_params;
dsge_prior_weight, 0.5;
end;
