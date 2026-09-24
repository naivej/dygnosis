// inventory: p_hank_quiet_het_pound_local
heterogeneity_dimension d;
var(heterogeneity=d) yh;
model(heterogeneity=d);
#a = 1;
yh = a + yh(-1);
end;