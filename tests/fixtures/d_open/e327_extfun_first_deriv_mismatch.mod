// inventory: e327_extfun_first_deriv_mismatch
var y;
varexo e;
parameters rho;
rho = 0.9;
model;
y = rho * y(-1) + e;
end;
external_function(name='foo', nargs=1, first_deriv_provided='f1');
external_function(name='foo', nargs=1, first_deriv_provided='f2');
