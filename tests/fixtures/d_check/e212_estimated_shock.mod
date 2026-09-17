// inventory: e212_estimated_shock
var y;
varexo e;
parameters rho;
rho = 0.9;
model;
y = rho * y(-1) + e;
end;
estimated_params;
rho, 0.8, 0, 1;
end;
shocks;
var e; stderr rho;
end;
