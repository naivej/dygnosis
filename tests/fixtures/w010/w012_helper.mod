// inventory: w012_helper_before_block
var y c;
varexo e;
parameters rho betta;
rho = 0.9;
betta = 0.99;
helper_foo = 1.5;

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
