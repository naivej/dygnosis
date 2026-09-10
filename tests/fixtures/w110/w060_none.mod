// inventory: w110_w060_drop
var y;
varexo e u;
parameters rho;
rho = 0.9;
model;
y = rho * y(-1) + e + u;
end;
