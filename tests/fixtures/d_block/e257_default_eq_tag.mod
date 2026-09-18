// inventory: e257_default_eq_tag
var y;
varexo e;
parameters rho;
rho = 0.9;
model;
[name='2'] y = rho * y(-1) + e;
y + y = 0;
end;
