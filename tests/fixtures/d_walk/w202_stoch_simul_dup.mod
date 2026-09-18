// inventory: w202_stoch_simul_dup
var y;
varexo e;
parameters rho;
rho = 0.5;
model;
y = rho * y(-1) + e;
end;
stoch_simul y, y;
