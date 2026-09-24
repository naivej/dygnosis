// inventory: d_pac_e434_tcm_current
var y z;
varexo e e2;
parameters b;
b = 0.8;
model;
  [name='Y'] diff(y) = b*z+e;
  [name='Z'] z = z(-1)+e2;
end;
trend_component_model(model_name=t,eqtags=['Y','Z'],targets=['Z']);
