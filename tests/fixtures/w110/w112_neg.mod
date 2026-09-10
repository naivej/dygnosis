// inventory: w110_w112_neg
var y;
varexo e;
parameters rho;
rho = 0.9;
model;
y = rho * y(-1) + e;
end;
shocks;
var e = -0.01;
end;
