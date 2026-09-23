// inventory: e001_opener_var_in_own_block
// A declared `shocks` beside a real `shocks` block: 7.1 accepts the pair.
var y shocks;
varexo e;
parameters rho;
rho = 0.95;

model;
y = rho * y(-1) + e + shocks;
shocks = 0.1 * y;
end;

initval;
y = 0;
shocks = 0;
end;

shocks;
var e; stderr 0.01;
end;

stoch_simul(order = 1, nograph);
