// inventory: p_pac_e001_pac_growth_binary
var x;
parameters beta;
beta = .9;

model;
  x = x(-1);
end;

pac_model(model_name=p, discount=beta, growth=x+);
