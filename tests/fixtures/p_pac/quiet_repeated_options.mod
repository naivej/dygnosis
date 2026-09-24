// inventory: p_pac_quiet_repeated_options
var x y;
parameters beta;
beta = .9;

model;
  x = x(-1);
  y = y(-1);
end;

var_model(model_name=aux, eqtags=['eq:x']);
var_expectation_model(model_name=vexp, expression=x, expression=y,
  auxiliary_model_name=aux, horizon=1, discount=.5, discount=beta);
pac_model(model_name=p, discount=beta, growth=.1*x, growth=.2*x,
  auxname=first_aux, auxname=second_aux);
