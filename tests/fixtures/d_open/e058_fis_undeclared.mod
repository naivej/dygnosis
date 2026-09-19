// inventory: e058_fis_undeclared
var y;
varexo e;
parameters rho;
rho = 0.9;
model;
y = rho * y(-1) + e;
end;
filter_initial_state;
zzz(0) = 0;
end;
