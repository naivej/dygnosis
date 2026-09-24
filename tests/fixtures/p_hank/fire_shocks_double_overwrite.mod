// inventory: p_hank_fire_shocks_double_overwrite
heterogeneity_dimension d;
var(heterogeneity=d) yh;
varexo(heterogeneity=d) eh;
model(heterogeneity=d);
yh = eh;
end;
shocks(heterogeneity=d, overwrite, overwrite);
var eh; stderr 0.1;
end;