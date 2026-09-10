// inventory: w022_stderr_only
var y c;
varexo e;
parameters rho betta sigma_z;
rho = 0.9;
betta = 0.99;
sigma_z = 0.01;

model;
y = rho * y(-1) + e;
c = betta * c(+1);
end;

shocks;
var e; stderr 0.01;
var e; stderr sigma_z;
end;

steady_state_model;
y = 0;
c = 0;
end;
stoch_simul;
