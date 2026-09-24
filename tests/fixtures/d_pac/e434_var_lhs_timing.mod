// inventory: d_pac_e434_var_lhs_timing
var y z;
varexo e e2;
parameters b;
b = 0.8;
model;
  [name='Y'] y(+1) = b*y(-1)+e;
  [name='Z'] z = z(-1)+e2;
end;
var_model(model_name=v,eqtags=['Y']);
