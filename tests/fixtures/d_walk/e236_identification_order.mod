// inventory: e236_identification_order
var y;
varexo e;
parameters rho;
rho = 0.5;
model;
y = rho * y(-1) + e;
end;
identification(order=4);
