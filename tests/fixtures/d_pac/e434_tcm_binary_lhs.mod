// inventory: d_pac_e434_tcm_binary_lhs
var y z;
varexo e e2;
parameters b;
b = 0.8;
model;
  [name='Y'] 2*y = b*(y(-1)-z(-1))+e;
  [name='Z'] z = z(-1)+e2;
end;
trend_component_model(model_name=t,eqtags=['Y','Z'],targets=['Z']);
