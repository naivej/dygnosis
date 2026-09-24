// inventory: d_pac_quiet_nested_local_lag
var y z;
varexo e e2;
parameters b;
b = 0.8;
model;
  #past1 = y(-1);
  #past2 = past1;
  [name='Y'] y = b*y(-1)+past2+e;
  [name='Z'] z = z(-1)+e2;
end;
var_model(model_name=v,eqtags=['Y']);
