// inventory: d_pac_e432_nested_local_second_selected
var y z;
var(log) w;
varexo e e2 e3;
parameters b;
b = 0.8;
model;
  #first = w(-1);
  #second = first;
  [name='Y'] y = b*y(-1)+e;
  [name='Z'] z = b*z(-1)+second+e2;
  [name='W'] w = b*w(-1)+e3;
end;
var_model(model_name=v,eqtags=['Y','Z']);
