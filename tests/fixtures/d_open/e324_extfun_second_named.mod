// inventory: e324_extfun_second_named
var y;
varexo e;
parameters rho;
rho = 0.9;
model;
y = rho * y(-1) + e;
end;
external_function(name='foo', second_deriv_provided='sd');
