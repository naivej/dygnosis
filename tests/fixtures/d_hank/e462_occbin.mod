// inventory: d_hank_e462_occbin
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

occbin_constraints;
name 'ELB';
bind a > 0;
end;
