// inventory: quiet_heterogeneity
varexo e;
parameters rho;
rho = 0.9;
var(heterogeneity=d) yh;
model;
yh = rho * yh(-1) + e;
end;
