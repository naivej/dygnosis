// inventory: p_hank_fire_het_comp_bad
heterogeneity_dimension d;
var(heterogeneity=d) yh;
model(heterogeneity=d);
yh = 1 ⟂ yh + 1;
end;