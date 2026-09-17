// inventory: e208_static_dynamic
var y;
varexo e;
parameters rho;
rho = 0.9;
model;
[static] y = 0;
y = rho * y(-1) + e;
end;
