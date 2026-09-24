// inventory: d_pac_e449_no_pac_use
var x;
varexo e;
parameters beta;
beta=.9;
model;
  x=.5*x(-1)+e;
end;
pac_model(model_name=p,discount=beta);
