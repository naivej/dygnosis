// inventory: e313_fis_dup
var y;
varexo e;
parameters rho;
rho = 0.9;
model;
y = rho * y(-1) + e;
end;
filter_initial_state;
y(0) = 0;
y(0) = 1;
end;
