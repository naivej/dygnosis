// inventory: check_dir_warn
var y;
varexo e;
parameters rho orphan_p;
rho = 0.9;

model;
y = rho * y(-1) + e + 0 * orphan_p;
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
