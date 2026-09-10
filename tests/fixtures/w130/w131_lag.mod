// inventory: w131_lag_ok
var y n;
varexo e;
parameters rho;
rho = 0.9;
model;
y = rho * y(-1) + e;
n = y;
end;
steady_state_model;
n = 1;
n = n(-1);
y = 0;
end;
