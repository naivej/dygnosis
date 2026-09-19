// inventory: quiet_extfun
var y;
varexo e;
parameters rho;
rho = 0.9;
external_function(name=foo, nargs=1);
model;
y = rho * y(-1) + foo(e);
end;
