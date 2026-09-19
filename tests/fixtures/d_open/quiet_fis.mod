// inventory: quiet_fis
var y;
varexo e;
parameters rho;
rho = 0.9;
model;
y = rho * y(-1) + e(-2);
end;
filter_initial_state;
y(0) = 0;
e(-1) = 0.1;
end;
