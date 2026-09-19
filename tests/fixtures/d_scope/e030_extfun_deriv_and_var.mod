// inventory: e030_extfun_deriv_and_var
var bar;
var y;
varexo e;
parameters rho;
rho = 0.9;
model;
y = rho * y(-1) + e;
end;
external_function(name='foo', first_deriv_provided='bar');
