// inventory: e268_shock_cov_mixed
var y;
varexo e;
parameters rho;
rho = 0.9;
model;
y = rho * y(-1) + e;
end;
shocks;
var y, e = 0.01;
end;
