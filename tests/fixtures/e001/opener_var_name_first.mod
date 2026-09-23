// inventory: e001_opener_var_name_first
// Position in the list does not change the reading: the name may lead it.
var shocks y;
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
