// inventory: d_hank_quiet_cross_dimension
heterogeneity_dimension d, e;
var y;
var(heterogeneity=d) a;
var(heterogeneity=e) b;
model;
y = SUM(a) + SUM(b);
end;
model(heterogeneity=d);
a = b + a(-1);
end;
model(heterogeneity=e);
b = b(-1);
end;
