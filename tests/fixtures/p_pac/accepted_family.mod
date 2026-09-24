// inventory: p_pac_quiet_family
var x y z v;
varexo ex ey ez;
parameters beta a;
beta = .9;
a = .2;

var_model(MODEL_NAME=aux_var, eqtags=['eq:x', 'eq:y'], structural);
trend_component_model(model_name=aux_trend, targets=['eq:x'], eqtags=['eq:x', 'eq:y']);
var_expectation_model(model_name=forecast_x, expression=x+y(-1), auxiliary_model_name=aux_var, horizon=1:Inf, discount=beta, time_shift=-1);
pac_model(model_name=pac_x, auxiliary_model_name=aux_var, discount=beta);

pac_target_info(pac_x);
  target v;
  auxname_target_nonstationary vns;
  component y;
  kind ll;
  auxname y_aux;
  component x;
  growth diff(x(-1));
  kind dd;
  auxname x_aux;
end;

model;
  [name='eq:x'] x = a*x(-1)+ex;
  [name='eq:y'] y = a*y(-1)+ey;
  [name='eq:v'] v = x+y;
  [name='eq:z'] z = var_expectation(forecast_x)+pac_expectation(pac_x)+pac_target_nonstationary(pac_x)+ez;
end;
