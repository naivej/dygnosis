// inventory: e332_homotopy_not_param
var y;
varexo e;
parameters rho;
rho = 0.9;
model;
y = rho * y(-1) + e;
end;
homotopy_setup;
y, 0, 1;
end;
