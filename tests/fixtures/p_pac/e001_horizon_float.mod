// inventory: p_pac_e001_horizon_float
var x;
parameters beta;
beta = .9;
model;
  x = x(-1);
end;

var_expectation_model(model_name=vexp, auxiliary_model_name=aux,
  expression=x, horizon=1.5, discount=beta);
