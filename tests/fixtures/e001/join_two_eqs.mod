// inventory: e001_join_two_model_eqs
var y c;
varexo e;
parameters rho betta;
rho = 0.5;
betta = 0.99;
model;
y = rho * y(-1) + e
c = betta * c(+1);
end;
