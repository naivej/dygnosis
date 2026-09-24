// inventory: p_pac_e001_empty_growth
var x;
parameters beta;
beta = .9;
model;
  x = x(-1);
end;

pac_model(model_name=p, discount=beta, growth=);
