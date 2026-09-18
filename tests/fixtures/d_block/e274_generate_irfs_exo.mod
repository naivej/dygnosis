// inventory: e274_generate_irfs_exo
var y;
varexo e;
parameters rho;
rho = 0.9;
model;
y = rho * y(-1) + e;
end;
generate_irfs;
a, e = 1, e = 2;
end;
