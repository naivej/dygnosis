// inventory: p_hank_fire_het_pound_twice
heterogeneity_dimension d;
var(heterogeneity=d) yh;
model(heterogeneity=d);
#a = 1;
#a = 2;
yh = a;
end;