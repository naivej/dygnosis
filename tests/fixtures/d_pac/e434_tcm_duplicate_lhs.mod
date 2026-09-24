// inventory: d_pac_e434_tcm_duplicate_lhs
var y z;
varexo e e2;
parameters b;
b = 0.8;
model;
  [name='Y'] diff(y) = b*(y(-1)-z(-1))+e;
  [name='Z'] diff(y) = b*(y(-1)-z(-1))+e2;
  [name='Extra'] z = z(-1)+e2;
end;
trend_component_model(model_name=t,eqtags=['Y','Z'],targets=['Z']);
