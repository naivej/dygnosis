// inventory: e314_fis_lag_mismatch
var y;
varexo e;
parameters rho;
rho = 0.9;
model;
y = rho * y(-1) + e;
end;
filter_initial_state;
y(-2) = 0;
end;
