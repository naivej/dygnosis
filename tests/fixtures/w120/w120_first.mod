// inventory: w120_first
var y;
varexo_det e;
parameters rho;
rho = 0.9;
model;
y = rho * y(-1) + e;
end;
estimation;
stoch_simul;
