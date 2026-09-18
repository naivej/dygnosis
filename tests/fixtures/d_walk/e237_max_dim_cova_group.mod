// inventory: e237_max_dim_cova_group
var y;
varexo e;
parameters rho;
rho = 0.5;
model;
y = rho * y(-1) + e;
end;
identification(max_dim_cova_group=0);
