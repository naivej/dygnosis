// inventory: p_hank_fire_w013_het_use
heterogeneity_dimension d;
var y z;
var(heterogeneity=d) yh;
model;
y = 1;
end;
model(heterogeneity=d);
yh = z;
end;
