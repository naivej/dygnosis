// inventory: e311_fis_not_endo
var y;
varexo e;
parameters rho;
rho = 0.9;
model;
y = rho * y(-1) + e;
end;
filter_initial_state;
rho(0) = 0.9;
end;
