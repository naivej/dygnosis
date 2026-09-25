// compare: heterogeneous equation edit stays out of the aggregate diff
var y;
varexo e;
parameters rho;
rho = 0.9;
heterogeneity_dimension d;
var(heterogeneity=d) yh;
model;
[name='agg'] y = rho*y(-1) + e;
end;
model(heterogeneity=d);
yh = yh(-1);
end;
