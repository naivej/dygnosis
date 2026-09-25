// inventory: d_hank_e474_extended
heterogeneity_dimension d;
var y;
varexo e;
parameters theta;
theta = 0.2;
var(heterogeneity=d) a;
varexo(heterogeneity=d) eh;
parameters(heterogeneity=d) ph;
model;
y = theta * SUM(a) + e;
end;
model(heterogeneity=d);
a = a(-1) + eh + ph;
end;

extended_path(periods=10);
