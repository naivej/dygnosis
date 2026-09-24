// inventory: p_hank_quiet_het_exo_body_use
heterogeneity_dimension d;
var(heterogeneity=d) yh;
varexo(heterogeneity=d) eh;
var y;
varexo e;
model;
y = e;
end;
model(heterogeneity=d);
yh = eh;
end;