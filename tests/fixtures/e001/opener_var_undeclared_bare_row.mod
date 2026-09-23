// inventory: e001_opener_var_undeclared_bare_row
// A bare `shocks;` row inside the model body, with nothing declaring the name.
// 7.1 reads the row as an equation and refuses it with `Unknown symbol`.
var y;
varexo e;
parameters rho;
rho = 0.95;

model;
y = rho * y(-1) + e;
shocks;
end;

stoch_simul(order = 1, nograph);
