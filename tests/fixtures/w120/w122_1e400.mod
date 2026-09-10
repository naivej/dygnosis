// inventory: w122_1e400
var y c;
varexo e;
parameters rho betta;
rho = 0.9;
betta = 0.99;
rho = 1e400;

model;
y = rho * y(-1) + e;
c = betta * c(+1);
end;

shocks;
var e; stderr 0.01;
end;

steady_state_model;
y = 0;
c = 0;
end;
steady;
stoch_simul;
