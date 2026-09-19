// inventory: quiet_e334_own_name_function
var y;
varexo e;
parameters rho;
rho = 0.9;
model;
y = rho * y(-1) + e;
end;
external_function(name='bar', first_deriv_provided='bar', second_deriv_provided='bar');
