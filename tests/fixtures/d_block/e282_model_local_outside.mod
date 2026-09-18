// inventory: e282_model_local_outside
var y;
varexo e;
parameters rho;
rho = 0.9;
model;
#z = y;
y = rho * y(-1) + e;
end;
rho = z;
