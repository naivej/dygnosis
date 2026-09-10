// inventory: w131_exp_ok
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
n = exp(n);
y = 0;
end;
