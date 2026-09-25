// inventory: p_hank_quiet_unused_het_endo
heterogeneity_dimension d;
var y;
var(heterogeneity=d) yh;
varexo e;
model;
y = e;
end;
model(heterogeneity=d);
y = y(-1);
end;
