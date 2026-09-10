// inventory: w010_unassigned_referenced
var y c;
varexo e;
parameters rho betta orphan_p;
rho = 0.9;
betta = 0.99;

model;
y = rho * y(-1) + e;
c = betta * c(+1);
y = rho * y(-1) + e + 0 * orphan_p;
end;

shocks;
var e; stderr 0.01;
end;

steady_state_model;
y = 0;
c = 0;
end;
stoch_simul;
