// inventory: e001_delete_shocks_end
var y;
varexo e;
parameters rho;
rho = 0.5;
model;
y = rho * y(-1) + e;
end;
shocks;
var e; stderr 0.01;
