// inventory: w130_timed_rhs_native
var y k;
varexo e;
parameters rho;
rho = 0.9;
model;
y = rho * y(-1) + e;
k = y;
end;
steady_state_model;
k = y(-1);
y = 1;
end;
