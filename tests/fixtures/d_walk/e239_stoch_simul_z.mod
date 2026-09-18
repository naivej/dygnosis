// inventory: e239_stoch_simul_z
var y;
varexo e;
parameters rho;
rho = 0.5;
model;
y = rho * y(-1) + e;
end;
stoch_simul z;
