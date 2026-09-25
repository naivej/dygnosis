// inventory: p_hank_fire_e025_same_tree_other_def
heterogeneity_dimension d;
var y;
var(heterogeneity=d) yh;
model;
y = a;
#a = 1;
end;
model(heterogeneity=d);
#a = 1;
yh = a;
end;
