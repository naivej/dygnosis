// inventory: d_pac_e440_pac_duplicate
var y z;
varexo e e2;
parameters b;
b = 0.8;
model;
  [name='Y'] y = b*y(-1)+e;
  [name='Z'] z = z(-1)+e2;
end;
pac_model(model_name=q,discount=b); pac_model(model_name=q,discount=b);
