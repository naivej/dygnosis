// inventory: e001_strip_betta_semi
var y;
varexo e;
parameters rho betta;
rho = 0.5;
betta = 0.99
gam = 0.005;
model;
y = rho * y(-1) + e;
end;
