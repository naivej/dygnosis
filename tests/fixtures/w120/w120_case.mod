// inventory: w120_case
var y;
varexo_det e;
parameters rho;
rho = 0.9;
model;
y = rho * y(-1) + e;
end;
Stoch_Simul;
