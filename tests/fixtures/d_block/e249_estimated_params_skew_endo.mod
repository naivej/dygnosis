// inventory: e249_estimated_params_skew_endo
var y;
varexo e;
parameters rho;
rho = 0.9;
model;
y = rho * y(-1) + e;
end;
estimated_params;
skew y, 0;
end;
