// inventory: p_hank_fire_w022_het_only
heterogeneity_dimension d;
var y;
varexo e;
parameters p;
p = 1;
var(heterogeneity=d) yh;
varexo(heterogeneity=d) eh;
parameters(heterogeneity=d) ph;
model(heterogeneity=d);
yh = ph*eh + y + e;
end;
