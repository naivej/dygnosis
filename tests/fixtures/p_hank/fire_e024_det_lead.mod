// inventory: p_hank_fire_e024_det_lead
heterogeneity_dimension d;
varexo_det ed;
var(heterogeneity=d) yh;
model(heterogeneity=d);
yh = ed(-1);
end;