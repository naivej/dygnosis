// inventory: quiet_extfun_deriv_names
var y;
varexo e;
parameters rho;
rho = 0.9;
model;
y = rho * y(-1) + e;
end;
external_function(name='foo', first_deriv_provided='bar', second_deriv_provided='baz');
