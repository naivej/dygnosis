// inventory: p_hank_quiet_two_dimensions
var y;
varexo e;
parameters rho;
rho = 0.9;
heterogeneity_dimension d1, d2;
var(heterogeneity=d1) y1;
var(heterogeneity=d2) y2;
model;
y = rho*y(-1) + e + SUM(y1) + SUM(y2);
end;
model(heterogeneity=d1);
y1 = rho*y1(-1);
end;
model(heterogeneity=d2);
y2 = rho*y2(-1) + e;
end;