// inventory: e028_sensitivity_varexo_det
var y;
varexo_det tau;
parameters rho;
rho = 0.5;
model;
y = rho * y(-1) + tau;
end;
initval;
y = 0;
tau = 0;
end;
sensitivity(identification=1);
