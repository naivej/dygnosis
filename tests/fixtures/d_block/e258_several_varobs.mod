// inventory: e258_several_varobs
var y;
varexo e;
parameters rho;
rho = 0.9;
model;
y = rho * y(-1) + e;
end;
varobs y;
varobs y;
