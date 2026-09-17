// inventory: w140_linear_exo_pf
var y;
varexo e;
parameters rho;
rho = 0.9;
model(linear);
y = abs(e);
end;
perfect_foresight_setup;
perfect_foresight_solver;
