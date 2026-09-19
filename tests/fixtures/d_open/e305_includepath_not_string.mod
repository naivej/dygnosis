// inventory: e305_includepath_not_string
@#includepath 1
var y;
varexo e;
parameters rho;
rho = 0.9;
model;
y = rho * y(-1) + e;
end;
