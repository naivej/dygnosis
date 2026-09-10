// inventory: e001_shocks_var_no_semi
var y;
varexo e;
parameters rho;
rho = 0.5;
model;
y = rho * y(-1) + e;
end;
shocks;
var e stderr 0.01;
end;
