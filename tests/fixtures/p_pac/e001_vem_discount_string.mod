// inventory: p_pac_e001_vem_discount_string
var x;
parameters beta;
beta = .9;

model;
  x = x(-1);
end;

var_expectation_model(model_name=vexp, auxiliary_model_name=aux,
  expression=x, horizon=1, discount='beta');
