// inventory: p_pac_e001_vem_discount_timing
var y;
varexo e;
parameters b;
b=.8;
model;
  [name='Y'] y=b*y(-1)+e;
end;
var_model(model_name=v,eqtags=['Y']);
var_expectation_model(model_name=a,variable=y,auxiliary_model_name=v,horizon=1,discount=y(-1));
