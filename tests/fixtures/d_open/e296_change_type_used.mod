// inventory: e296_change_type_used
var y;
varexo e;
parameters rho;
rho = 0.9;
model;
y = rho * y(-1) + e;
end;
change_type(varexo) y;
