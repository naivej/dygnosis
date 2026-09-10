// inventory: e024_timed_det_exo
var y;
varexo_det tau;
parameters rho;
rho = 0.5;
model;
y = rho * y(-1) + tau(-1);
end;
