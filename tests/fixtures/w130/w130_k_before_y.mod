// inventory: w130_k_before_y
var y k;
varexo e;
parameters rho;
rho = 0.9;
model;
y = rho * y(-1) + e;
end;
steady_state_model;
k = y;
y = 1;
end;
