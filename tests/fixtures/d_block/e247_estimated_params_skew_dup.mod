// inventory: e247_estimated_params_skew_dup
var y;
varexo e;
parameters rho;
rho = 0.9;
model;
y = rho * y(-1) + e;
end;
estimated_params;
skew e, 0;
skew e, 0.1;
end;
