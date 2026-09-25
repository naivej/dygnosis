// inventory: d_hank_e473_nonsep
heterogeneity_dimension d;
var(heterogeneity=d) a;
model(heterogeneity=d);
a = log(a(-1)+a(+1));
end;
