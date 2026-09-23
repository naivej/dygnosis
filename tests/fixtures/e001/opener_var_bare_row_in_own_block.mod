// inventory: e001_opener_var_bare_row_in_own_block
// A bare `shocks;` row inside a real `shocks` body. The rows of that block are
// `var` / `corr` / `skew` statements, so 7.1 refuses the row whether or not the
// spelling is declared.
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
var e;
shocks;
stderr 0.01;
end;
