// inventory: e259_several_varexobs
var y;
varexo e;
parameters rho;
rho = 0.9;
model;
y = rho * y(-1) + e;
end;
varexobs e;
varexobs e;
