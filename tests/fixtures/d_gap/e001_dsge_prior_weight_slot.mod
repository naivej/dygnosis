// inventory: e001_dsge_prior_weight_slot
var y;
varexo e;
parameters rho;
rho = 0.9;
model;
y = rho * y(-1) + e;
end;
osr_params rho;
optim_weights;
dsge_prior_weight 1;
end;
osr;
