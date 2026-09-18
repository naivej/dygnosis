// inventory: e267_shock_stderr_param
var y;
varexo e;
parameters rho;
rho = 0.9;
model;
y = rho * y(-1) + e;
end;
shocks;
var rho;
stderr 0.01;
end;
