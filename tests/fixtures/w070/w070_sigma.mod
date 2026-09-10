// inventory: w070_sigma_e
var y c;
varexo e;
parameters rho betta sigma_e;
rho = 0.9;
betta = 0.99;
sigma_e = -0.1;

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
