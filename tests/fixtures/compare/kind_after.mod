// compare: kind changes, including det and heterogeneous
varexo e;
var(heterogeneity=d) y;
varexo_det u;
heterogeneity_dimension d;
parameters beta;
beta = 0.95;
model;
y = beta*y(-1)+e+u;
end;
