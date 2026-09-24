// inventory: p_hank_fire_het_model_mixed_options
heterogeneity_dimension d;
var(heterogeneity=d) yh;
model(heterogeneity=d, linear);
yh = 1;
end;