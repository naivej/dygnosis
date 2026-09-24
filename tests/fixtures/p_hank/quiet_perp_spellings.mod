// inventory: p_hank_quiet_perp_spellings
heterogeneity_dimension d;
var(heterogeneity=d) y1, y2;
model(heterogeneity=d);
y1 = 1 ⟂ y1 >= 0;
y2 = 2 _|_ y2 >= 0;
end;