// inventory: d_pac_e443_vem_shift
var y z;
varexo e e2;
parameters b;
b = 0.8;
model;
  [name='Y'] y = b*y(-1)+e;
  [name='Z'] z = z(-1)+e2;
end;
var_expectation_model(model_name=a,variable=y,auxiliary_model_name=v,horizon=1,time_shift=1);
