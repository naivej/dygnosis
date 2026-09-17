// inventory: e200_write_latex
var y;
varexo e;
parameters rho;
rho = 0.9;
model;
y = rho * y(-1) + e;
end;
write_latex_steady_state_model;
