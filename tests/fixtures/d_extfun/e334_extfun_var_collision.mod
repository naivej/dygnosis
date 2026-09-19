// inventory: e334_extfun_var_collision
var y;
var bar;
varexo e;
parameters rho;
rho = 0.9;
model;
y = rho * y(-1) + e;
end;
external_function(name='foo', first_deriv_provided='bar', second_deriv_provided='bar');
