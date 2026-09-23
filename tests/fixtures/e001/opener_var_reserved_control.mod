// inventory: e001_opener_var_reserved_control
// The control: a declaration keyword in the list still ends it, and 7.1 refuses the
// file with `syntax error, unexpected VAR`.
var y var;
varexo e;
parameters rho;
rho = 0.95;

model;
y = rho * y(-1) + e;
end;
