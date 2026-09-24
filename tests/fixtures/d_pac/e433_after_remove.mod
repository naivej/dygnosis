// inventory: d_pac_e433_after_remove
var y z;
varexo e e2;
parameters b;
b = 0.8;
model;
  [name='Y'] y = b*y(-1)+e;
  [name='Z'] z = z(-1)+e+e2;
end;
model_remove('Y');
var_model(model_name=v,eqtags=['Y']);
