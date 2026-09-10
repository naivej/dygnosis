// inventory: e010_extra
var y;
varexo e;
parameters rho;
rho = 0.5;

model;
y = rho * y(-1) + e;
y = 0;
end;
