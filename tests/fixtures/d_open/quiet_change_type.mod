// inventory: quiet_change_type
var y;
parameters rho;
rho = 0.9;
change_type(varexo) y;
model;
y = rho * y(-1);
end;
