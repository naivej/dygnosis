// inventory: e001_refuse_macro
@#define C = 1
y_@{C}
var y;
varexo e;
parameters rho betta;
rho = 0.5;
betta = 0.99
model;
y = rho * y(-1) + e;
end;
