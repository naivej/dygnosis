// inventory: w131_n_const
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
n = 1/3;
y = 0;
end;
