// inventory: e328_extfun_first_top_second_named
var y;
varexo e;
parameters rho;
rho = 0.9;
model;
y = rho * y(-1) + e;
end;
external_function(name='foo', first_deriv_provided='foo', second_deriv_provided='baz');
