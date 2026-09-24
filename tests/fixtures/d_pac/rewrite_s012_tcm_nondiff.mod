// inventory: d_pac_rewrite_s012_tcm_nondiff
var y z;
varexo e e2;
parameters b;
b = 0.8;
model;
  [name='Y'] y = b*(y(-1)-z(-1))+e;
  [name='Z'] z = z(-1)+e2;
end;
trend_component_model(model_name=t,eqtags=['Y','Z'],targets=['Z']);
