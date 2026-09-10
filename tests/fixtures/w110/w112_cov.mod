// inventory: w110_w112_cov_neg
var y;
varexo e u;
parameters rho;
rho = 0.9;
model;
y = rho * y(-1) + e + u;
end;
shocks;
var e, u = -0.01;
end;
