// inventory: w200_stoch_abs
var y;
varexo e;
parameters rho;
rho = 0.9;
model;
y = abs(y(-1)) + e;
end;
stoch_simul;
