// inventory: e322_extfun_no_name
var y;
varexo e;
parameters rho;
rho = 0.9;
model;
y = rho * y(-1) + e;
end;
external_function(nargs=1);
