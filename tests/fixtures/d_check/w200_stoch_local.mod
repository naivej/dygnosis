// inventory: w200_stoch_local
var y;
varexo e;
parameters rho;
rho = 0.9;
model;
# z = abs(y);
y = rho * y(-1) + e;
end;
stoch_simul;
