// inventory: d_pac_e020_preempts_pac_check
var y z;
varexo e e2;
parameters b;
b = 0.8;
model;
  [name='Y'] y = b*y(-1)+e+ghost;
  [name='Z'] z = z(-1)+e2;
end;
pac_model(model_name=q,discount=b); pac_target_info(q); auxname_target_nonstationary yns; component z; auxname zaux; kind dd; end;
