// inventory: p_pac_e310_vem_discount_trend
var y;
varexo e;
parameters b;
trend_var(growth_factor=1.01) A;
b=.8;
model;
  [name='Y'] y=b*y(-1)+e;
end;
var_model(model_name=v,eqtags=['Y']);
var_expectation_model(model_name=a,variable=y,auxiliary_model_name=v,horizon=1,discount=A);
