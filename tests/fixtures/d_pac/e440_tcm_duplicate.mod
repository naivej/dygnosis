// inventory: d_pac_e440_tcm_duplicate
var y z;
varexo e e2;
parameters b;
b = 0.8;
model;
  [name='Y'] y = b*y(-1)+e;
  [name='Z'] z = z(-1)+e2;
end;
trend_component_model(model_name=t,eqtags=['Y'],targets=['Y']); trend_component_model(model_name=t,eqtags=['Z'],targets=['Z']);
