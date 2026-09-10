// inventory: e001_variable_inside_model_not_var_typo
var y variable;
varexo e;
parameters rho;
rho = 0.5;
model;
y = rho * y(-1) + e;
variable = y;
end;
