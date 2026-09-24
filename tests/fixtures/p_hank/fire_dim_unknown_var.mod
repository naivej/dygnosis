// inventory: p_hank_fire_dim_unknown_var
varexo e;
parameters rho;
rho = 0.9;
var(heterogeneity=d) yh;
model;
yh = rho * yh(-1) + e;
end;