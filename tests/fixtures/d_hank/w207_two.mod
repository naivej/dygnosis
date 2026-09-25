// inventory: d_hank_w207_two
heterogeneity_dimension d, e;
var y;
var(heterogeneity=d) a;
var(heterogeneity=e) b;
model;
y = SUM(a);
end;
model(heterogeneity=d);
a = a(-1);
end;
model(heterogeneity=e);
b = b(-1);
end;
