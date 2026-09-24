// inventory: p_hank_fire_e021_unused_exogenous
heterogeneity_dimension d;
var(heterogeneity=d) yh;
var y;
varexo e, e2;
model;
y = e;
end;
model(heterogeneity=d);
yh = yh(-1);
end;