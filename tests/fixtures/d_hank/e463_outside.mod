// inventory: d_hank_e463_outside
heterogeneity_dimension d;
var y;
varexo e;
parameters theta;
var(heterogeneity=d) a;
theta = a;
varexo(heterogeneity=d) eh;
parameters(heterogeneity=d) ph;
model;
y = theta * SUM(a) + e;
end;
model(heterogeneity=d);
a = a(-1) + eh + ph;
end;
