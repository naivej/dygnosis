// inventory: written_e192_quiet — square heterogeneous dimension
heterogeneity_dimension h;
var(heterogeneity=h) c n;
model(heterogeneity=h);
c=n;
n=n(-1);
end;
