// inventory: p_pac_e001_pac_kind_bad
var x;
parameters beta;
beta = .9;
model;
  x = x(-1);
end;

pac_model(model_name=p, discount=beta, kind=other);
