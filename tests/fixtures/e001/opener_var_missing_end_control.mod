// inventory: e001_opener_var_missing_end_control
// The control: a genuinely missing `end;` before a real `initval;`.
var y;
varexo e;
parameters rho;
rho = 0.95;

model;
y = rho * y(-1) + e;
initval;
y = 0;
end;
