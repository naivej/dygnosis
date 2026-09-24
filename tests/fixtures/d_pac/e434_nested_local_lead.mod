// inventory: d_pac_e434_nested_local_lead
var y z;
varexo e e2;
parameters b;
b = 0.8;
model;
  #ahead1 = y(+1);
  #ahead2 = ahead1;
  [name='Y'] y = b*y(-1)+ahead2+e;
  [name='Z'] z = z(-1)+e2;
end;
var_model(model_name=v,eqtags=['Y']);
