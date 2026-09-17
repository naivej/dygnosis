// inventory: e213_pf_order
var y;
varexo e;
parameters rho;
rho = 0.9;
model;
y = rho * y(-1) + e;
end;
perfect_foresight_solver;
