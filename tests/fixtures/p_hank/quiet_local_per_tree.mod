// inventory: p_hank_quiet_local_per_tree
heterogeneity_dimension d1, d2;
var y;
var(heterogeneity=d1) y1;
var(heterogeneity=d2) y2;
model;
#a = 1;
y = a;
end;
model(heterogeneity=d1);
#a = 1;
y1 = a;
end;
model(heterogeneity=d2);
#a = 1;
y2 = a;
end;
