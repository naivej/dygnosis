// inventory: d_pac_e434_unary_duplicate_lhs
var y z;
varexo e e2;
parameters b;
b = 0.8;
model;
  [name='Y'] y = b*y(-1)+e;
  [name='Z'] -y = b*y(-1)+e2;
  [name='Extra'] z = z(-1)+e2;
end;
var_model(model_name=v,eqtags=['Y','Z']);
