// inventory: w120_det
var y;
varexo_det e;
parameters rho;
rho = 0.9;
model;
y = rho * y(-1) + e;
end;
stoch_simul;
