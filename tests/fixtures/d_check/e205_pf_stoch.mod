// inventory: e205_pf_stoch
var y;
varexo e;
parameters rho;
rho = 0.9;
model;
y = rho * y(-1) + e;
end;
perfect_foresight_setup;
perfect_foresight_solver;
stoch_simul;
