// inventory: w022_unused_assigned
var y c;
varexo e;
parameters rho betta unused_p;
rho = 0.9;
betta = 0.99;
unused_p = 0.5;

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
stoch_simul;
