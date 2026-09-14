// inventory: check_dir_clean
var y;
varexo e;
parameters rho;
rho = 0.9;

model;
y = rho * y(-1) + e;
end;

shocks;
var e; stderr 0.01;
end;

initval;
y = 0;
end;

steady_state_model;
y = 0;
end;
