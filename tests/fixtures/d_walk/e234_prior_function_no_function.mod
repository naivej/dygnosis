// inventory: e234_prior_function_no_function
var y;
varexo e;
parameters rho;
rho = 0.5;
model;
y = rho * y(-1) + e;
end;
prior_function(sampling_draws=10);
