// inventory: e326_extfun_nargs_mismatch
var y;
varexo e;
parameters rho;
rho = 0.9;
model;
y = rho * y(-1) + e;
end;
external_function(name='foo', nargs=1);
external_function(name='foo', nargs=2);
