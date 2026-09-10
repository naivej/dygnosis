// inventory: e001_strip_last_eq_semi
var y;
varexo e;
parameters rho;
rho = 0.5;
model;
y = rho * y(-1) + e
end;
