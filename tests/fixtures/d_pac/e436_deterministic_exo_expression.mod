// inventory: d_pac_e436_deterministic_exo_expression
var y z;
varexo e e2;
varexo_det d;
parameters b;
b = 0.8;
model;
  [name='Y'] y = b*y(-1)+e+d;
  [name='Z'] z = z(-1)+e2;
end;
var_model(model_name=v,eqtags=['Y']);
var_expectation_model(model_name=a,expression=d,auxiliary_model_name=v,horizon=1);
