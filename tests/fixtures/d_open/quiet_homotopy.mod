// inventory: quiet_homotopy
var y;
varexo e;
varexo_det ed;
parameters rho;
rho = 0.9;
model;
y = rho * y(-1) + e + ed;
end;
homotopy_setup;
rho, 0, 1;
ed, 0.5;
end;
