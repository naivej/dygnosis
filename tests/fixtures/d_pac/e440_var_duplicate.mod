// inventory: d_pac_e440_var_duplicate
var y z;
varexo e e2;
parameters b;
b = 0.8;
model;
  [name='Y'] y = b*y(-1)+e;
  [name='Z'] z = z(-1)+e2;
end;
var_model(model_name=v,eqtags=['Y']); var_model(model_name=v,eqtags=['Z']);
