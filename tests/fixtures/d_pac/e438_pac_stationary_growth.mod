// inventory: d_pac_e438_pac_stationary_growth
var y z;
varexo e e2;
parameters b;
b = 0.8;
model;
  [name='Y'] y = b*y(-1)+e;
  [name='Z'] z = z(-1)+e2;
end;
pac_model(model_name=q,discount=b); pac_target_info(q); target y; auxname_target_nonstationary yns; component z; auxname zaux; growth y; kind ll; end;
