// inventory: p_pac_e279_vem_discount_external
var y;
varexo e;
parameters b;
b=.8;
external_function(name=helper);
model;
  [name='Y'] y=b*y(-1)+e;
end;
var_model(model_name=v,eqtags=['Y']);
var_expectation_model(model_name=a,variable=y,auxiliary_model_name=v,horizon=1,discount=helper);
