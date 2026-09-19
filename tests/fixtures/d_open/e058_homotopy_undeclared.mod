// inventory: e058_homotopy_undeclared
var y;
varexo e;
parameters rho;
rho = 0.9;
model;
y = rho * y(-1) + e;
end;
homotopy_setup;
zzz, 0, 1;
end;
