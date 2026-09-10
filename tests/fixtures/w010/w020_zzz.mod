// inventory: w020_unused_endo
var y c z;
varexo e;
parameters rho betta;
rho = 0.9;
betta = 0.99;
model;
y = rho * y(-1) + e;
c = betta * c(+1);
y = y;
end;
steady_state_model;
y = 0;
c = 0;
z = 0;
end;
