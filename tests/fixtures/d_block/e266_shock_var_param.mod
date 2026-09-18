// inventory: e266_shock_var_param
var y;
varexo e;
parameters rho;
rho = 0.9;
model;
y = rho * y(-1) + e;
end;
shocks;
var rho = 0.01;
end;
