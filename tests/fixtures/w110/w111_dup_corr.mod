// inventory: w110_w111_dup_corr
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
corr e, u = 0.1;
corr u, e = 0.2;
end;

steady_state_model;
y = 0;
c = 0;
end;
stoch_simul;
