// inventory: w110_corr_abs_1
var y c;
varexo e u;
parameters rho betta;
rho = 0.9;
betta = 0.99;

model;
y = rho * y(-1) + e;
c = betta * c(+1);
y = rho * y(-1) + e + u;
end;

shocks;
var e; stderr 0.01;
corr e, u = 1;
end;

steady_state_model;
y = 0;
c = 0;
end;
stoch_simul;
