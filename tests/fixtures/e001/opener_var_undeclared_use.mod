// inventory: e001_opener_var_undeclared_use
// The name is a block opener and nothing declares it, so 7.1 reads the equation
// and refuses it with `Unknown symbol`. That is the neighbouring **E020** path.
var y;
varexo e;
parameters rho;
rho = 0.95;

model;
y = rho * y(-1) + e + shocks;
end;

stoch_simul(order = 1, nograph);
