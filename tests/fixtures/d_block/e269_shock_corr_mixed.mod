// inventory: e269_shock_corr_mixed
var y;
varexo e;
parameters rho;
rho = 0.9;
model;
y = rho * y(-1) + e;
end;
shocks;
corr y, e = 0.1;
end;
