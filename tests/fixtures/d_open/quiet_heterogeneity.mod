// inventory: quiet_heterogeneity
varexo e;
parameters rho;
rho = 0.9;
heterogeneity_dimension d;
var(heterogeneity=d) yh;
var c, k;
model;
c = SUM(yh);
k = rho*k(-1) + e;
end;
model(heterogeneity=d);
yh = rho*yh(-1) + e;
end;
