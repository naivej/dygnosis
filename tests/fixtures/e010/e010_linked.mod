// inventory: e010_drop_log_n_linked
var y z;
varexo e;
parameters rho;
rho = 0.5;
model;
y = rho * y(-1) + e;
end;
