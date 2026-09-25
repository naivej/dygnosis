// inventory: d_hank_w207_with_lag
heterogeneity_dimension d, e;
var y;
varexo(heterogeneity=d) eh;
var(heterogeneity=d) a;
var(heterogeneity=e) b;
model;
y = SUM(a);
end;
model(heterogeneity=d);
a = eh(-1);
end;
model(heterogeneity=e);
b = b(-1);
end;
