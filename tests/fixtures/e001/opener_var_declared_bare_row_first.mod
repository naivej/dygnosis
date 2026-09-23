// inventory: e001_opener_var_declared_bare_row_first
// The same clause with the bare row first in the body: position does not matter, and
// 7.1 accepts the file either way.
var y shocks;
varexo e;
parameters rho;
rho = 0.95;

model;
shocks;
y = rho * y(-1) + e + shocks;
end;

initval;
y = 0;
shocks = 0;
end;

shocks;
var e; stderr 0.01;
end;

stoch_simul(order = 1, nograph);
