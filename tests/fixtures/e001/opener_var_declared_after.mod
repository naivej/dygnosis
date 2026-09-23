// inventory: e001_opener_var_declared_after
// The declaration sits after the model block, so the name is not yet in 7.1's
// symbol table when the equation is read: the file is refused with `Unknown symbol`.
varexo e;
parameters rho;
rho = 0.95;

model;
y = rho * y(-1) + e + matched_irfs;
matched_irfs = 0.1 * y;
end;

var y matched_irfs;

initval;
y = 0;
matched_irfs = 0;
end;

shocks;
var e; stderr 0.01;
end;
