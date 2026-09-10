// inventory: w022_ss_only
var y c;
varexo e;
parameters rho betta dummy_p;
rho = 0.9;
betta = 0.99;
dummy_p = 0;

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
y = dummy_p;
end;
stoch_simul;
