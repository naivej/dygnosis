// inventory: d_pac_e446_pac_aux
var y z;
varexo e e2;
parameters b;
b = 0.8;
model;
  [name='Y'] y = b*y(-1)+e;
  [name='Z'] z = z(-1)+e2;
end;
pac_model(model_name=q,discount=b,auxiliary_model_name=missing);
