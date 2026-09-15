// inventory: companions_ss_present
// Convention FILENAME_steadystate.m is present; no initval / steady_state_model.
var y;
varexo e;
parameters rho;
rho = 0.5;

model;
y = rho * y(-1) + e;
end;

steady;
