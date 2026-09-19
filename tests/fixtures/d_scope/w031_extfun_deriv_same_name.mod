// inventory: w031_extfun_deriv_same_name
var y;
varexo e;
parameters rho;
rho = 0.9;
model;
y = rho * y(-1) + e;
end;
external_function(name='foo', first_deriv_provided='foo');
