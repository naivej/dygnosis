// inventory: shape_w042_missing_ss
var y c;
varexo e;
parameters rho betta;
rho = 0.9;
betta = 0.99;
model;
y = rho * y(-1) + e;
c = betta * c(+1);
end;
steady_state_model;
y = 0;
end;
