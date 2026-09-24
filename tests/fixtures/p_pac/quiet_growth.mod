// inventory: p_pac_quiet_growth
var x;
parameters beta;
beta = .9;

model;
  x = x(-1);
end;

pac_model(model_name=p, discount=beta, growth=.1*x, auxname=px);
