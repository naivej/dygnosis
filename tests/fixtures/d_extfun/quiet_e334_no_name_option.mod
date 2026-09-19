// inventory: quiet_e334_no_name_option
var y;
varexo e;
parameters rho;
rho = 0.9;
model;
y = rho * y(-1) + e;
end;
external_function(first_deriv_provided='bar', second_deriv_provided='bar');
